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

use std::fs::File;
use std::sync::Arc;

use pingora::server::configuration::ServerConf;

use serde::{Deserialize, Serialize};

use crate::def;

// this is the main configuration for the systems
// providing low level flexibility
#[derive(Debug, Deserialize)]
struct SystemConfig {
    /// Version
    pub version: Option<usize>,
    /// Whether to run this process in the background.
    pub daemon: Option<bool>,
    /// When configured and `daemon` setting is `true`, error log will be written to the given
    /// file. Otherwise StdErr will be used.
    pub error_log: Option<String>,
    /// The pid (process ID) file of this server to be created when running in background
    pub pid_file: Option<String>,
    /// the path to the upgrade socket
    ///
    /// In order to perform zero downtime restart, both the new and old process need to agree on the
    /// path to this sock in order to coordinate the upgrade.
    pub upgrade_sock: Option<String>,
    /// If configured, after daemonization, this process will switch to the given user before
    /// starting to serve traffic.
    pub user: Option<String>,
    /// Similar to `user`, the group this process should switch to.
    pub group: Option<String>,
    /// How many threads **each** service should get. The threads are not shared across services.
    pub threads: Option<usize>,
    /// Allow work stealing between threads of the same service. Default `true`.
    pub work_stealing: Option<bool>,
    /// The path to CA file the SSL library should use. If empty, the default trust store location
    /// defined by the SSL library will be used.
    pub ca_file: Option<String>,
    /// Grace period in seconds before starting the final step of the graceful shutdown after signaling shutdown.
    pub grace_period_seconds: Option<u64>,
    /// Timeout in seconds of the final step for the graceful shutdown.
    pub graceful_shutdown_timeout_seconds: Option<u64>,
    // These options don't belong here as they are specific to certain services
    /// IPv4 addresses for a client connector to bind to. See [`ConnectorOptions`].
    /// Note: this is an _unstable_ field that may be renamed or removed in the future.
    pub client_bind_to_ipv4: Option<Vec<String>>,
    /// IPv6 addresses for a client connector to bind to. See [`ConnectorOptions`].
    /// Note: this is an _unstable_ field that may be renamed or removed in the future.
    pub client_bind_to_ipv6: Option<Vec<String>>,
    /// Keepalive pool size for client connections to upstream. See [`ConnectorOptions`].
    /// Note: this is an _unstable_ field that may be renamed or removed in the future.
    pub upstream_keepalive_pool_size: Option<usize>,
    /// Number of dedicated thread pools to use for upstream connection establishment.
    /// See [`ConnectorOptions`].
    /// Note: this is an _unstable_ field that may be renamed or removed in the future.
    pub upstream_connect_offload_threadpools: Option<usize>,
    /// Number of threads per dedicated upstream connection establishment pool.
    /// See [`ConnectorOptions`].
    /// Note: this is an _unstable_ field that may be renamed or removed in the future.
    pub upstream_connect_offload_thread_per_pool: Option<usize>,
    /// When enabled allows TLS keys to be written to a file specified by the SSLKEYLOG
    /// env variable. This can be used by tools like Wireshark to decrypt upstream traffic
    /// for debugging purposes.
    /// Note: this is an _unstable_ field that may be renamed or removed in the future.
    pub upstream_debug_ssl_keylog: Option<bool>,
    /// anything below here will be used as gateway declaration config
    ///
    /// Upper stream Cluster Configurations
    pub clusters: Option<Vec<ClusterConfig>>,
    /// Consumers list, this act something as database for the acl
    pub consumers: Option<Vec<def::Consumer>>,
}

// Individual cluster configuration
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
    pub discovery: Option<def::DiscoveryType>,
    // the rate limit responsible for the maximum request to be limited
    pub rate_limit: Option<def::Limiter>,
    // the used cache type
    pub cache: Option<def::CacheType>,
    // the retry and timout mechanism is provided for connection failures
    pub retry: Option<usize>,
    pub timeout: Option<u64>,
    // request filter & modification
    pub request: Option<def::Request>,
    // response filter & modification
    pub response: Option<def::Response>,
    // enables ip restriction and whitelisted only
    pub ip: Option<def::IpWhitelist>,
    // the global auth strategy for the service
    pub auth: Option<def::AuthType>,
    // the global consumers for the service
    pub consumers: Option<Vec<def::Consumer>>,
    // the upstream is the hardcoded uri for proxy
    // note: the upstream will be ignored if you provide discovery in the configuration
    pub upstream: Option<Vec<String>>,
    // routes or endpoint configuration
    pub routes: Option<Vec<RouteConfig>>,
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
    pub fn get_discovery(&self) -> &Option<def::DiscoveryType> {
        &self.discovery
    }
    pub fn get_rate_limit(&self) -> &Option<def::Limiter> {
        &self.rate_limit
    }
    pub fn get_cache(&self) -> &Option<def::CacheType> {
        &self.cache
    }
    pub fn get_retry(&self) -> &Option<usize> {
        &self.retry
    }
    pub fn get_timeout(&self) -> &Option<u64> {
        &self.timeout
    }
    pub fn get_auth(&self) -> &Option<def::AuthType> {
        &self.auth
    }
    pub fn get_consumers(&self) -> &Option<Vec<def::Consumer>> {
        &self.consumers
    }
    pub fn get_upstream(&self) -> &Option<Vec<String>> {
        &self.upstream
    }
    pub fn get_routes(&self) -> &Option<Vec<RouteConfig>> {
        &self.routes
    }
}

