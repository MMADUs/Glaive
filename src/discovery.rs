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
use std::net::{IpAddr, Ipv4Addr, SocketAddr as StdSocketAddr};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use pingora::lb::{Backend, Backends, LoadBalancer};
use pingora::lb::discovery::ServiceDiscovery;
use pingora::prelude::{background_service, RoundRobin, TcpHealthCheck, Error, Result as PingoraResult};
use pingora::services::background::{BackgroundService, GenBackgroundService};
use pingora::protocols::l4::socket::SocketAddr;
use pingora::server::ShutdownWatch;

use async_trait::async_trait;
use consulrs::client::{ConsulClient, ConsulClientSettingsBuilder};
use consulrs::service::health_by_id;

pub fn new_consul_connection() -> ConsulClient {
    let client = ConsulClient::new(
        ConsulClientSettingsBuilder::default()
            .address("https://127.0.0.1:8500")
            .token("e58c4bc3-8530-32bd-bfcf-f742f01dabbf")
            .build()
            .unwrap()
    ).unwrap();
    client
}

pub struct ConsulServiceDiscovery {
    consul: ConsulClient,
    service_id: String,
}

// i will assume this was used to get all the service uri from consul
impl ConsulServiceDiscovery {
    fn new(consul: ConsulClient, service_id: String) -> ConsulServiceDiscovery {
        ConsulServiceDiscovery { consul, service_id }
    }

    async fn discover_backends(&self) ->  PingoraResult<Vec<Backend>> {
        let available_services = health_by_id(&self.consul, "catalog-service", None).await;

        let backends: Vec<Backend> = match available_services {
            Ok(consul_response) => {
                let services = consul_response.response
                    .into_iter()
                    .map(|(agent_info)| {
                        let service = agent_info.service;
                        let address = service.address.unwrap_or("empty".to_string());
                        let ip = address.parse::<Ipv4Addr>().expect("Invalid IP address");
                        let port = service.port
                            .expect("Port is missing") // This handles the Option
                            .try_into() // This attempts to convert u64 to u16
                            .expect("Port number out of range for u16");
                        let std_socket = StdSocketAddr::new(IpAddr::V4(ip), port);
                        SocketAddr::Inet(std_socket)
                    })
                    .map(|address| Backend {
                        addr: address,
                        weight: 1,
                    })
                    .collect();
                services
            }
            Err(e) => panic!("error discovering services: {}", e.to_string()),
        };
        Ok(backends)
    }
}

// i will assume this executes every time to always discover the backends from the impl above
#[async_trait]
impl ServiceDiscovery for ConsulServiceDiscovery {
    async fn discover(&self) -> PingoraResult<(BTreeSet<Backend>, HashMap<u64, bool>)> {
        let backends = self.discover_backends().await?;
        let result: BTreeSet<Backend> = backends.into_iter().collect();
        Ok((result, HashMap::new()))
    }
}

pub struct ConsulDiscoveryService {
    // pub discovery: Arc<ConsulServiceDiscovery>,
    pub lb: Arc<LoadBalancer<RoundRobin>>,
}

impl ConsulDiscoveryService {
    async fn update_backends(&self) {
        self.lb.update().await.expect("Failed to update backends");
    }
}

#[async_trait]
impl BackgroundService for ConsulDiscoveryService {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        let update_interval = Duration::from_secs(10); // Adjust as needed

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

// build the cluster with hardcoded upstream
pub fn build_cluster_discovery(
    consul: ConsulClient,
    service_id: String,
) -> (
    GenBackgroundService<LoadBalancer<RoundRobin>>,
    GenBackgroundService<ConsulDiscoveryService>,
) {
    let consul_discovery = ConsulServiceDiscovery::new(consul, service_id);

    let consul_backends = Backends::new(Box::new(consul_discovery));
    let mut consul_upstream: LoadBalancer<RoundRobin> = LoadBalancer::from_backends(consul_backends);

    let hc = TcpHealthCheck::new();
    consul_upstream.set_health_check(hc);
    consul_upstream.health_check_frequency = Some(Duration::from_secs(5));

    let background = background_service("discovery HC", consul_upstream);

    let consul_upd_service = ConsulDiscoveryService {
        lb: background.task().clone(),
    };

    let updater = background_service("discovery updater", consul_upd_service);
    (background, updater)
}