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
use serde::{Deserialize, Serialize};

use crate::bucket::CacheBucket;
use crate::cache::MemoryStorage;

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
    pub fn new_storage(&self) -> (
        Option<CacheBucket>,
        Option<usize>,
    ) {
        let mega_byte: usize = 1024 * 1024;
        let bucket = CacheBucket::new(
            MemoryStorage::with_capacity(8192)
                .with_reject_empty_body(true)
                .with_max_file_size(Some(mega_byte * self.max_size)),
        )
            .with_eviction(LRUEvictionManager::<16>::with_capacity(mega_byte * self.max_cache, 8192))
            .with_cache_lock(CacheLock::new(Duration::from_millis(self.lock_timeout as u64)));
        // return bucket and ttl
        (Some(bucket), Some(self.cache_ttl))
    }
}

// redis cache config
#[derive(Debug, Deserialize, Serialize)]
struct RedisCache {}