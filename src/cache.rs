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
use std::fmt::Debug;

use pingora::cache::storage::{HandleHit, HandleMiss, HitHandler, MissHandler};
use pingora::cache::key::{CacheHashKey, CacheKey, CompactCacheKey, HashBinary};
use pingora::cache::max_file_size::ERR_RESPONSE_TOO_LARGE;
use pingora::cache::trace::SpanHandle;
use pingora::cache::{CacheMeta, PurgeType, Storage};
use pingora::{Error, Result};

use scc::HashMap;
use serde::{Deserialize, Serialize};
use ahash::RandomState;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use http::header;

#[derive(Clone, Debug)]
pub struct MemoryStorage {}

#[async_trait]
impl Storage for MemoryStorage {
    async fn lookup(
        &'static self,
        key: &CacheKey,
        _trace: &SpanHandle,
    ) -> Result<Option<(CacheMeta, HitHandler)>> {
        println!("lookup occur on Storage impl");
        let meta = CacheMeta::deserialize("tes".as_ref(), "tes".as_ref())?;
        Ok(Some((
            meta,
            Box::new(SccHitHandler{}),
        )))
    }

    async fn get_miss_handler(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        _trace: &SpanHandle,
    ) -> Result<MissHandler> {
        println!("get miss handler occur on Storage impl");
        let miss_handler = SccMissHandler {};
        Ok(Box::new(miss_handler))
    }

    async fn purge(
        &'static self,
        key: &CompactCacheKey,
        _purge_type: PurgeType,
        _trace: &SpanHandle,
    ) -> Result<bool> {
        println!("purge occur on Storage impl");
        let hash = key.combined_bin();
        Ok(true)
    }

    async fn update_meta(
        &'static self,
        key: &CacheKey,
        meta: &CacheMeta,
        _trace: &SpanHandle,
    ) -> Result<bool> {
        println!("update meta occur on Storage impl");
        Ok(true)
    }

    fn support_streaming_partial_write(&self) -> bool {
        println!("support streaming partial write occur on Storage impl");
        false
    }

    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        println!("as any occur on Storage impl");
        self
    }
}

pub struct SccHitHandler {}

#[async_trait]
impl HandleHit for SccHitHandler {
    async fn read_body(&mut self) -> Result<Option<Bytes>> {
        println!("read body occur on handle hit impl");
        Ok(None)
    }

    async fn finish(
        mut self: Box<Self>,
        _storage: &'static (dyn Storage + Sync),
        _key: &CacheKey,
        _trace: &SpanHandle,
    ) -> Result<()> {
        println!("finish occur on handle hit impl");
        Ok(())
    }

    fn can_seek(&self) -> bool {
        println!("can seek occur on handle hit impl");
        true
    }

    fn seek(&mut self, start: usize, end: Option<usize>) -> Result<()> {
        println!("seek occur on handle hit impl");
        Ok(())
    }

    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        println!("as any occur on handle hit impl");
        self
    }
}

#[derive(Debug)]
struct SccMissHandler {}

#[async_trait]
impl HandleMiss for SccMissHandler {
    async fn write_body(&mut self, data: bytes::Bytes, _eof: bool) -> Result<()> {
        println!("write body occur on handle miss impl");
        Ok(())
    }

    async fn finish(self: Box<Self>) -> Result<usize> {
        println!("finish occur on handle miss impl");
        Ok(0)
    }
}