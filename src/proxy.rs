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

use std::collections::HashMap;
use std::time::Duration;

use pingora::prelude::{HttpPeer};
use pingora::proxy::{ProxyHttp, Session};
use pingora::{Error, Result};
use pingora::http::{ResponseHeader};
use pingora::cache::{CacheKey, NoCacheReason, RespCacheable};
use pingora::cache::RespCacheable::Uncacheable;
use pingora::cache::eviction::lru::Manager as LRUEvictionManager;
use pingora::cache::lock::CacheLock;

use serde::Serialize;
use async_trait::async_trait;
use bytes::Bytes;
use once_cell::sync::Lazy;

use crate::bucket::CacheBucket;
use crate::cache::{NoCompression, SccMemoryCache};
use crate::cluster::ClusterMetadata;
use crate::path::select_cluster;
use crate::limiter::rate_limiter;

// Main Struct as Router to implement ProxyHttp
pub struct ProxyRouter {
    pub clusters: Vec<ClusterMetadata>,
    pub prefix_map: HashMap<String, usize>,
}

const MB: usize = 1024 * 1024;

pub static STATIC_CACHE: Lazy<CacheBucket> = Lazy::new(|| {
    CacheBucket::new(
        SccMemoryCache::with_capacity(8192)
            .with_reject_empty_body(true)
            .with_max_file_size(Some(MB * 8))
            .with_compression(NoCompression),
    )
        .with_eviction(LRUEvictionManager::<16>::with_capacity(MB * 128, 8192))
        .with_cache_lock(CacheLock::new(Duration::from_millis(1000)))
});

// struct for proxy context
pub struct RouterCtx {
    pub cluster_address: usize,
    pub proxy_retry: usize,
}

// the struct for default proxy response
#[derive(Serialize)]
struct Default {
    server: String,
    version: String,
    message: String,
    github: String,
}

#[async_trait]
impl ProxyHttp for ProxyRouter {
    // initialize ctx types
    type CTX = RouterCtx;

    // initial ctx values
    fn new_ctx(&self) -> Self::CTX { RouterCtx {
        cluster_address: 0,
        proxy_retry: 0,
    }}

    /**
    * The upstream_peer is the third phase that executes in the lifecycle
    * if this lifecycle returns the http peer
    */
    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        // Select the cluster based on the selected index
        let cluster = &self.clusters[ctx.cluster_address];

        // Set up the upstream
        let upstream = cluster.upstream.select(b"", 256).unwrap(); // Hash doesn't matter for round_robin
        println!("upstream peer is: {:?}", upstream);
        println!("upstream tls: {:?}", cluster.tls);

        // Set SNI to the cluster's host
        let mut peer = Box::new(HttpPeer::new(upstream, cluster.tls, cluster.host.clone()));

        // given the proxy timeout
        let timeout = cluster.timeout.unwrap_or(100);
        peer.options.connection_timeout = Some(Duration::from_millis(timeout));
        println!("send request to upstream");
        Ok(peer)
    }

    /**
    * The request_filter is the first phase that executes in the lifecycle
    * if this lifecycle returns true = the proxy stops | false = continue to upper lifecycle
    */
    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX
    ) -> Result<bool>
    where
        Self::CTX: Send + Sync,
    {
        // Clone the original request header and get the URI path
        let cloned_req_header = session.req_header().clone();
        let original_uri = cloned_req_header.uri.path();
        println!("original uri: {}", original_uri);

        // result to consider if request should continue or stop
        let mut result: bool;

        // select the cluster based on prefix
        result = select_cluster(&self.prefix_map, original_uri, session, ctx).await;

        // Select the cluster based on the selected index
        let cluster = &self.clusters[ctx.cluster_address];

        println!("uri before send to upstream: {}", session.req_header().uri.path());
        println!("The value of my_bool is: {}", result);

        // currently to handle default proxy when cluster does not exist on config
        result = match cluster.host.starts_with("//default//") {
            true => {
                // response with 200 and body
                let mut header = ResponseHeader::build(200, None)?;
                header.insert_header("Content-Type", "application/json")?;
                let body = Default{
                    server: "ArcX API Gateway".to_string(),
                    version: "2.0.0 Release".to_string(),
                    message: "start configuring your gateway!".to_string(),
                    github: "https://github.com/MMADUs/ArcX".to_string(),
                };
                let json_body = serde_json::to_string(&body).unwrap();
                let body_bytes = Some(Bytes::from(json_body));
                session.set_keepalive(None);
                session.write_response_header(Box::new(header), true).await?;
                session.write_response_body(body_bytes, true).await?;
                true
            }
            false => { false }
        };

        // validate if rate limit exist from config
        match cluster.rate_limit {
            Some(limit) => {
                // rate limit incoming request
                result = rate_limiter(limit, session, ctx).await;
            },
            None => {}
        }
        Ok(result)
    }

    /**
    * The proxy_upstream_filter is the second phase that executes in the lifecycle
    * if this lifecycle returns false = the proxy stops | true = continue to upper lifecycle
    */
    async fn proxy_upstream_filter(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX
    ) -> Result<bool>
    where
        Self::CTX: Send + Sync,
    {
        Ok(true)
    }

    // 5. fifth phase executed
    async fn response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        // Remove content-length because the size of the new body is unknown
        upstream_response.remove_header("Content-Length");
        upstream_response.insert_header("Transfer-Encoding", "Chunked")?;
        Ok(())
    }

    // // filter if response should be cached
    // fn request_cache_filter(&self, _session: &mut Session, _ctx: &mut Self::CTX) -> Result<()> {
    //     Ok(())
    // }
    //
    // // generate the cache key, if the filter says the response should be cache
    // fn cache_key_callback(&self, session: &Session, _ctx: &mut Self::CTX) -> Result<CacheKey> {
    //     let req_header = session.req_header();
    //     Ok(CacheKey::default(req_header))
    // }
    //
    // // decide if the response is cacheable
    // fn response_cache_filter(
    //     &self,
    //     _session: &Session,
    //     _resp: &ResponseHeader,
    //     _ctx: &mut Self::CTX,
    // ) -> Result<RespCacheable> {
    //     Ok(Uncacheable(NoCacheReason::Custom("default")))
    // }

    /**
    * The fail_to_connect is the phase that executes after upstream_peer
    * in this lifecycle it checks if request can be retryable or error
    */
    fn fail_to_connect(
        &self,
        _session: &mut Session,
        _peer: &HttpPeer,
        ctx: &mut Self::CTX,
        mut e: Box<Error>,
    ) -> Box<Error> {
        // Select the cluster based on the selected index
        let cluster = &self.clusters[ctx.cluster_address];

        // check if retry reach limits
        if ctx.proxy_retry > cluster.retry.unwrap_or(1) {
            return e;
        }
        // set to be retryable
        ctx.proxy_retry += 1;
        e.set_retry(true);
        e
    }

    // async fn logging(
    //     &self,
    //     session: &mut Session,
    //     _e: Option<&pingora_core::Error>,
    //     ctx: &mut Self::CTX,
    // ) {
    //     let response_code = session
    //         .response_written()
    //         .map_or(0, |resp| resp.status.as_u16());
    //     info!(
    //         "{} response code: {response_code}",
    //         self.request_summary(session, ctx)
    //     );
    //
    //     self.req_metric.inc();
    // }
}