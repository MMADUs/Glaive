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

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use pingora::prelude::{HttpPeer};
use pingora::proxy::{ProxyHttp, Session};
use pingora::{Error, Result};
use pingora::http::{ResponseHeader};
use pingora::cache::{CacheKey, CacheMeta, CachePhase, NoCacheReason, RespCacheable};
use pingora::cache::eviction::lru::Manager as LRUEvictionManager;
use pingora::cache::lock::CacheLock;

use serde::Serialize;
use async_trait::async_trait;
use bytes::Bytes;
use once_cell::sync::Lazy;

use crate::bucket::CacheBucket;
use crate::cache::MemoryStorage;
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
        MemoryStorage::with_capacity(8192)
            .with_reject_empty_body(true)
            .with_max_file_size(Some(MB * 8))
    )
        .with_eviction(LRUEvictionManager::<16>::with_capacity(MB * 128, 8192))
        .with_cache_lock(CacheLock::new(Duration::from_millis(1000)))
});

// struct for proxy context
pub struct RouterCtx {
    pub cluster_address: usize,
    pub proxy_retry: usize,
    pub uri_origin: Option<String>,
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
        uri_origin: None,
    }}

    // The upstream_peer phase executes after request_filter
    // this lifecycle returns the http peer
    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        // Select the cluster based on the selected index
        let cluster = &self.clusters[ctx.cluster_address];

        // Set up the upstream
        let upstream = cluster.upstream.select(b"", 256).unwrap(); // Hash doesn't matter for round_robin
        // Set SNI to the cluster's host
        let mut peer = Box::new(HttpPeer::new(upstream, cluster.tls, cluster.host.clone()));
        // given the proxy timeout
        let timeout = cluster.timeout.unwrap_or(100);
        peer.options.connection_timeout = Some(Duration::from_millis(timeout));
        Ok(peer)
    }


    // The request_filter is the first phase that executes in the lifecycle
    // if this lifecycle returns true = the proxy stops | false = continue to upper lifecycle
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
        // set uri origin to ctx for cache key later
        ctx.uri_origin = Some(cloned_req_header.uri.to_string());

        // select the cluster based on prefix
        let path_result = select_cluster(&self.prefix_map, original_uri, session, ctx).await;
        match path_result {
            true => return Ok(true),
            false => (),
        }

        // Select the cluster based on the selected index
        let cluster = &self.clusters[ctx.cluster_address];

        // currently to handle default proxy when cluster does not exist on config
        let default_handler_result = match cluster.host.starts_with("//default//") {
            true => {
                // response with 200 and body
                let mut header = ResponseHeader::build(200, None)?;
                header.insert_header("Content-Type", "application/json")?;
                let body = Default{
                    server: "Glaive Gateway".to_string(),
                    version: "2.0.0 Release".to_string(),
                    message: "start configuring your gateway!".to_string(),
                    github: "https://github.com/MMADUs/Glaive".to_string(),
                };
                let json_body = serde_json::to_string(&body).unwrap();
                let body_bytes = Some(Bytes::from(json_body));
                session.set_keepalive(None);
                session.write_response_header(Box::new(header), true).await?;
                session.write_response_body(body_bytes, true).await?;
                true
            }
            false => false,
        };
        match default_handler_result {
            true => return Ok(true),
            false => (),
        }

        // validate if rate limit exist from config
        match cluster.rate_limit {
            Some(limit) => {
                // rate limit incoming request
                let limiter_result = rate_limiter(limit, session, ctx).await;
                match limiter_result {
                    true => return Ok(true),
                    false => (),
                }
            },
            None => (),
        }
        // continue the request
        Ok(false)
    }

    // filter if response should be cached by enabling it
    fn request_cache_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<()> {
        // select the cluster based on the selected index
        let cluster = &self.clusters[ctx.cluster_address];
        // get request method
        let method = session.req_header().method.clone();
        // filter if request method is GET and Storage exist
        if method == "GET" && cluster.cache_storage.is_some() {
            STATIC_CACHE.enable(session);
        }
        Ok(())
    }

    // generate the cache key, if the filter says the response should be cache
    fn cache_key_callback(&self, session: &Session, ctx: &mut Self::CTX) -> Result<CacheKey> {
        // generate key based on the uri method
        // this makes the cache meta unique and prevent cache conflict among other routes
        let key = match ctx.uri_origin.clone() {
            // TODO: make the cache key more unique proper
            // currently just an origin as temp key
            Some(origin) => CacheKey::new(&origin, &origin, ""),
            None => CacheKey::default(session.req_header())
        };
        Ok(key)
    }

    // decide if the response is cacheable
    fn response_cache_filter(
        &self,
        _session: &Session,
        resp: &ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> Result<RespCacheable> {
        // select the cluster to get the ttl
        let cluster = &self.clusters[ctx.cluster_address];
        let ttl = cluster.cache_ttl.unwrap() as u64;
        // get current time and set expiry
        let current_time = SystemTime::now();
        let fresh_until = current_time + Duration::new(ttl, 0);
        let meta = CacheMeta::new(fresh_until, current_time, 0, 0, resp.clone());
        Ok(RespCacheable::Cacheable(meta))
        // response for no cache
        // Ok(RespCacheable::Uncacheable(NoCacheReason::Custom("Your reason")))
    }

    // the response filter is responsible for modifying response
    async fn response_filter(
        &self,
        session: &mut Session,
        upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        // default server identity in headers
        upstream_response.insert_header("Server", "Glaive Gateway")?;

        // insert header for cache status
        if session.cache.enabled() {
            match session.cache.phase() {
                CachePhase::Hit => upstream_response.insert_header("x-cache-status", "hit")?,
                CachePhase::Miss => upstream_response.insert_header("x-cache-status", "miss")?,
                CachePhase::Stale => upstream_response.insert_header("x-cache-status", "stale")?,
                CachePhase::Expired => upstream_response.insert_header("x-cache-status", "expired")?,
                CachePhase::Revalidated | CachePhase::RevalidatedNoCache(_) => upstream_response.insert_header("x-cache-status", "revalidated")?,
                _ => upstream_response.insert_header("x-cache-status", "invalid")?,
            }
        } else {
            match session.cache.phase() {
                CachePhase::Disabled(NoCacheReason::Deferred) => upstream_response.insert_header("x-cache-status", "deferred")?,
                _ => upstream_response.insert_header("x-cache-status", "no-cache")?,
            }
        }
        // cache duration
        if let Some(d) = session.cache.lock_duration() {
            upstream_response.insert_header("x-cache-lock-time-ms", format!("{}", d.as_millis()))?
        }
        Ok(())
    }

    // The fail_to_connect phase executes when upstream_peer is unable to connect
    // in this lifecycle it checks if request can be retryable or error
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