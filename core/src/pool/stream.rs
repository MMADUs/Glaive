use std::sync::Arc;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use crate::pool::pool::ConnectionPool;
use crate::listener::listener::{Stream};
use tokio::sync::Mutex;
use tokio::net::{TcpStream, UnixStream};
use ahash::AHasher;

use super::pool::ConnectionMetadata;

#[derive(Clone)]
enum PeerNetwork {
    Tcp(String, usize),
    Unix(String),
}

#[derive(Clone)]
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

pub struct StreamManager {
    connection_pool: Arc<ConnectionPool<Arc<Mutex<Stream>>>>,
}

const DEFAULT_POOL_SIZE: usize = 128;

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

impl StreamManager {
    async fn new_stream_connection(&self, peer: UpstreamPeer) -> Result<Stream, ()> {
        let stream = match peer.network {
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

    async fn find_connection_stream(&self, peer: UpstreamPeer) -> Option<Stream> {
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
                        let tes: Stream = stream.into_inner();
                        Some(tes)
                    },
                    Err(_) => None,
                }
            },
            None => None,
        }
    }

    async fn get_stream_connection(&self, peer: UpstreamPeer) -> Result<(Stream, bool), ()> {
        let reused_connection = self.find_connection_stream(peer.clone()).await;
        match reused_connection {
            Some(stream_connection) => {
                return Ok((stream_connection, true))
            },
            None => {
                let new_stream_connection = self.new_stream_connection(peer.clone()).await;
                match new_stream_connection {
                    Ok(stream_connection) => {
                        return Ok((stream_connection, false))
                    },
                    Err(_) => return Err(())
                }
            }
        }
    }

    async fn return_stream_connection(&self, connection: Stream, peer: UpstreamPeer) {
        let new_metadata = ConnectionMetadata::new(100, 101);
        let connection_stream = Arc::new(Mutex::new(connection));
        let (closed_connection_notifier, connection_pickup_notification) = self.connection_pool.open_connection(&new_metadata, connection_stream);
        let pool = Arc::clone(&self.connection_pool);
        if let Some(timeout) = peer.connection_timeout {
            let timeout_duration = Duration::from_secs(timeout as u64);
            tokio::spawn(async move {
                pool.connection_idle_timeout(&new_metadata, timeout_duration, closed_connection_notifier, connection_pickup_notification).await;
            });
        }
    }
}
