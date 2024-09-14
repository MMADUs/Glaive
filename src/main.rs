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

mod config;
mod cluster;
mod proxy;
mod limiter;

use pingora::prelude::{Opt};
use pingora::proxy::{http_proxy_service, ProxyHttp};
use pingora::server::Server;

use serde::Deserialize;

use crate::config::load_config;

fn main() {
    // Setup a server
    let opt = Opt::parse_args();
    let mut arc_server = Server::new(Some(opt)).unwrap();
    arc_server.bootstrap();

    // load config
    let (proxy_router, server_clusters) = load_config();

    // added every built cluster to arc server
    for (_idx, cluster_service) in server_clusters.into_iter().enumerate() {
        arc_server.add_service(cluster_service);
    }

    // Build the proxy with the list of clusters
    let mut router_service = http_proxy_service(&arc_server.configuration, proxy_router);

    // Proxy server port
    router_service.add_tcp("0.0.0.0:6188");

    // Set the proxy to the server
    arc_server.add_service(router_service);
    arc_server.run_forever();
}