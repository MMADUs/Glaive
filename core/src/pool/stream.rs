use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpStream, UnixStream};
use tokio::sync::Mutex;

use crate::pool::pool::{ConnectionMetadata, ConnectionPool};
use crate::service::peer::{PeerNetwork, UpstreamPeer};
use crate::stream::{stream::Stream, types::StreamType};

// the stream manager is used as the bridge from request lifetime to connection pool
// used to managing socket stream connection
pub struct StreamManager {
    connection_pool: Arc<ConnectionPool<Arc<Mutex<Stream>>>>,
}

const DEFAULT_POOL_SIZE: usize = 128;

// PUBLIC METHODS
// stream manager implementation
impl StreamManager {
    // new stream manager
    pub fn new(pool_size: Option<usize>) -> Self {
        StreamManager {
            connection_pool: Arc::new(ConnectionPool::new(pool_size.unwrap_or(DEFAULT_POOL_SIZE))),
        }
    }

    // used to retrive connection from pool
    // returns the stream and the bool value determine the newly created stream or reused
    pub async fn get_connection_from_pool(&self, peer: &UpstreamPeer) -> Result<(Stream, bool), ()> {
        self.get_stream_connection(peer).await
    }

    // used to return connection after use
    // returns nothing
    pub async fn return_connection_to_pool(&self, connection: Stream, peer: &UpstreamPeer) {
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
            PeerNetwork::Tcp(address) => {
                match TcpStream::connect(address).await {
                    Ok(tcp_stream) => {
                        let stream_type = StreamType::from(tcp_stream);
                        let dyn_stream_type: Stream = Box::new(stream_type);
                        dyn_stream_type
                    }
                    Err(_) => return Err(()),
                }
            }
            PeerNetwork::Unix(socket_path) => match UnixStream::connect(socket_path).await {
                Ok(unix_stream) => {
                    let stream_type = StreamType::from(unix_stream);
                    let dyn_stream_type: Stream = Box::new(stream_type);
                    dyn_stream_type
                }
                Err(_) => return Err(()),
            },
        };
        Ok(stream)
    }

    // used to find a connection from the connection pool
    // returns some stream, there likely a chance stream does not exist
    async fn find_connection_stream(&self, peer: &UpstreamPeer) -> Option<Stream> {
        // get the peer connection group id
        let connection_group_id = peer.get_group_id();
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
                    }
                    Err(_) => None,
                }
            }
            None => None,
        }
    }

    // used to get the stream connection
    // this is the function that is going to be called during request
    // find a connection in pool, if does not exist, create a new socket connection
    // returns the stream and the bool to determine if the connection is new or reused.
    async fn get_stream_connection(&self, peer: &UpstreamPeer) -> Result<(Stream, bool), ()> {
        // find connection from pool
        let reused_connection = self.find_connection_stream(&peer).await;
        match reused_connection {
            Some(stream_connection) => return Ok((stream_connection, true)),
            None => {
                // new socket connection
                let new_stream_connection = self.new_stream_connection(&peer).await;
                match new_stream_connection {
                    Ok(stream_connection) => return Ok((stream_connection, false)),
                    Err(_) => return Err(()),
                }
            }
        }
    }

    // used to return used connection after request finished
    // returns nothing
    async fn return_stream_connection(&self, connection: Stream, peer: &UpstreamPeer) {
        // generate new metadata
        let group_id = peer.get_group_id();
        let unique_id = connection.get_unique_id();
        let metadata = ConnectionMetadata::new(group_id, unique_id);
        // wrapping connection and store it to pool
        let connection_stream = Arc::new(Mutex::new(connection));
        let (closed_connection_notifier, connection_pickup_notification) = self
            .connection_pool
            .add_connection(&metadata, connection_stream);
        let pool = Arc::clone(&self.connection_pool);
        // if the peer provides an idle timeout
        // the returned idle connection will be removed when time exceeded
        if let Some(timeout) = peer.connection_timeout {
            let timeout_duration = Duration::from_secs(timeout as u64);
            tokio::spawn(async move {
                pool.connection_idle_timeout(
                    &metadata,
                    timeout_duration,
                    closed_connection_notifier,
                    connection_pickup_notification,
                )
                .await;
            });
        }
    }
}
