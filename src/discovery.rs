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
use std::net::{IpAddr, Ipv4Addr, SocketAddr as StdSocketAddr, SocketAddrV4};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use pingora::lb::{Backend, Backends, LoadBalancer};
use pingora::lb::discovery::ServiceDiscovery;
use pingora::prelude::{background_service, RoundRobin, TcpHealthCheck, Result as PingoraResult};
use pingora::services::background::{BackgroundService, GenBackgroundService};
use pingora::protocols::l4::socket::SocketAddr as PingoraSocketAddr;
use pingora::server::ShutdownWatch;

use async_trait::async_trait;
use rs_consul::{Consul, Config, GetServiceNodesRequest, ResponseMeta};

// consul connection test
#[tokio::test]
async fn consul_test() {
    let mut config = Config::default();
    config.address = "http://localhost:8500".to_string();
    config.token = Some("218c8f86-5c49-af4c-f57e-07d2a1b0aee4".to_string());
    println!("consul address: {:?}", &config.address);
    println!("consul token: {:?}", &config.token);
    let client = Consul::new(config);
    let discover_req = GetServiceNodesRequest {
        service: "catalog-service",
        passing: true,
        ..Default::default()
    };
    let ResponseMeta { response, .. } = client.get_service_nodes(discover_req, None).await.unwrap();
    println!("service nodes: {:?}", response);
}

// main discovery
pub struct Discovery {
    consul: Arc<Consul>,
}
// main discovery utilities
impl Discovery {
    // create a new consul client connection
    pub fn new_consul_discovery() -> Discovery {
        let mut config = Config::default();
        config.address = "http://localhost:8500".to_string();
        config.token = Some("218c8f86-5c49-af4c-f57e-07d2a1b0aee4".to_string());
        println!("consul address: {:?}", &config.address);
        println!("consul token: {:?}", &config.token);
        Discovery { consul: Arc::new(Consul::new(config)) }
    }

    // build the cluster with hardcoded upstream
    pub fn build_cluster_discovery(
        &self,
        service_id: String,
    ) -> (
        GenBackgroundService<LoadBalancer<RoundRobin>>,
        GenBackgroundService<DiscoveryBackgroundService>,
    ) {
        // new service discovery
        let consul_discovery = ConsulServiceDiscovery::new(Arc::clone(&self.consul), service_id);
        // use the discovery as the backends, because it returns the backends
        let consul_backends = Backends::new(Box::new(consul_discovery));
        let mut consul_upstream: LoadBalancer<RoundRobin> = LoadBalancer::from_backends(consul_backends);
        // health check the discovered backends
        let hc = TcpHealthCheck::new();
        consul_upstream.set_health_check(hc);
        consul_upstream.health_check_frequency = Some(Duration::from_secs(5));
        // assign health check as background process
        let background = background_service("discovery HC", consul_upstream);
        // clone the background task for backends discovery updates
        let consul_upd_service = DiscoveryBackgroundService {
            lb: background.task().clone(),
        };
        // assign the updater utilities to background process
        let updater = background_service("discovery updater", consul_upd_service);
        (background, updater)
    }
}

// service discovering
pub struct ConsulServiceDiscovery {
    consul: Arc<Consul>,
    service_id: String,
}
// utilities for the main discovering logistics
impl ConsulServiceDiscovery {
    fn new(consul: Arc<Consul>, service_id: String) -> ConsulServiceDiscovery {
        ConsulServiceDiscovery { consul, service_id }
    }

    async fn discover_backends(&self) ->  PingoraResult<Vec<Backend>> {
        // discovery request
        let discover_req = GetServiceNodesRequest {
            service: &*self.service_id,
            passing: true,
            ..Default::default()
        };
        // get services nodes
        let ResponseMeta { response, .. } = self.consul.get_service_nodes(discover_req, None).await.unwrap();
        // extracting raw response to address
        let addresses: Vec<(String, u16)> = response
            .iter()
            .map(|sn| {
                // extracting address and port from discovery result
                (sn.service.address.clone(), sn.service.port.clone())
            })
            .collect();
        // reconstruct raw address and ports to a backend socket address
        let backends = addresses
            .into_iter()
            .map(|(address, port)| {
                println!("backend address: {:?} and port: {:?}", &address, &port);
                // currently a hardcoded localhost as ipv4
                let ip = Ipv4Addr::new(127, 0, 0, 1);
                let socket_v4 = SocketAddrV4::new(ip, port);
                let socket_addr = StdSocketAddr::V4(socket_v4);
                PingoraSocketAddr::Inet(socket_addr)
            })
            .map(|socket_address| Backend {
                addr: socket_address,
                weight: 1,
            })
            .collect();
        Ok(backends)
    }
}

// the built-in proxy discovery that needs to be implemented
#[async_trait]
impl ServiceDiscovery for ConsulServiceDiscovery {
    async fn discover(&self) -> PingoraResult<(BTreeSet<Backend>, HashMap<u64, bool>)> {
        let backends = self.discover_backends().await?;
        let result: BTreeSet<Backend> = backends.into_iter().collect();
        Ok((result, HashMap::new()))
    }
}

// background service
pub struct DiscoveryBackgroundService {
    pub lb: Arc<LoadBalancer<RoundRobin>>,
}
// update backends
impl DiscoveryBackgroundService {
    async fn update_backends(&self) {
        self.lb.update().await.expect("Failed to update backends");
    }
}
// the background service is responsible for updating the backends
#[async_trait]
impl BackgroundService for DiscoveryBackgroundService {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        // interval duration for discovering services
        let update_interval = Duration::from_secs(10);
        // discovering and updating
        loop {
            tokio::select! {
                _ = tokio::time::sleep(update_interval) => {
                    self.update_backends().await;
                }
                _ = shutdown.changed() => {
                    break;
                }
            }
        }
    }
}