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

use std::time::Duration;

use pingora::prelude::Session;
use pingora::http::ResponseHeader;
use pingora_limits::rate::Rate;

use once_cell::sync::Lazy;

// ctx router
use crate::proxy::RouterCtx;

// Rate limiter
static RATE_LIMITER: Lazy<Rate> = Lazy::new(|| Rate::new(Duration::from_secs(10)));

pub async fn rate_limiter(
    max_req_limit: isize,
    session: &mut Session,
    _ctx: &mut RouterCtx,
) -> bool {
    // find the appid headers
    let appid = match session
        .req_header()
        .headers
        .get("appid")
        .and_then(|header_value| header_value.to_str().ok())
    {
        Some(value) => value.to_string(),
        None => {
            let header = ResponseHeader::build(400, None).unwrap();
            session.write_response_header(Box::new(header), true).await.unwrap();
            return true
        }
    };

    // retrieve the current window requests
    let curr_window_requests = RATE_LIMITER.observe(&appid, 1);

    // if rate limit exceed
    if curr_window_requests > max_req_limit {
        // rate limited, return 429
        let mut header = ResponseHeader::build(429, None).unwrap();
        header.insert_header("X-Rate-Limit-Limit", max_req_limit.to_string()).unwrap();
        header.insert_header("X-Rate-Limit-Remaining", "0").unwrap();
        header.insert_header("X-Rate-Limit-Reset", "1").unwrap();
        session.set_keepalive(None);
        session.write_response_header(Box::new(header), true).await.unwrap();
        return true;
    }
    false
}