// route endpoint config
#[derive(Debug, Deserialize, Serialize)]
pub struct RouteConfig {
    // the route or endpoint name
    pub name: Option<String>,
    // the list of path for this route
    pub paths: Option<Vec<String>>,
    // the list of allowed methods in this route
    // by default or leaving empty, all method is allowed
    pub methods: Option<Vec<String>>,
    // request filter & modification
    pub request: Option<def::Request>,
    // response filter & modification
    pub response: Option<def::Response>,
    // the specified auth strategy for this route
    pub auth: Option<def::AuthType>,
    // enables ip restriction and whitelisted only
    pub ip: Option<def::IpWhitelist>,
    // the list of allowed consumers for this route
    pub consumers: Option<Vec<def::Consumer>>,
}

impl RouteConfig {
    pub fn get_name(&self) -> &Option<String> {
        &self.name
    }
    pub fn get_paths(&self) -> &Option<Vec<String>> {
        &self.paths
    }
    pub fn get_methods(&self) -> &Option<Vec<String>> {
        &self.methods
    }
    pub fn get_auth(&self) -> &Option<def::AuthType> {
        &self.auth
    }
    pub fn get_consumers(&self) -> &Option<Vec<def::Consumer>> {
        &self.consumers
    }
}

// parse yaml file to configuration based on provided path
fn load_yaml(file_path: &str) -> SystemConfig {
    let file = File::open(file_path).expect("Unable to find configuration file.");
    serde_yaml::from_reader(file).expect("Unable to parse YAML")
}

// our main gateway configuration
#[derive(Debug)]
pub struct GatewayConfig {
    pub clusters: Option<Vec<ClusterConfig>>,
    pub consumers: Option<Vec<def::Consumer>>,
}

// load config from yaml and merge to server configuration
pub fn load_config(server_config: &mut Arc<ServerConf>) -> GatewayConfig {
    // load and parse the yaml file as configuration
    // TODO: the file path should be configurable soon.
    let config = load_yaml("config.yaml");
    let server_config = Arc::get_mut(server_config).unwrap();

    // version
    if let Some(version) = config.version {
        server_config.version = version.clone();
    }
    // daemon
    if let Some(daemon) = config.daemon {
        server_config.daemon = daemon.clone();
    }
    // error log
    if let Some(error_log) = config.error_log {
        server_config.error_log = Some(error_log.clone());
    }
    // pid file
    if let Some(pid_file) = config.pid_file {
        server_config.pid_file = pid_file.clone();
    }
    // upgrade sock
    if let Some(upgrade_sock) = config.upgrade_sock {
        server_config.upgrade_sock = upgrade_sock.clone();
    }
    // user
    if let Some(user) = config.user {
        server_config.user = Some(user.clone());
    }
    // group
    if let Some(group) = config.group {
        server_config.group = Some(group.clone());
    }
    // threads
    if let Some(threads) = config.threads {
        server_config.threads = threads.clone();
    }
    // work stealing
    if let Some(work_stealing) = config.work_stealing {
        server_config.work_stealing = work_stealing.clone();
    }
    // ca file
    if let Some(ca_file) = config.ca_file {
        server_config.ca_file = Some(ca_file.clone());
    }
    // grace period
    if let Some(grace_period_seconds) = config.grace_period_seconds {
        server_config.grace_period_seconds = Some(grace_period_seconds.clone());
    }
    // graceful shutdown
    if let Some(graceful_shutdown_timeout_seconds) = config.graceful_shutdown_timeout_seconds {
        server_config.graceful_shutdown_timeout_seconds =
            Some(graceful_shutdown_timeout_seconds.clone());
    }
    // client bind ipv4
    if let Some(client_bind_to_ipv4) = config.client_bind_to_ipv4 {
        server_config.client_bind_to_ipv4 = client_bind_to_ipv4.clone();
    }
    // client bind ipv6
    if let Some(client_bind_to_ipv6) = config.client_bind_to_ipv6 {
        server_config.client_bind_to_ipv6 = client_bind_to_ipv6.clone();
    }
    // upstream keep alive pool
    if let Some(upstream_keepalive_pool_size) = config.upstream_keepalive_pool_size {
        server_config.upstream_keepalive_pool_size = upstream_keepalive_pool_size.clone();
    }
    // upstream connect threadpools
    if let Some(upstream_connect_offload_threadpools) = config.upstream_connect_offload_threadpools
    {
        server_config.upstream_connect_offload_threadpools =
            Some(upstream_connect_offload_threadpools.clone());
    }
    // upstream connect thread per pool
    if let Some(upstream_connect_offload_thread_per_pool) =
        config.upstream_connect_offload_thread_per_pool
    {
        server_config.upstream_connect_offload_thread_per_pool =
            Some(upstream_connect_offload_thread_per_pool.clone());
    }
    // debug ssl keylog
    if let Some(upstream_debug_ssl_keylog) = config.upstream_debug_ssl_keylog {
        server_config.upstream_debug_ssl_keylog = upstream_debug_ssl_keylog.clone();
    }
    // return our main config
    let gateway_conf = GatewayConfig {
        clusters: config.clusters,
        consumers: config.consumers,
    };
    gateway_conf
}
