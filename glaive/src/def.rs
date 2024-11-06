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

use std::time::Duration;

use pingora::cache::eviction::lru::Manager as LRUEvictionManager;
use pingora::cache::lock::CacheLock;
use pingora::lb::LoadBalancer;
use pingora::prelude::RoundRobin;
use pingora::services::background::GenBackgroundService;

use serde::{Deserialize, Serialize};

use crate::bucket::CacheBucket;
use crate::cache::MemoryStorage;
use crate::discovery::{Discovery, DiscoveryBackgroundService};

// enum discovery type
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DiscoveryType {
    Consul { consul: ConsulType },
    // DNS { dns: DNSType },
    // Kubernetes { kubernetes: KubernetesType },
}

// hashicorp consul config
#[derive(Debug, Deserialize, Serialize)]
pub struct ConsulType {
    // the service name is mandatory and used for querying to consul
    pub name: String,
    // the passing option determines if the discovery should return healthy/alive services
    // the default is false. set passing to true if you want to get healthy services.
    pub passing: bool,
}

impl ConsulType {
    pub fn build_cluster(
        &self,
        discovery: &Discovery,
        updater_bg_process: &mut Vec<GenBackgroundService<DiscoveryBackgroundService>>,
    ) -> GenBackgroundService<LoadBalancer<RoundRobin>> {
        // Build the cluster with discovery
        let (cluster, updater) =
            discovery.build_cluster_discovery(self.name.clone(), self.passing.clone());
        updater_bg_process.push(updater);
        cluster
    }
}

// dns config
#[derive(Debug, Deserialize, Serialize)]
struct DNSType {}

// kubernetes config
#[derive(Debug, Deserialize, Serialize)]
struct KubernetesType {}

// enum auth type
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum AuthType {
    Key { key: Key },
    JWT { jwt: Jwt },
    External { external: External },
}

// key auth config
#[derive(Debug, Deserialize, Serialize)]
pub struct Key {
    pub allowed: Vec<String>,
}

// jwt auth config
#[derive(Debug, Deserialize, Serialize)]
pub struct Jwt {
    pub secret: String,
}

// external auth config
#[derive(Debug, Deserialize, Serialize)]
struct External {}

// ip whitelist
#[derive(Debug, Deserialize, Serialize)]
pub struct IpWhitelist {
    pub whitelist: Vec<String>,
}

// enum cache type
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CacheType {
    Memory { memory: MemoryCache },
    // Redis { redis: RedisCache },
}

// memory cache config
#[derive(Debug, Deserialize, Serialize)]
pub struct MemoryCache {
    pub cache_ttl: usize,
    pub max_size: usize,
    pub max_cache: usize,
    pub lock_timeout: usize,
}

impl MemoryCache {
    pub fn new_storage(&self) -> (Option<CacheBucket>, Option<usize>) {
        let mega_byte: usize = 1024 * 1024;
        let bucket = CacheBucket::new(
            MemoryStorage::with_capacity(8192)
                .with_reject_empty_body(true)
                .with_max_file_size(Some(mega_byte * self.max_size)),
        )
        .with_eviction(LRUEvictionManager::<16>::with_capacity(
            mega_byte * self.max_cache,
            8192,
        ))
        .with_cache_lock(CacheLock::new(Duration::from_millis(
            self.lock_timeout as u64,
        )));
        // return bucket and ttl
        (Some(bucket), Some(self.cache_ttl))
    }
}

// redis cache config
#[derive(Debug, Deserialize, Serialize)]
struct RedisCache {}

// consumer config
#[derive(Debug, Deserialize, Serialize)]
pub struct Consumer {
    // the allowed consumer name
    pub name: String,
    // the list of allowed access control
    pub acl: Vec<String>,
}

impl Consumer {
    pub fn get_name(&self) -> &String {
        &self.name
    }
    pub fn get_acl(&self) -> &Vec<String> {
        &self.acl
    }
}

// the main limiter configuration
#[derive(Debug, Deserialize, Serialize)]
pub struct Limiter {
    global: Option<RatelimitType>,
    client: Option<RatelimitType>,
}

impl Limiter {
    pub fn get_global(&self) -> &Option<RatelimitType> {
        &self.global
    }
    pub fn get_client(&self) -> &Option<RatelimitType> {
        &self.client
    }
}

// enum rate limit type
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RatelimitType {
    Basic { basic: BasicLimiter },
    // Redis { redis: RedisLimiter },
}

// basic limiter config
#[derive(Debug, Deserialize, Serialize)]
pub struct BasicLimiter {
    pub limit: isize,
}

// redis limiter config
#[derive(Debug, Deserialize, Serialize)]
struct RedisLimiter {}

// headers config
#[derive(Debug, Deserialize, Serialize)]
pub struct Headers {
    // used for storing a list headers data to be inserted
    pub insert: Option<Vec<InsertHeader>>,
    // used for removing a list of headers
    pub remove: Option<Vec<RemoveHeader>>,
}

impl Headers {
    pub fn to_be_inserted(&self) -> &Option<Vec<InsertHeader>> {
        &self.insert
    }
    pub fn to_be_removed(&self) -> &Option<Vec<RemoveHeader>> {
        &self.remove
    }
}

// insert header config
#[derive(Debug, Deserialize, Serialize)]
pub struct InsertHeader {
    // header key
    pub key: String,
    // header value
    pub value: String,
}

// remove header config
#[derive(Debug, Deserialize, Serialize)]
pub struct RemoveHeader {
    // header key
    pub key: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Request {
    pub headers: Option<Headers>,
}

impl Request {
    pub fn get_headers(&self) -> &Option<Headers> {
        &self.headers
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Response {
    pub headers: Option<Headers>,
}

impl Response {
    pub fn get_headers(&self) -> &Option<Headers> {
        &self.headers
    }
}
