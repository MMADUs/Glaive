// Copyright (c) 2024-2025 ArcX, Inc.
//
// This file is part of ArcX Gateway
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

use std::collections::HashMap;
use std::fs::File;

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
    upstreams: Vec<String>,
}

// Config struct from yaml
#[derive(Debug, Deserialize)]
struct Config {
    clusters: Vec<ClusterConfig>,
}

fn load_yaml(file_path: &str) -> Config {
    let file = File::open(file_path).expect("Unable to open the file");
    serde_yaml::from_reader(file).expect("Unable to parse YAML")
}

fn build_cluster_service(
    upstreams: &[&str],
) -> GenBackgroundService<LoadBalancer<RoundRobin>> {
    let mut cluster = LoadBalancer::try_from_iter(upstreams).unwrap();
    cluster.set_health_check(TcpHealthCheck::new());
    cluster.health_check_frequency = Some(std::time::Duration::from_secs(1));
    background_service("cluster health check", cluster)
}

pub fn load_config() -> (
    ProxyRouter,
    Vec<GenBackgroundService<LoadBalancer<RoundRobin>>>
){
    // Read config from the yaml
    let config = load_yaml("config.yaml");

    // List of Built cluster for the main server
    let mut server_clusters = Vec::new();

    // List of clusters and prefix for the proxy router
    let mut clusters = Vec::new();
    let mut prefix_map = HashMap::new();

    // Set up a cluster based on config
    for (idx, cluster_configuration) in config.clusters.iter().enumerate() {
        let cluster_service = build_cluster_service(
            &cluster_configuration.upstreams.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        );

        // Add the cluster to the list
        clusters.push(cluster_service.task());
        server_clusters.push(cluster_service);

        // Add the prefix to the prefix list
        prefix_map.insert(cluster_configuration.prefix.clone(), idx);
        println!("Setting up cluster: {}", idx + 1)
    }

    // Set the list of clusters into routes
    let main_router = ProxyRouter{
        clusters,
        prefix_map,
    };

    // return both
    (main_router, server_clusters)
}