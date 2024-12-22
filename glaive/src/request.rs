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

use pingora::{http::RequestHeader, proxy::Session};

use http::HeaderName;
use once_cell::sync::Lazy;

pub static HTTP_HEADER_X_FORWARDED_FOR: Lazy<HeaderName> =
    Lazy::new(|| HeaderName::from_str("X-Forwarded-For").unwrap());
    
pub static HTTP_HEADER_X_REAL_IP: Lazy<HeaderName> =
    Lazy::new(|| HeaderName::from_str("X-Real-Ip").unwrap());

pub struct RequestProvider {}

impl RequestProvider {
    // new request provide instance
    pub fn new() -> Self {
        RequestProvider {}
    }

    // get client remote address
    fn get_remote_addr(&self, session: &Session) -> Option<(String, u16)> {
        if let Some(addr) = session.client_addr() {
            if let Some(addr) = addr.as_inet() {
                return Some((addr.ip().to_string(), addr.port()));
            }
        }
        None
    }

    /// Gets client ip from X-Forwarded-For,
    /// If none, get from X-Real-Ip,
    /// If none, get remote addr.
    pub fn get_client_ip(&self, session: &Session) -> Option<String> {
        // Check X-Forwarded-For header
        if let Some(value) = session.get_header(HTTP_HEADER_X_FORWARDED_FOR.clone()) {
            let arr: Vec<&str> = value.to_str().unwrap_or_default().split(", ").collect();
            if !arr.is_empty() {
                return Some(arr[0].trim().to_string());
            }
        }
        // Check X-Real-IP header
        if let Some(value) = session.get_header(HTTP_HEADER_X_REAL_IP.clone()) {
            return Some(value.to_str().unwrap_or_default().to_string());
        }
        // Fallback to remote address
        if let Some((addr, _)) = &self.get_remote_addr(session) {
            return Some(addr.to_string());
        }
        // Return None if no IP was found
        None
    }

    /// Gets string value from req header.
    pub fn get_req_header_value<'a>(&'a self, session: &'a Session, key: &str) -> Option<&'a str> {
        let req_header = session.req_header();
        if let Some(value) = req_header.headers.get(key) {
            if let Ok(value) = value.to_str() {
                return Some(value);
            }
        }
        None
    }

    /// Gets cookie value from req header.
    pub fn get_cookie_value<'a>(
        &'a self,
        session: &'a Session,
        cookie_name: &str,
    ) -> Option<&'a str> {
        if let Some(cookie_value) = &self.get_req_header_value(session, "Cookie") {
            for item in cookie_value.split(';') {
                if let Some((k, v)) = item.split_once('=') {
                    if k.trim() == cookie_name {
                        return Some(v.trim());
                    }
                }
            }
        }
        None
    }

    /// Converts query string to map.
    pub fn convert_query_map(&self, query: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        for item in query.split('&') {
            if let Some((key, value)) = item.split_once('=') {
                m.insert(key.to_string(), value.to_string());
            }
        }
        m
    }

    /// Gets query value from req header.
    pub fn get_query_value<'a>(
        &self,
        req_header: &'a RequestHeader,
        name: &str,
    ) -> Option<&'a str> {
        if let Some(query) = req_header.uri.query() {
            for item in query.split('&') {
                if let Some((k, v)) = item.split_once('=') {
                    if k == name {
                        return Some(v.trim());
                    }
                }
            }
        }
        None
    }

    /// Get request host in this order of precedence:
    /// host name from the request line,
    /// or host name from the "Host" request header field
    pub fn get_host<'a>(&'a self, header: &'a RequestHeader) -> Option<&str> {
        if let Some(host) = header.uri.host() {
            return Some(host);
        }
        if let Some(host) = header.headers.get("Host") {
            if let Ok(value) = host.to_str().map(|host| host.split(':').next()) {
                return value;
            }
        }
        None
    }

    /// Get the content length from http request header.
    pub fn get_content_length(&self, header: &RequestHeader) -> Option<usize> {
        if let Some(content_length) = header.headers.get(http::header::CONTENT_LENGTH) {
            if let Ok(size) = content_length.to_str().unwrap_or_default().parse::<usize>() {
                return Some(size);
            }
        }
        None
    }
}
