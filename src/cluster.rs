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

// Raw individual cluster configuration
#[derive(Debug, Deserialize)]
pub struct ClusterConfig {
    name: Option<String>,
    prefix: Option<String>,
    host: Option<String>,
    tls: Option<bool>,
    discovery: Option<bool>,
    rate_limit: Option<isize>,
    retry: Option<usize>,
    timeout: Option<u64>,
    upstream: Vec<String>,
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
    background_service("cluster health check", cluster)
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
pub fn build_cluster(yaml_clusters_configuration: Vec<ClusterConfig>) -> (
    ProxyRouter,
    Vec<GenBackgroundService<LoadBalancer<RoundRobin>>>
){
    // Validate if there is prefix duplication
    match validate_duplicated_prefix(&yaml_clusters_configuration) {
        true => panic!("found duplicated upstream prefix"),
        false => {}
    }

    // List of Built cluster for the main server
    let mut server_clusters = Vec::new();
    // List of clusters and prefix for the proxy router
    let mut clusters: Vec<ClusterMetadata> = Vec::new();
    let mut prefix_map = HashMap::new();

    // Set up a cluster based on config
    for (idx, cluster_conf) in yaml_clusters_configuration.into_iter().enumerate() {
        // Validate cluster config
        match validate_cluster_config(&cluster_conf) {
            true => {},
            false => panic!("invalid upstream configuration")
        }

        // Build cluster
        let cluster_service = build_cluster_service(
            &cluster_conf.upstream.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        );

        // Add the cluster metadata to the cluster list
        clusters.push( ClusterMetadata {
            name: cluster_conf.name.unwrap(),
            host: cluster_conf.host.unwrap(),
            tls: cluster_conf.tls.unwrap(),
            rate_limit: cluster_conf.rate_limit,
            retry: cluster_conf.retry,
            timeout: cluster_conf.timeout,
            upstream: cluster_service.task(),
        });
        // push cluster to list
        server_clusters.push(cluster_service);
        // Add the prefix to the prefix list
        prefix_map.insert(cluster_conf.prefix.unwrap().clone(), idx);
    }

    // Set the list of clusters into routes
    let main_router = ProxyRouter{
        clusters,
        prefix_map,
    };
    // return both
    (main_router, server_clusters)
}