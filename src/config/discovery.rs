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

use pingora::lb::LoadBalancer;
use pingora::prelude::RoundRobin;
use pingora::services::background::GenBackgroundService;

use serde::{Deserialize, Serialize};

use crate::discovery::{Discovery, DiscoveryBackgroundService};

// enum discovery type
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DiscoveryType {
    Consul { consul: ConsulType },
    // DNS { dns: DNSType },
    // Kubernetes { kubernetes: KubernetesType },
}

// hashicorp consul config
#[derive(Debug, Deserialize, Serialize)]
pub struct ConsulType {
    // the service name is mandatory and used for querying to consul
    pub name: String,
    // the passing option determines if the discovery should return healthy/alive services
    // the default is false. set passing to true if you want to get healthy services.
    pub passing: bool,
}

impl ConsulType {
    pub fn build_cluster(
        &self,
        discovery: &Discovery,
        updater_bg_process: &mut Vec<GenBackgroundService<DiscoveryBackgroundService>>
    ) -> GenBackgroundService<LoadBalancer<RoundRobin>> {
        // Build the cluster with discovery
        let (cluster, updater) = discovery.build_cluster_discovery(
            self.name.clone(),
            self.passing.clone(),
        );
        updater_bg_process.push(updater);
        cluster
    }
}

// dns config
#[derive(Debug, Deserialize, Serialize)]
struct DNSType {}

// kubernetes config
#[derive(Debug, Deserialize, Serialize)]
struct KubernetesType {}