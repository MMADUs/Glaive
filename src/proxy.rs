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
use pingora::{Error, Result as PingoraResult};
use pingora::http::{ResponseHeader};
use pingora::cache::{CacheKey, CacheMeta, CachePhase, NoCacheReason, RespCacheable};

use async_trait::async_trait;

use crate::cluster::ClusterMetadata;
use crate::config::limiter::RatelimitType;
use crate::config::consumer::Consumer;
use crate::path::select_cluster;
use crate::limiter::rate_limiter;

// Main Struct as Router to implement ProxyHttp
pub struct ProxyRouter {
    pub clusters: Vec<ClusterMetadata>,
    pub prefix_map: HashMap<String, usize>,
    pub consumers: Option<Vec<Consumer>>,
}

// struct for proxy context
pub struct RouterCtx {
    pub cluster_address: usize,
    pub proxy_retry: usize,
    pub uri_origin: Option<String>,
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
    ) -> PingoraResult<Box<HttpPeer>> {
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
    ) -> PingoraResult<bool>
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

        // check if rate limiter is enabled
        if let Some(limiter_type) = cluster.get_rate_limit() {
            match limiter_type {
                RatelimitType::Basic { basic} => {
                    // rate limit incoming request
                    let limiter_result = rate_limiter(basic.limit, session, ctx).await;
                    match limiter_result {
                        true => return Ok(true),
                        false => (),
                    }
                }
            }
        }

        // check if routes are declared in config
        if let Some(routes) = cluster.get_routes() {
            // get current path
            let cloned_new_header = session.req_header();
            let path = cloned_new_header.uri.path();
            // check if the current uri matches any of the listed routes
            let path_exist = routes
                .iter()
                .any(|route| {
                    // check if routes provide a path
                    if let Some(route_paths) = route.get_paths() {
                        // find the current ui in the list of path
                        route_paths.iter().any(|route_path| {
                            path == route_path
                        })
                    } else {
                        false
                    }
                });
            // validate if path exist
            if path_exist {
                println!("uri path match: {}", path)
            }
        }
        // continue the request
        Ok(false)
    }

    // filter if response should be cached by enabling it
    fn request_cache_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> PingoraResult<()> {
        // select the cluster based on the selected index
        let cluster = &self.clusters[ctx.cluster_address];
        // get request method
        let method = session.req_header().method.clone();
        // filter if request method is GET and Storage exist
        if method == "GET" {
            if let Some(storage) = cluster.get_cache_storage() {
                storage.enable(session)
            }
        }
        Ok(())
    }

    // generate the cache key, if the filter says the response should be cache
    fn cache_key_callback(&self, session: &Session, ctx: &mut Self::CTX) -> PingoraResult<CacheKey> {
        // select the cluster based on the selected index
        let cluster = &self.clusters[ctx.cluster_address];
        // generate key based on the uri method
        // this makes the cache meta unique and prevent cache conflict among other routes
        let key = match ctx.uri_origin.clone() {
            Some(origin) => CacheKey::new(cluster.get_name(), &origin, ""),
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
    ) -> PingoraResult<RespCacheable> {
        // select the cluster to get the ttl
        let cluster = &self.clusters[ctx.cluster_address];
        let ttl = cluster.get_cache_ttl().unwrap_or(2) as u64;
        // get current time and set expiry
        let current_time = SystemTime::now();
        let fresh_until = current_time + Duration::new(ttl, 0);
        let meta = CacheMeta::new(fresh_until, current_time, 0, 0, resp.clone());
        // cache response
        Ok(RespCacheable::Cacheable(meta))
    }

    // the response filter is responsible for modifying response
    async fn response_filter(
        &self,
        session: &mut Session,
        upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> PingoraResult<()>
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

        // cache lock duration
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
        let max_try = cluster.get_retry().unwrap_or(1);
        if ctx.proxy_retry > max_try {
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