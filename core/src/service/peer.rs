use crate::pool::pool::ConnectionGroupID;
use ahash::AHasher;
use std::hash::{Hash, Hasher};

// upstream peer network type
pub enum PeerNetwork {
    Tcp(String, usize),
    Unix(String),
}

// upstream peer is a metadata for upstream servers
// storing information for server peer
pub struct UpstreamPeer {
    pub name: String,
    pub service: String,
    pub network: PeerNetwork,
    pub connection_timeout: Option<usize>,
}

impl UpstreamPeer {
    // new upstream peer
    pub fn new(
        name: &str,
        service: &str,
        network_type: PeerNetwork,
        timeout: Option<usize>,
    ) -> Self {
        UpstreamPeer {
            name: name.to_string(),
            service: service.to_string(),
            network: network_type,
            connection_timeout: timeout,
        }
    }

    // get the group id from this peer
    // each peer should give unique group id
    pub fn get_group_id(&self) -> ConnectionGroupID {
        let mut hasher = AHasher::default();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

// hash implementation to generate connection peer id
impl Hash for UpstreamPeer {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.name.hash(hasher);
        self.service.hash(hasher);
        // Hash the network enum variants and their contents
        match &self.network {
            PeerNetwork::Tcp(address, port) => {
                // Hash a discriminant value for Tcp variant
                hasher.write_u8(0);
                address.hash(hasher);
                port.hash(hasher);
            }
            PeerNetwork::Unix(path) => {
                // Hash a discriminant value for Unix variant
                hasher.write_u8(1);
                path.hash(hasher);
            }
        }
    }
}
