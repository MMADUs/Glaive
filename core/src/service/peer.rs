use crate::{listener::socket::SocketAddress, pool::pool::ConnectionGroupID};
use ahash::AHasher;
use std::hash::{Hash, Hasher};

// upstream peer is a metadata for upstream servers
// storing information for server peer
pub struct UpstreamPeer {
    pub name: String,
    pub service: String,
    pub address: SocketAddress,
    pub connection_timeout: Option<usize>,
}

impl UpstreamPeer {
    // new upstream peer
    pub fn new(
        name: &str,
        service: &str,
        socket_address: SocketAddress,
        timeout: Option<usize>,
    ) -> Self {
        UpstreamPeer {
            name: name.to_string(),
            service: service.to_string(),
            address: socket_address,
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
        self.address.hash(hasher);
    }
}
