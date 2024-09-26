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

use std::any::Any;
use std::sync::Arc;
use std::time::SystemTime;
use pingora::cache::storage::{HandleHit, HandleMiss, HitHandler, MissHandler};
use pingora::cache::key::{CacheHashKey, CacheKey, CompactCacheKey, HashBinary};
use pingora::cache::trace::{Span, SpanHandle};
use pingora::cache::{CacheMeta, PurgeType, Storage};
use pingora::{Error, Result};

use scc::HashMap;
use serde::{Deserialize, Serialize};
use ahash::RandomState;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};

// used in cache object to stores the cache meta in bytes
type BinaryMeta = (Bytes, Bytes);

// the main hashmap where all the cache was stored
pub type SharedHashMap = Arc<HashMap<HashBinary, SccCacheObject, RandomState>>;

// main cache struct that impl the Storage
#[derive(Clone, Debug)]
pub struct MemoryStorage {
    pub cache: SharedHashMap,
    /// Maximum allowed body size for caching
    pub max_file_size_bytes: Option<usize>,
    /// Will reject cache admissions with empty body responses
    pub reject_empty_body: bool,
}

impl MemoryStorage {
    // new cache instance
    pub fn new() -> Self {
        MemoryStorage {
            cache: Arc::new(HashMap::with_hasher(RandomState::new())),
            max_file_size_bytes: None,
            reject_empty_body: false,
        }
    }
    // new cache instance with cache capacity
    pub fn with_capacity(capacity: usize) -> Self {
        MemoryStorage {
            cache: Arc::new(HashMap::with_capacity_and_hasher(
                capacity,
                RandomState::new(),
            )),
            max_file_size_bytes: None,
            reject_empty_body: false,
        }
    }
    // max file size config when cache occurs
    pub fn with_max_file_size(mut self, max_bytes: Option<usize>) -> Self {
        self.max_file_size_bytes = max_bytes;
        self
    }
    // reject empty cache config when cache occurs
    pub fn with_reject_empty_body(mut self, should_error: bool) -> Self {
        self.reject_empty_body = should_error;
        self
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    // cache lookup is responsible for finding the cache data
    async fn lookup(
        &'static self,
        key: &CacheKey,
        _trace: &SpanHandle,
    ) -> Result<Option<(CacheMeta, HitHandler)>> {
        // gen hash from key
        let hash = key.combined_bin();
        // find cache in hashmap with hash key
        let cache_object = match self.cache.get_async(&hash).await {
            Some(obj) => obj.get().clone(),
            None => return Ok(None),
        };
        // deserialize cache from binary
        let meta = CacheMeta::deserialize(&cache_object.meta.0, &cache_object.meta.1)?;
        // checks if the cache is not expired yet
        // if cache expires, purge or remove the cache immediately from hashmap
        if meta.fresh_until() < SystemTime::now() {
            self.purge(&key.to_compact(), PurgeType::Invalidation, &Span::inactive().handle()).await.expect("PURGE ERROR");
        }
        // return the meta and the hit handler
        Ok(Some((
            meta,
            Box::new(CacheHitHandler::new(cache_object, self.clone())),
        )))
    }

    // get_miss_handler is executed when cache did not hit
    // return the miss handler to cache the new response
    async fn get_miss_handler(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        _trace: &SpanHandle,
    ) -> Result<MissHandler> {
        // gen hash from key
        let hash = key.combined_bin();
        // serialize meta from response and sends to miss handler
        let raw_meta = meta.serialize()?;
        let meta = (Bytes::from(raw_meta.0), Bytes::from(raw_meta.1));
        // new miss handler
        let miss_handler = CacheMissHandler {
            body_buf: BytesMut::new(),
            meta,
            key: hash,
            inner: self.clone(),
        };
        Ok(Box::new(miss_handler))
    }

    // purge is used to remove the cache from hashmap
    async fn purge(
        &'static self,
        key: &CompactCacheKey,
        _purge_type: PurgeType,
        _trace: &SpanHandle,
    ) -> Result<bool> {
        // gen hash from key
        let hash = key.combined_bin();
        // remove
        Ok(self.cache.remove(&hash).is_some())
    }

    // update_meta is used to update the current meta in hashmap
    async fn update_meta(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        _trace: &SpanHandle,
    ) -> Result<bool> {
        // gen hash from key
        let hash = key.combined_bin();
        // serialize new meta from response
        let new_meta = meta.serialize()?;
        let new_meta = (Bytes::from(new_meta.0), Bytes::from(new_meta.1));
        // perform updates
        let updated = self
            .cache
            .update_async(&hash, move |_, value| {
                value.meta = new_meta;
            })
            .await;
        // checks if update succeed or fails
        if let Some(()) = updated {
            Ok(true)
        } else {
            Err(Error::create(
                pingora::ErrorType::Custom("No meta found for update_meta"),
                pingora::ErrorSource::Internal,
                Some(format!("key = {:?}", key).into()),
                None,
            ))
        }
    }

    // currently not using partial write
    fn support_streaming_partial_write(&self) -> bool {
        false
    }
    // ignored
    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        self
    }
}

