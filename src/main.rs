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

mod cluster;
mod path;
mod proxy;
mod limiter;
mod config;
mod auth;
mod discovery;

use std::collections::HashMap;

use pingora::prelude::Opt;
use pingora::proxy::http_proxy_service;
use pingora::server::Server;

use crate::cluster::{build_cluster, build_cluster_service, ClusterMetadata};
use crate::config::load_config;
use crate::proxy::ProxyRouter;

// #[tokio::main]
fn main() {
    // Setup a server
    let opt = Opt::parse_args();
    let mut arc_server = Server::new(Some(opt)).unwrap();

    // load configuration and merge to server configuration
    let cluster_configuration = load_config(&mut arc_server.configuration);
    arc_server.bootstrap();

    // // discovery test
    // let discovery = Discovery::new_consul_discovery();
    // let (cluster1, updater1) = discovery.build_cluster_discovery("catalog-service".to_string());
    // arc_server.add_service(cluster1);
    // arc_server.add_service(updater1);
    // let (cluster2, updater2) = discovery.build_cluster_discovery("catalog-service".to_string());
    // arc_server.add_service(cluster2);
    // arc_server.add_service(updater2);

    // checks the cluster configuration existence and build the cluster
    let proxy_router = match cluster_configuration {
        Some(cluster_config) => {
            // build the entire cluster from the configuration
            let (proxy_router, clusters, updaters) = build_cluster(cluster_config);
            // added every cluster background process to arc server
            for (_idx, cluster_service) in clusters.into_iter().enumerate() {
                arc_server.add_service(cluster_service);
            }
            // added every updater background process to arc server
            for (_idx, updater_process) in updaters.into_iter().enumerate() {
                arc_server.add_service(updater_process);
            }
            proxy_router
        },
        None => {
            // the default prefix if none of cluster exist.
            let mut default_cluster: Vec<ClusterMetadata> = Vec::new();
            let mut default_prefix = HashMap::new();
            let default = build_cluster_service(&["0:0"]);
            let metadata = ClusterMetadata{
                name: "ArcX Gateway".to_string(),
                host: "//default//".to_string(),
                tls: false,
                rate_limit: None,
                retry: None,
                timeout: None,
                upstream: default.task(),
            };
            default_cluster.push(metadata);
            default_prefix.insert("/".to_string(), 0);
            let router = ProxyRouter{
                clusters: default_cluster,
                prefix_map: default_prefix,
            };
            router
        }
    };

    // Build the proxy with the list of clusters
    let mut router_service = http_proxy_service(&arc_server.configuration, proxy_router);
    // Proxy server port
    router_service.add_tcp("0.0.0.0:6188");
    // Set the proxy to the server
    arc_server.add_service(router_service);
    arc_server.run_forever();
}