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

use serde::{Deserialize, Serialize};

// enum cache type
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CacheType {
    Memory { memory: MemoryCache },
    Redis { redis: RedisCache },
}

// memory cache config
#[derive(Debug, Deserialize, Serialize)]
struct MemoryCache {
    cache_ttl: usize,
    max_size: usize,
    max_cache: usize,
    lock_timeout: usize,
}

// redis cache config
#[derive(Debug, Deserialize, Serialize)]
struct RedisCache {}