// the struct used for the cache object
// the cache object is the actual data that is stored in the hashmap
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
pub struct SccCacheObject {
    meta: BinaryMeta,
    body: Bytes,
}

// hit handler when cache hit
pub struct CacheHitHandler {
    cache_object: SccCacheObject,
    inner: MemoryStorage,
    done: bool,
    range_start: usize,
    range_end: usize,
}

impl CacheHitHandler {
    pub(crate) fn new(cache_object: SccCacheObject, inner: MemoryStorage) -> Self {
        let len = cache_object.body.len();
        CacheHitHandler {
            cache_object,
            inner,
            done: false,
            range_start: 0,
            range_end: len,
        }
    }
}

#[async_trait]
impl HandleHit for CacheHitHandler {
    // read the body from the cache
    async fn read_body(&mut self) -> Result<Option<Bytes>> {
        if self.done {
            Ok(None)
        } else {
            self.done = true;
            Ok(Some(
                self.cache_object
                    .body
                    .slice(self.range_start..self.range_end),
            ))
        }
    }

    // when cache found, continue to the downstream
    async fn finish(
        mut self: Box<Self>,
        _storage: &'static (dyn Storage + Sync),
        _key: &CacheKey,
        _trace: &SpanHandle,
    ) -> Result<()> {
        Ok(())
    }

    // ignored
    fn can_seek(&self) -> bool {
        true
    }

    // seek is responsible for validating bytes length to be processed
    fn seek(&mut self, start: usize, end: Option<usize>) -> Result<()> {
        let len = self.cache_object.body.len();
        if start >= len {
            return Error::e_explain(
                pingora::ErrorType::InternalError,
                format!("seek start out of range {start} >= {len}"),
            );
        }
        self.range_start = start;
        if let Some(end) = end {
            self.range_end = std::cmp::min(len, end);
        }
        // mark the process to not stop
        self.done = false;
        Ok(())
    }

    // ignored
    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        self
    }
}

// miss handler when cache did not hit
#[derive(Debug)]
struct CacheMissHandler {
    meta: BinaryMeta,
    key: HashBinary,
    body_buf: BytesMut,
    inner: MemoryStorage,
}

#[async_trait]
impl HandleMiss for CacheMissHandler {
    // the write body will validate if the response is cacheable by size
    // decide if the response should be written to cache
    async fn write_body(&mut self, data: Bytes, _eof: bool) -> Result<()> {
        // validate response size
        if let Some(max_file_size_bytes) = self.inner.max_file_size_bytes {
            if self.body_buf.len() + data.len() > max_file_size_bytes {
                return Error::e_explain(
                    pingora::cache::max_file_size::ERR_RESPONSE_TOO_LARGE,
                    format!(
                        "writing data of size {} bytes would exceed max file size of {} bytes",
                        data.len(),
                        max_file_size_bytes
                    ),
                );
            }
        }
        // continue with the response data
        self.body_buf.extend_from_slice(&data);
        Ok(())
    }

    // this is responsible for validating empty response data
    // the logic that cache the response to the hashmap
    async fn finish(self: Box<Self>) -> Result<usize> {
        let body_len = self.body_buf.len();
        // validate if response data is not empty
        if body_len == 0 && self.inner.reject_empty_body {
            let err = Error::create(
                pingora::ErrorType::Custom("cache write error: empty body"),
                pingora::ErrorSource::Internal,
                None,
                None,
            );
            return Err(err);
        }
        // get response data and build as the cache object
        let body = self.body_buf.freeze();
        let size = body.len() + self.meta.0.len() + self.meta.1.len();
        let cache_object = SccCacheObject {
            body,
            meta: self.meta,
        };
        // write data to hashmap
        self.inner
            .cache
            .insert_async(self.key, cache_object)
            .await
            .ok();
        // returns the data size
        Ok(size)
    }
}