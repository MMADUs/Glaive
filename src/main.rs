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
mod cache;
mod bucket;

use std::collections::HashMap;

use pingora::prelude::Opt;
use pingora::proxy::http_proxy_service;
use pingora::server::Server;

use crate::cluster::{build_cluster, build_cluster_service, ClusterMetadata};
use crate::config::load_config;
use crate::proxy::ProxyRouter;

fn main() {
    // logging init
    tracing_subscriber::fmt::init();

    // Setup a server
    let opt = Opt::parse_args();
    let mut arc_server = Server::new(Some(opt)).unwrap();
    // load configuration and merge to server configuration
    let cluster_configuration = load_config(&mut arc_server.configuration);
    arc_server.bootstrap(); // preparing

    // checks the cluster configuration existence and build the cluster
    let proxy_router = match cluster_configuration {
        Some(cluster_config) => {
            // build the entire cluster from the configuration
            // the built cluster will return the main proxy router and the necessary background processing
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
            // the default metadata if none of cluster exist.
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

    // Build the proxy with the prepared configuration and the main proxy router
    let mut router_service = http_proxy_service(&arc_server.configuration, proxy_router);
    // Proxy server port to listen
    router_service.add_tcp("0.0.0.0:6188");
    // Set the main proxy as background process and run forever.
    arc_server.add_service(router_service);
    arc_server.run_forever();
}