use std::sync::Arc;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use crate::pool::pool::ConnectionPool;
use crate::listener::listener::{Stream};
use tokio::sync::Mutex;
use tokio::net::{TcpStream, UnixStream};
use ahash::AHasher;

use super::pool::ConnectionMetadata;

enum PeerNetwork {
    Tcp(String, usize),
    Unix(String),
}

pub struct UpstreamPeer {
    name: String,
    service: String,
    network: PeerNetwork,
    connection_timeout: Option<usize>,
}

impl UpstreamPeer {
    fn gen_peer_id(&self) -> u64 {
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

// the stream manager is used as the bridge from request lifetime to connection pool
// used to managing socket stream connection
pub struct StreamManager {
    connection_pool: Arc<ConnectionPool<Arc<Mutex<Stream>>>>,
}

const DEFAULT_POOL_SIZE: usize = 128;

// PUBLIC METHODS
// stream manager implementation
impl StreamManager {
    pub fn new(pool_size: Option<usize>) -> Self {
        StreamManager {
            connection_pool: Arc::new(ConnectionPool::new(pool_size.unwrap_or(DEFAULT_POOL_SIZE))),
        }
    }

    pub async fn get_connection_from_pool(&self, peer: UpstreamPeer) -> Result<(Stream, bool), ()> {
        self.get_stream_connection(peer).await
    }

    async fn return_connection_to_pool(&self, connection: Stream, peer: UpstreamPeer) {
        self.return_stream_connection(connection, peer).await
    }
}

// PRIVATE METHODS
// stream manager implementation
impl StreamManager {
    // used to make a new socket connection to upstream peer
    // returns the connection stream
    async fn new_stream_connection(&self, peer: &UpstreamPeer) -> Result<Stream, ()> {
        let stream = match &peer.network {
            PeerNetwork::Tcp(address, port) => {
                let tcp_address = format!("{}:{}", address, port);
                match TcpStream::connect(tcp_address).await {
                    Ok(tcp_stream) => {
                        let stream: Stream = Box::new(tcp_stream);
                        stream
                    },
                    Err(_) => return Err(()),
                }
            },
            PeerNetwork::Unix(socket_path) => {
                match UnixStream::connect(socket_path).await {
                    Ok(unix_stream) => {
                        let stream: Stream = Box::new(unix_stream);
                        stream
                    },
                    Err(_) => return Err(()),
                }
            },
        };
        Ok(stream)
    }

    // used to find a connection from the connection pool
    // returns some stream, there likely a chance stream does not exist
    async fn find_connection_stream(&self, peer: &UpstreamPeer) -> Option<Stream> {
        // get the peer connection group id
        let connection_group_id = peer.gen_peer_id();
        // find connection if exist
        match self.connection_pool.find_connection(connection_group_id) {
            Some(wrapped_stream) => {
                // acquire lock
                {
                    let _ = wrapped_stream.lock().await;
                }
                // unwrapping the arc wrapper
                match Arc::try_unwrap(wrapped_stream) {
                    Ok(stream) => {
                        // unwrap the mutex
                        let connection_stream: Stream = stream.into_inner();
                        Some(connection_stream)
                    },
                    Err(_) => None,
                }
            },
            None => None,
        }
    }

    // used to get the stream connection
    // this is the function that is going to be called during request
    // find a connection in pool, if does not exist, create a new socket connection
    // returns the stream and the bool to determine if the connection is new or reused.
    async fn get_stream_connection(&self, peer: UpstreamPeer) -> Result<(Stream, bool), ()> {
        // find connection from pool
        let reused_connection = self.find_connection_stream(&peer).await;
        match reused_connection {
            Some(stream_connection) => {
                return Ok((stream_connection, true))
            },
            None => {
                // new socket connection
                let new_stream_connection = self.new_stream_connection(&peer).await;
                match new_stream_connection {
                    Ok(stream_connection) => {
                        return Ok((stream_connection, false))
                    },
                    Err(_) => return Err(())
                }
            }
        }
    }

    // used to return used connection after request finished
    // returns nothing
    async fn return_stream_connection(&self, connection: Stream, peer: UpstreamPeer) {
        // generate new metadata
        let new_group_id = peer.gen_peer_id();
        // TODO: replace 101 (unique id) as file descriptor (fd)
        let new_metadata = ConnectionMetadata::new(new_group_id, 101);
        // wrapping connection and store it to pool
        let connection_stream = Arc::new(Mutex::new(connection));
        let (closed_connection_notifier, connection_pickup_notification) = self.connection_pool.add_connection(&new_metadata, connection_stream);
        let pool = Arc::clone(&self.connection_pool);
        // if the peer provides an idle timeout
        // the returned idle connection will be removed when time exceeded
        if let Some(timeout) = peer.connection_timeout {
            let timeout_duration = Duration::from_secs(timeout as u64);
            tokio::spawn(async move {
                pool.connection_idle_timeout(&new_metadata, timeout_duration, closed_connection_notifier, connection_pickup_notification).await;
            });
        }
    }
}
