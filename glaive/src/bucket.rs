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

use pingora::cache::eviction::EvictionManager;
use pingora::cache::lock::CacheLock;
use pingora::cache::predictor::CacheablePredictor;
use pingora::cache::{HttpCache, Storage};
use pingora::proxy::Session;

// the caching bucket is used for configuring all the cache storage
#[derive(Clone, Copy)]
pub struct CacheBucket {
    pub storage: &'static (dyn Storage + Sync),
    pub eviction: Option<&'static (dyn EvictionManager + Sync)>,
    pub predictor: Option<&'static (dyn CacheablePredictor + Sync)>,
    pub cache_lock: Option<&'static CacheLock>,
}

impl CacheBucket {
    // new instances
    pub fn new<T>(storage: T) -> Self
    where
        T: Storage + Sync + 'static,
    {
        CacheBucket {
            storage: Box::leak(Box::new(storage)),
            eviction: None,
            predictor: None,
            cache_lock: None,
        }
    }
    // provide eviction manager
    pub fn with_eviction<T: EvictionManager + Sync + 'static>(mut self, eviction: T) -> Self {
        let b = Box::new(eviction);
        self.eviction = Some(Box::leak(b));
        self
    }
    // without eviction
    pub fn without_eviction(&self) -> Self {
        let mut this = self.clone();
        this.eviction = None;
        this
    }
    // provide predictor
    pub fn with_predictor<T: CacheablePredictor + Sync + 'static>(mut self, predictor: T) -> Self {
        let b = Box::new(predictor);
        self.predictor = Some(Box::leak(b));
        self
    }
    // without predictor
    pub fn without_predictor(&self) -> Self {
        let mut this = self.clone();
        this.predictor = None;
        this
    }
    // provide cache lock
    pub fn with_cache_lock(mut self, cache_lock: CacheLock) -> Self {
        let b = Box::new(cache_lock);
        self.cache_lock = Some(Box::leak(b));
        self
    }
    // without cache lock
    pub fn without_cache_lock(&self) -> Self {
        let mut this = self.clone();
        this.cache_lock = None;
        this
    }
    // private method used to enable cache
    fn enable_cache(&self, cache: &mut HttpCache) {
        cache.enable(self.storage, self.eviction, self.predictor, self.cache_lock)
    }
    // the method that is used to enable cache
    pub fn enable(&self, session: &mut Session) {
        self.enable_cache(&mut session.cache)
    }
}
