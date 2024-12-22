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

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use redis::{Commands, Connection, RedisError};

pub struct RedisClient {
    client: Connection,
}

impl RedisClient {
    pub fn new(host: &str, port: &str) -> Result<Self, RedisError> {
        let address = format!("redis://{}:{}", host, port);
        let client = redis::Client::open(address)?;
        let client_conn = client.get_connection()?;
        Ok(RedisClient {
            client: client_conn,
        })
    }

    // Set a cache entry with expiration (TTL)
    pub fn set_with_expiration(
        &mut self,
        key: &str,
        value: &str,
        ttl_seconds: u64,
    ) -> Result<(), RedisError> {
        self.client.set_ex(key, value, ttl_seconds)?; // Set key with TTL
        Ok(())
    }

    // Get cache value
    pub fn get_cache(&mut self, key: &str) -> Result<Option<String>, RedisError> {
        let result: Option<String> = self.client.get(key)?;
        Ok(result)
    }

    pub fn fixed_window_rate_limit(
        &mut self,
        key: &str,
        limit: i64,
        period_seconds: i64,
    ) -> Result<bool, RedisError> {
        let current_count: i64 = self.client.incr(key, 1)?;
        if current_count == 1 {
            self.client.expire(key, period_seconds)?;
        }
        Ok(current_count <= limit)
    }

    pub fn sliding_window_log_rate_limit(
        &mut self,
        key: &str,
        limit: usize,
        window_seconds: i64,
    ) -> Result<bool, RedisError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs() as i64;
        let window_start = now - window_seconds;

        redis::pipe()
            .zrevrangebyscore(key, "-inf", window_start)
            .zadd(key, now, now)
            .zcard(key)
            .expire(key, window_seconds)
            .query::<((), (), usize, ())>(&mut self.client)
            .map(|(_, _, count, _)| count <= limit)
    }

    pub fn token_bucket_rate_limit(
        &mut self,
        key: &str,
        capacity: i64,
        refill_rate: f64,
        period_seconds: usize,
    ) -> Result<bool, RedisError> {
        // initialize lua script for query
        let script = redis::Script::new(
            r"
              local key = KEYS[1]
              local capacity = tonumber(ARGV[1])
              local refill_rate = tonumber(ARGV[2])
              local period_seconds = tonumber(ARGV[3])
              local now = tonumber(ARGV[4])
  
              local last_refill = redis.call('GET', key .. ':last_refill') or '0'
              last_refill = tonumber(last_refill)
  
              local tokens = redis.call('GET', key) or tostring(capacity)
              tokens = tonumber(tokens)
  
              local elapsed = now - last_refill
              local refill = math.min(capacity, tokens + (elapsed * refill_rate))
  
              if refill >= 1 then
                  redis.call('SET', key, tostring(refill - 1))
                  redis.call('SET', key .. ':last_refill', tostring(now))
                  redis.call('EXPIRE', key, period_seconds)
                  redis.call('EXPIRE', key .. ':last_refill', period_seconds)
                  return 1
              else
                  return 0
              end
          ",
        );
        // get current time
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
        // execute script
        let result: i32 = script
            .key(key)
            .arg(capacity)
            .arg(refill_rate)
            .arg(period_seconds)
            .arg(now)
            .invoke(&mut self.client)?;

        Ok(result == 1)
    }

    pub fn leaky_bucket_rate_limit(
        &mut self,
        key: &str,
        capacity: i64,
        leak_rate: f64,
    ) -> Result<bool, RedisError> {
        // initialize lua script for query
        let script = redis::Script::new(
            r"
              local key = KEYS[1]
              local capacity = tonumber(ARGV[1])
              local leak_rate = tonumber(ARGV[2])
              local now = tonumber(ARGV[3])
  
              local last_check = redis.call('GET', key .. ':last_check') or tostring(now)
              last_check = tonumber(last_check)
  
              local level = redis.call('GET', key) or '0'
              level = tonumber(level)
  
              local elapsed = now - last_check
              local leaked = math.max(0, level - (elapsed * leak_rate))
  
              if leaked < capacity then
                  redis.call('SET', key, tostring(leaked + 1))
                  redis.call('SET', key .. ':last_check', tostring(now))
                  return 1
              else
                  redis.call('SET', key, tostring(capacity))
                  redis.call('SET', key .. ':last_check', tostring(now))
                  return 0
              end
          ",
        );
        // get current time
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
        // execute script
        let result: i32 = script
            .key(key)
            .arg(capacity)
            .arg(leak_rate)
            .arg(now)
            .invoke(&mut self.client)?;

        Ok(result == 1)
    }

    pub fn semaphore_rate_limit(
        &mut self,
        key: &str,
        max_concurrency: i64,
    ) -> Result<bool, RedisError> {
        let script = redis::Script::new(
            r"
              local key = KEYS[1]
              local max_concurrency = tonumber(ARGV[1])
  
              local current = redis.call('GET', key) or '0'
              current = tonumber(current)
  
              if current < max_concurrency then
                  redis.call('INCR', key)
                  return 1
              else
                  return 0
              end
          ",
        );

        let result: i32 = script
            .key(key)
            .arg(max_concurrency)
            .invoke(&mut self.client)?;

        Ok(result == 1)
    }

    pub fn release_semaphore(&mut self, key: &str) -> Result<(), RedisError> {
        self.client.decr(key, 1)
    }
}
