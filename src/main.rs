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

mod cluster;
mod path;
mod proxy;
mod limiter;
mod auth;
mod discovery;
mod cache;
mod bucket;
mod default;
mod config;

use std::env;

use pingora::prelude::Opt;
use pingora::proxy::http_proxy_service;
use pingora::server::Server;

use dotenv::dotenv;
use tracing::info;
use tracing_subscriber::fmt;

use crate::cluster::build_cluster;
use crate::config::config::load_config;
use crate::default::DefaultProxy;

fn main() {
    // logger config builder
    let subscriber = fmt()
        // Configure various options here
        .with_target(false)
        .with_level(true)
        .with_thread_names(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .with_ansi(true)
        // .compact() // .compact is for non-json option
        // .json()
        .finish();
    // init logger
    tracing::subscriber::set_global_default(subscriber).expect("Failed to set subscriber");

    // dotenv init
    dotenv().ok();
    // get port and build the address
    let port = env::var("PORT").unwrap_or_else(|_| "6188".to_string());
    let formatted_address = format!("0.0.0.0:{}", port);
    let address = formatted_address.as_str();

    // Setup a server
    let opt = Opt::parse_args();
    let mut server = Server::new(Some(opt)).unwrap();
    // load configuration and merge to server configuration
    let cluster_configuration = load_config(&mut server.configuration);
    server.bootstrap(); // preparing

    // checks the cluster configuration existence and build the cluster
    match cluster_configuration {
        Some(cluster_config) => {
            // build the entire cluster from the configuration
            // the built cluster will return the main proxy router and the necessary background processing
            let (proxy_router, clusters, updaters) = build_cluster(cluster_config);
            // added every cluster background process to server
            for (_idx, cluster_service) in clusters.into_iter().enumerate() {
                server.add_service(cluster_service);
            }
            // added every updater background process to server
            for (_idx, updater_process) in updaters.into_iter().enumerate() {
                server.add_service(updater_process);
            }
            // build the proxy service and listen
            let mut router = http_proxy_service(&server.configuration, proxy_router);
            router.add_tcp(address);
            server.add_service(router);
        },
        None => {
            // this is the default proxy trait that runs when configuration does not exist
            let mut default = http_proxy_service(&server.configuration, DefaultProxy{});
            default.add_tcp(address);
            server.add_service(default);
        }
    };
    info!("Gateway is listening on {}", address);
    // run the server forever.
    server.run_forever();
}