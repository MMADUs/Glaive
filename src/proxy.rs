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
use std::sync::Arc;

use pingora::lb::LoadBalancer;
use pingora::prelude::{HttpPeer, RoundRobin};
use pingora::proxy::{ProxyHttp, Session};
use pingora::{Result};

use async_trait::async_trait;

// utilities to select cluster
use crate::cluster::select_cluster;

// Main Struct as Router to implement ProxyHttp
pub struct ProxyRouter {
    pub clusters: Vec<Arc<LoadBalancer<RoundRobin>>>,
    pub prefix_map: HashMap<String, usize>,
}

// struct for proxy context
pub struct RouterCtx {
    pub cluster_address: usize,
}

#[async_trait]
impl ProxyHttp for ProxyRouter {
    // initialize ctx types
    type CTX = RouterCtx;

    // initial ctx values
    fn new_ctx(&self) -> Self::CTX {
        RouterCtx { cluster_address: 0 }
    }

    // connect to upstream IO
    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        // Select the cluster based on the selected index
        let cluster = &self.clusters[ctx.cluster_address];

        // Set up the upstream
        let upstream = cluster.select(b"", 256).unwrap(); // Hash doesn't matter for round robin
        println!("upstream peer is: {:?}", upstream);

        // Set SNI to the cluster's host
        let peer = Box::new(HttpPeer::new(upstream, false, "host.docker.internal".to_string()));
        Ok(peer)
    }

    // the very first thing that executes
    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<bool> {
        // Clone the original request header and get the URI path
        let cloned_req_header = session.req_header().clone();
        let original_uri = cloned_req_header.uri.path();
        println!("original uri: {}", original_uri);

        // select the cluster based on prefix
        let result = select_cluster(&self.prefix_map, original_uri, session, ctx).await;

        println!("uri before send to upstream: {}", session.req_header().uri.path());

        Ok(result)
    }
}