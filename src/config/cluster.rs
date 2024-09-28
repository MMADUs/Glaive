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

use crate::config::{limiter, discovery, auth, route, cache};

// Raw individual cluster configuration
#[derive(Debug, Deserialize)]
pub struct ClusterConfig {
    // this is the main service name & its mandatory
    pub name: Option<String>,
    // the prefix is mandatory. responsible for the uri path for the proxy to handle
    // make sure to prevent prefix duplication and invalid format
    pub prefix: Option<String>,
    // the host is mandatory. the host is responsible for the SNI and Headers
    pub host: Option<String>,
    // the TLS is mandatory. it checks if the proxy should be secured as HTTPS
    pub tls: Option<bool>,
    // discovery allows you to discover services, the default is consul.
    // if the discovery config is provided, the upstream config will be ignored
    pub discovery: Option<discovery::DiscoveryType>,
    // the rate limit responsible for the maximum request to be limited
    pub rate_limit: Option<limiter::RatelimitType>,
    // the used cache type
    pub cache: Option<cache::CacheType>,
    // the retry and timout mechanism is provided for connection failures
    pub retry: Option<usize>,
    pub timeout: Option<u64>,
    // the global auth strategy for the service
    pub auth: Option<auth::AuthType>,
    // the upstream is the hardcoded uri for proxy
    // note: the upstream will be ignored if you provide discovery in the configuration
    pub upstream: Option<Vec<String>>,
    // routes or endpoint configuration
    pub routes: Option<Vec<route::Route>>,
}

impl ClusterConfig {
    pub fn get_name(&self) -> &Option<String> {
        &self.name
    }
    pub fn get_prefix(&self) -> &Option<String> {
        &self.prefix
    }
    pub fn get_host(&self) -> &Option<String> {
        &self.host
    }
    pub fn get_tls(&self) -> &Option<bool> {
        &self.tls
    }
    pub fn get_discovery(&self) -> &Option<discovery::DiscoveryType> {
        &self.discovery
    }
    pub fn get_rate_limit(&self) -> &Option<limiter::RatelimitType> {
        &self.rate_limit
    }
    pub fn get_cache(&self) -> &Option<cache::CacheType> {
        &self.cache
    }
    pub fn get_retry(&self) -> &Option<usize> {
        &self.retry
    }
    pub fn get_timeout(&self) -> &Option<u64> {
        &self.timeout
    }
    pub fn get_auth(&self) -> &Option<auth::AuthType> {
        &self.auth
    }
    pub fn get_upstream(&self) -> &Option<Vec<String>> {
        &self.upstream
    }
    pub fn get_routes(&self) -> &Option<Vec<route::Route>> {
        &self.routes
    }
}