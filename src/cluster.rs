/**
 * Copyright (c) 2024-2025 Glaive, Inc.
 *
 * This file is part of Glaive Gateway
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use pingora::lb::LoadBalancer;
use pingora::prelude::{background_service, RoundRobin, TcpHealthCheck};
use pingora::services::background::GenBackgroundService;

use crate::bucket::CacheBucket;
use crate::config::auth::AuthType;
use crate::config::cache::CacheType;
use crate::config::discovery::DiscoveryType;
use crate::discovery::{Discovery, DiscoveryBackgroundService};
use crate::config::cluster::ClusterConfig;
use crate::config::consumer::Consumer;
use crate::config::limiter::Limiter;
use crate::config::route::Route;

// build the cluster with hardcoded upstream
pub fn build_cluster_service(
    upstreams: &[&str],
) -> GenBackgroundService<LoadBalancer<RoundRobin>> {
    let mut cluster = LoadBalancer::try_from_iter(upstreams).unwrap();
    // upstream health check
    // frequency is set to every 1 second by default
    let hc = TcpHealthCheck::new();
    cluster.set_health_check(hc);
    cluster.health_check_frequency = Some(Duration::from_secs(1));
    // return the healthcheck & cluster background processing
    background_service("default cluster healthcheck", cluster)
}

// validate clusters configuration
fn validate_cluster_config(config: &ClusterConfig) -> bool {
    // mandatory cluster identity
    if config.name.is_none() || config.prefix.is_none() || config.host.is_none() || config.tls.is_none() {
        println!("CLUSTER IDENTITY ERROR");
        return false;
    }
    // mandatory cluster prefix formatter
    if let Some(prefix) = &config.prefix {
        if prefix.is_empty() || !prefix.starts_with('/') || prefix.ends_with('/') {
            println!("CLUSTER PREFIX ERROR");
            return false;
        }
    }
    // mandatory upstream check
    for upstream in &config.upstream {
        if upstream.is_empty() {
            println!("CLUSTER UPSTREAM ERROR");
            return false;
        }
    }
    true
}

// validate duplicates upstream prefix
fn validate_duplicated_prefix(clusters: &[ClusterConfig]) -> bool {
    let mut seen = HashSet::new();
    for cluster in clusters {
        if !seen.insert(&cluster.prefix) {
            return true;
        }
    }
    false
}

// Validate if any cluster has a discovery configuration
fn has_discovery_enabled(clusters: &[ClusterConfig]) -> bool {
    // Return true if any cluster has discovery configuration
    clusters.iter().any(|cluster| cluster.discovery.is_some())
}

// cluster metadata is mandatory
// the metadata is used for configuring upstream cluster
pub struct ClusterMetadata {
    pub name: String,
    pub host: String,
    pub tls: bool,
    pub limiter: Option<Limiter>,
    pub cache_storage: Option<CacheBucket>,
    pub cache_ttl: Option<usize>,
    pub retry: Option<usize>,
    pub timeout: Option<u64>,
    pub auth: Option<AuthType>,
    pub consumers: Option<Vec<Consumer>>,
    pub routes: Option<Vec<Route>>,
    pub upstream: Arc<LoadBalancer<RoundRobin>>,
}

impl ClusterMetadata {
    pub fn get_name(&self) -> &String {
        &self.name
    }
    pub fn get_host(&self) -> &String {
        &self.host
    }
    pub fn get_tls(&self) -> &bool {
        &self.tls
    }
    pub fn get_rate_limit(&self) -> &Option<Limiter> {
        &self.limiter
    }
    pub fn get_cache_storage(&self) -> &Option<CacheBucket> {
        &self.cache_storage
    }
    pub fn get_cache_ttl(&self) -> &Option<usize> {
        &self.cache_ttl
    }
    pub fn get_retry(&self) -> &Option<usize> {
      &self.retry
    }
    pub fn get_timeout(&self) -> &Option<u64> {
        &self.timeout
    }
    pub fn get_auth(&self) -> &Option<AuthType> {
        &self.auth
    }
    pub fn get_consumers(&self) -> &Option<Vec<Consumer>> {
        &self.consumers
    }
    pub fn get_routes(&self) -> &Option<Vec<Route>> {
        &self.routes
    }
    pub fn get_upstream(&self) -> &Arc<LoadBalancer<RoundRobin>> {
        &self.upstream
    }
}

// return type for built clusters
pub struct BuiltClusters {
    pub clusters: Vec<ClusterMetadata>,
    pub prefix_map: HashMap<String, usize>,
    pub cluster_bg_service: Vec<GenBackgroundService<LoadBalancer<RoundRobin>>>,
    pub updater_bg_service: Vec<GenBackgroundService<DiscoveryBackgroundService>>,
}

// build the entire cluster from the configuration
pub fn build_cluster(
    yaml_clusters_configuration: Vec<ClusterConfig>
) -> BuiltClusters {
    // Validate if there is prefix duplication
    match validate_duplicated_prefix(&yaml_clusters_configuration) {
        true => panic!("found duplicated upstream prefix"),
        false => {}
    }

    // checks if some of the config provide discovery
    // when found, create the discovery instances
    let discovery = match has_discovery_enabled(&yaml_clusters_configuration) {
        true => {
            println!("discovery found! creating consul connection...");
            Some(Discovery::new_consul_discovery())
        }
        false => None
    };

    // Declare a mutable list for built process to be added as background processing
    let mut cluster_background_process = Vec::new();
    let mut updater_background_process = Vec::new();
    // Declare a mutable list for clusters and prefix for the proxy router
    let mut clusters: Vec<ClusterMetadata> = Vec::new();
    let mut prefix_map = HashMap::new();

    // Iterate to build each cluster based on the configuration
    for (idx, cluster_conf) in yaml_clusters_configuration.into_iter().enumerate() {
        // Validate cluster config
        match validate_cluster_config(&cluster_conf) {
            true => {},
            false => panic!("invalid upstream configuration")
        }

        // Check if cluster uses discovery, otherwise build the hardcoded upstream uri
        let cluster_service = match cluster_conf.discovery {
            // if the cluster used discovery
            Some(discovery_type) => {
                // select and build based on discovery strategy
                match discovery_type {
                    // consul strategy
                    DiscoveryType::Consul { consul } => {
                        let discovery = discovery.as_ref().expect("Error Consul Connection");
                        consul.build_cluster(discovery, &mut updater_background_process)
                    },
                }
            }
            None => {
                // Build default hardcoded http cluster
                let cluster = build_cluster_service(
                    &cluster_conf.upstream.unwrap().iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                );
                cluster
            },
        };

        // check if cluster is using cache
        let (cluster_cache_storage, cluster_cache_ttl) = match cluster_conf.cache {
            // if cache config found, check the storage strategy
            Some(cache_type) => {
                // check the storage strategy
                let storage = match cache_type {
                    // if cache uses memory
                    CacheType::Memory { memory } => {
                        let (storage, ttl) = memory.new_storage();
                        (storage, ttl)
                    },
                    // if cache using redis
                    // CacheType::Redis { redis } => (None, None)
                };
                storage
            },
            None => (None, None)
        };

        // Build the cluster metadata and add it to the cluster list
        clusters.push( ClusterMetadata {
            name: cluster_conf.name.unwrap_or("unnamed-cluster".to_string()),
            host: cluster_conf.host.unwrap_or("localhost".to_string()),
            tls: cluster_conf.tls.unwrap_or(false),
            limiter: cluster_conf.rate_limit,
            cache_storage: cluster_cache_storage,
            cache_ttl: cluster_cache_ttl,
            retry: cluster_conf.retry,
            timeout: cluster_conf.timeout,
            auth: cluster_conf.auth,
            consumers: cluster_conf.consumers,
            routes: cluster_conf.routes,
            upstream: cluster_service.task(),
        });
        // Add every cluster process to the list of cluster background processing
        cluster_background_process.push(cluster_service);
        // Add the prefix to the prefix list
        prefix_map.insert(cluster_conf.prefix.unwrap().clone(), idx);
    }

    // Set the list of clusters and prefixes to the main proxy router
    let built = BuiltClusters {
        clusters,
        prefix_map,
        cluster_bg_service: cluster_background_process,
        updater_bg_service: updater_background_process,
    };
    built
}