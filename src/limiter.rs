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
use std::collections::HashMap;

use pingora::http::ResponseHeader;
use pingora::prelude::Session;
use pingora_limits::rate::Rate;

use bytes::Bytes;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use crate::proxy::RouterCtx;
use crate::response::ResponseProvider;

// Global config limiter refresh duration
// Value is 60 seconds by default
static RATE_LIMITER: Lazy<Rate> = Lazy::new(|| Rate::new(Duration::from_secs(60)));

// for rate limit exceed response body
#[derive(Debug, Serialize, Deserialize)]
pub struct TooManyRequest {
    pub status_code: usize,
    pub message: String,
}

pub struct LimiterProvider {
    response_provider: ResponseProvider,
}

impl LimiterProvider {
    pub fn new() -> Self {
        LimiterProvider {
            response_provider: ResponseProvider::new(),
        }
    }

    // global limiter for service/route level
    // this uses service identity as hash
    pub async fn global_limiter(
        &self,
        max_req_limit: isize,
        session: &mut Session,
        ctx: &mut RouterCtx,
    ) -> bool {
        // make sure cluster identity exist
        if let Some(service_id) = &ctx.cluster_identity {
            println!("service_id: {}", service_id);
            // retrieve the current window requests
            let curr_window_requests = RATE_LIMITER.observe(&service_id, 1);
            // if rate limit exceed
            if curr_window_requests > max_req_limit {
                let limit_str = max_req_limit.to_string();
                // rate limited, return 429
                let mut headers: HashMap<&str, &str> = HashMap::new();
                headers.insert("X-Rate-Limit-Limit", limit_str.as_str());
                headers.insert("X-Rate-Limit-Remaining", "0");
                headers.insert("X-Rate-Limit-Reset", "1");
                let _ = &self
                    .response_provider
                    .error_response(session, 429, "Too many request", Some(headers))
                    .await;
                return true;
            }
            // continue request
            return false;
        }
        // todo here, either ignore or throw errors
        false
    }

    // client limiter for service/route level
    // this uses client credential as hash
    // used credential token as hash, but for public routes, uses ip by default
    pub async fn client_limiter(
        &self,
        max_req_limit: isize,
        session: &mut Session,
        ctx: &mut RouterCtx,
    ) -> bool {
        // Get client credential or address
        let client_credential = ctx
            .client_credentials
            .as_ref()
            .or(ctx.client_address.as_ref());
        // check if credential exist
        if let Some(credential) = client_credential {
            // retrieve the current window requests
            let curr_window_requests = RATE_LIMITER.observe(&credential, 1);
            // if rate limit exceed
            if curr_window_requests > max_req_limit {
                let limit_str = max_req_limit.to_string();
                // rate limited, return 429
                let mut headers: HashMap<&str, &str> = HashMap::new();
                headers.insert("X-Rate-Limit-Limit", limit_str.as_str());
                headers.insert("X-Rate-Limit-Remaining", "0");
                headers.insert("X-Rate-Limit-Reset", "1");
                let _ = &self
                    .response_provider
                    .error_response(session, 429, "Too many request", Some(headers))
                    .await;
                return true;
            }
            // continue request
            return false;
        };
        // todo here, either ignore or throw errors
        false
    }
}
