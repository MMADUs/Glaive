/**
 * Copyright (c) 2024-2025 ArcX, Inc.
 *
 * This file is part of ArcX Gateway
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

use serde::Deserialize;

use crate::proxy::ProxyRouter;
use crate::discovery::{Discovery, DiscoveryBackgroundService};

// Raw individual cluster configuration
#[derive(Debug, Deserialize)]
pub struct ClusterConfig {
    // this is the main service name & its mandatory
    name: Option<String>,
    // the prefix is mandatory. responsible for the uri path for the proxy to handle
    // make sure to prevent prefix duplication and invalid format
    prefix: Option<String>,
    // the host is mandatory. the host is responsible for the SNI and Headers
    host: Option<String>,
    // the TLS is mandatory. it checks if the proxy should be secured as HTTPS
    tls: Option<bool>,
    // discovery allows you to discover services, the default is consul.
    // if the discovery config is provided, the upstream config will be ignored
    discovery: Option<DiscoveryConfig>,
    // the rate limit responsible for the maximum request to be limited
    rate_limit: Option<isize>,
    // the retry and timout mechanism is provided for connection failures
    retry: Option<usize>,
    timeout: Option<u64>,
    // the upstream is the hardcoded uri for proxy
    // note: the upstream will be ignored if you provide discovery in the configuration
    upstream: Vec<String>,
}

// Raw discovery configuration
#[derive(Debug, Deserialize)]
pub struct DiscoveryConfig {
    // the service name is mandatory and used for querying to consul
    name: Option<String>,
    // the passing option determines if the discovery should return healthy/alive services
    // the default is false. set passing to true if you want to get healthy services.
    passing: Option<bool>,
}

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
    if config.name.is_none() || config.prefix.is_none() || config.host.is_none() || config.tls.is_none() || config.upstream.is_empty() {
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
    pub rate_limit: Option<isize>,
    pub retry: Option<usize>,
    pub timeout: Option<u64>,
    pub upstream: Arc<LoadBalancer<RoundRobin>>,
}

// build the entire cluster from the configuration
pub fn build_cluster(
    yaml_clusters_configuration: Vec<ClusterConfig>
) -> (
    ProxyRouter,
    Vec<GenBackgroundService<LoadBalancer<RoundRobin>>>,
    Vec<GenBackgroundService<DiscoveryBackgroundService>>,
){
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

        // Check if the cluster uses discovery
        let cluster_service = if let Some(discovery_conf) = cluster_conf.discovery {
            // Ensure discovery has been initialized
            let discovery_builder = discovery.as_ref().expect("Discovery is enabled but consul discovery is not created");
            // Build the cluster with discovery
            let (cluster, updater) = discovery_builder.build_cluster_discovery(
                discovery_conf.name.unwrap(),
                discovery_conf.passing.unwrap_or(false),
            );
            updater_background_process.push(updater);
            cluster
        } else {
            // Build default hardcoded http cluster
            let cluster = build_cluster_service(
                &cluster_conf.upstream.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            );
            cluster
        };

        // Build the cluster metadata and add it to the cluster list
        clusters.push( ClusterMetadata {
            name: cluster_conf.name.unwrap(),
            host: cluster_conf.host.unwrap(),
            tls: cluster_conf.tls.unwrap(),
            rate_limit: cluster_conf.rate_limit,
            retry: cluster_conf.retry,
            timeout: cluster_conf.timeout,
            upstream: cluster_service.task(),
        });
        // Add every cluster process to the list of cluster background processing
        cluster_background_process.push(cluster_service);
        // Add the prefix to the prefix list
        prefix_map.insert(cluster_conf.prefix.unwrap().clone(), idx);
    }

    // Set the list of clusters and prefixes to the main proxy router
    let main_router = ProxyRouter{
        clusters,
        prefix_map,
    };
    // return all to be applied to the server instances.
    (main_router, cluster_background_process, updater_background_process)
}