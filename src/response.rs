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

use std::collections::HashMap;
use std::str::FromStr;
use pingora::proxy::Session;
use pingora::http::{ResponseHeader};

use bytes::Bytes;
use http::{HeaderName, HeaderValue, StatusCode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    status_code: usize,
    message: String,
}

pub struct ResponseProvider {}

impl ResponseProvider {
    pub fn new() -> Self {
        ResponseProvider {}
    }

    pub async fn error_response(
        &self,
        session: &mut Session,
        res_status_code: usize,
        res_message: &str,
        headers: Option<HashMap<&str, &str>>
    ) -> Result<(),()> {
        // new response header
        let status_code = StatusCode::from_u16(res_status_code as u16).unwrap();
        let mut res_header = ResponseHeader::build(status_code, None).unwrap();
        // check if headers exist
        if let Some(headers) = headers {
            // insert all headers
            for (key, value) in headers.iter() {
                let header_format = key.replace(' ', "-");
                let header_key = HeaderName::from_str(header_format.as_str()).unwrap();
                let header_value = HeaderValue::from_str(*value).unwrap();
                res_header.insert_header(header_key, header_value).unwrap();
            }
        }
        // response body with json type
        res_header.insert_header("Content-Type", "application/json").unwrap();
        let error_response = Response {
            status_code: res_status_code,
            message: res_message.to_string(),
        };
        // parse response body as json bytes
        let json_body = serde_json::to_string(&error_response).unwrap();
        let body_bytes = Some(Bytes::from(json_body));
        session.write_response_header(Box::new(res_header), false).await.unwrap();
        session.write_response_body(body_bytes, true).await.unwrap();
        session.set_keepalive(None);
        Ok(())
    }
}