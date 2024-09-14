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
use std::fs::File;
use std::sync::Arc;
use std::time::Duration;

use pingora::lb::LoadBalancer;
use pingora::prelude::{background_service, RoundRobin, TcpHealthCheck};
use pingora::services::background::GenBackgroundService;

use serde::Deserialize;

// main proxy router
use crate::proxy::ProxyRouter;

// Individual cluster from yaml
#[derive(Debug, Deserialize)]
struct ClusterConfig {
    name: String,
    prefix: String,
    rate_limit: Option<isize>,
    upstreams: Vec<String>,
}

// Config struct from yaml
#[derive(Debug, Deserialize)]
struct Config {
    clusters: Vec<ClusterConfig>,
}

// load config from yaml
fn load_yaml(file_path: &str) -> Config {
    let file = File::open(file_path).expect("Unable to open the file");
    serde_yaml::from_reader(file).expect("Unable to parse YAML")
}

// build the cluster
fn build_cluster_service(
    upstreams: &[&str],
) -> GenBackgroundService<LoadBalancer<RoundRobin>> {
    let mut cluster = LoadBalancer::try_from_iter(upstreams).unwrap();
    cluster.set_health_check(TcpHealthCheck::new());
    cluster.health_check_frequency = Some(Duration::from_secs(1));
    background_service("cluster health check", cluster)
}

// validate clusters configuration
fn validate_cluster_config(config: &ClusterConfig) -> bool {
    if config.name.is_empty() {
        return true;
    }

    if config.prefix.is_empty() || !config.prefix.starts_with('/') || config.prefix.ends_with('/') {
        return true;
    }

    if config.upstreams.is_empty() {
        return true;
    }

    for upstream in &config.upstreams {
        if upstream.is_empty() {
            return true;
        }
    }
    false
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

pub struct ClusterMetadata {
    pub name: String,
    pub rate_limit: Option<isize>,
    pub upstream: Arc<LoadBalancer<RoundRobin>>,
}

pub fn load_config() -> (
    ProxyRouter,
    Vec<GenBackgroundService<LoadBalancer<RoundRobin>>>
){
    // Read config from the yaml
    let config = load_yaml("config.yaml");

    // Validate if there is prefix duplication
    match validate_duplicated_prefix(&config.clusters) {
        true => panic!("found duplicated upstream prefix"),
        false => {}
    }

    // List of Built cluster for the main server
    let mut server_clusters = Vec::new();

    // List of clusters and prefix for the proxy router
    let mut clusters: Vec<ClusterMetadata> = Vec::new();
    let mut prefix_map = HashMap::new();

    // Set up a cluster based on config
    for (idx, cluster_configuration) in config.clusters.iter().enumerate() {
        // Validate cluster config
        match validate_cluster_config(&cluster_configuration) {
            true => panic!("invalid upstream configuration"),
            false => {}
        }

        // Build cluster
        let cluster_service = build_cluster_service(
            &cluster_configuration.upstreams.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        );

        // Add the cluster metadata to the cluster list
        clusters.push( ClusterMetadata {
            name: cluster_configuration.name.clone(),
            rate_limit: cluster_configuration.rate_limit.clone(),
            upstream: cluster_service.task(),
        });
        // push cluster to list
        server_clusters.push(cluster_service);

        // Add the prefix to the prefix list
        prefix_map.insert(cluster_configuration.prefix.clone(), idx);

        println!("Setting up {} upstream", cluster_configuration.name)
    }

    // Set the list of clusters into routes
    let main_router = ProxyRouter{
        clusters,
        prefix_map,
    };

    // return both
    (main_router, server_clusters)
}