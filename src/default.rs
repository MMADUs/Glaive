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

use pingora::prelude::{HttpPeer, ProxyHttp, Session, Result as PingoraResult};
use pingora::http::ResponseHeader;

use async_trait::async_trait;
use serde::Serialize;
use bytes::Bytes;

#[derive(Serialize)]
struct Response {
    server: String,
    version: String,
    message: String,
    github: String,
}

// the default proxy is used when the configuration is empty
pub struct DefaultProxy {}

#[async_trait]
impl ProxyHttp for DefaultProxy {
    type CTX = ();
    fn new_ctx(&self) -> () {}

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> PingoraResult<Box<HttpPeer>> { Ok(Box::new(HttpPeer::new("", false, "".to_string()))) }

    async fn request_filter(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX
    ) -> PingoraResult<bool>
    where
        Self::CTX: Send + Sync,
    {
        // build header with code 200 and json response
        let mut header = ResponseHeader::build(200, None)?;
        header.insert_header("Content-Type", "application/json")?;
        let body = Response {
            server: "Glaive Gateway".to_string(),
            version: "2.0.0".to_string(),
            message: "start configuring your gateway!".to_string(),
            github: "https://github.com/MMADUs/Glaive".to_string(),
        };
        let json_body = serde_json::to_string(&body).unwrap();
        let body_bytes = Some(Bytes::from(json_body));
        session.set_keepalive(None);
        session.write_response_header(Box::new(header), true).await?;
        session.write_response_body(body_bytes, true).await?;
        // stop lifetime
        Ok(true)
    }
}