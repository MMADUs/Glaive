use crate::listener::listener::{ListenerAddress, NetworkStack, Socket};
use crate::pool::stream::StreamManager;
use crate::service::peer::{PeerNetwork, UpstreamPeer};
use crate::stream::stream::Stream;
use futures::future;
use std::sync::Arc;
use tokio::io::{self};

// TESTING traits for customization soon
pub trait ServiceType: Send + Sync + 'static {
    fn say_hi(&self) -> String;
}

// used to build service
// each service can serve on multiple network
// many service is served to the main server
pub struct Service<A> {
    name: String,
    service: A,
    network: NetworkStack,
    stream_session: StreamManager,
}

// service implementation mainly for managing service
impl<A> Service<A> {
    // new service
    pub fn new(name: &str, service_type: A) -> Self {
        Service {
            name: name.to_string(),
            service: service_type,
            network: NetworkStack::new(),
            stream_session: StreamManager::new(None),
        }
    }

    // add new tcp address to service
    pub fn add_tcp_network(&mut self, address: &str) {
        self.network.new_tcp_address(address);
    }

    // add new unix socket path to service
    pub fn add_unix_socket(&mut self, path: &str) {
        self.network.new_unix_path(path);
    }

    // this is probably getting rid soon,
    // just some temporary to get things working
    pub fn get_address_stack(&self) -> Vec<ListenerAddress> {
        self.network.address_stack.clone()
    }
}

// service implementation mainly for running the service
impl<A: ServiceType + Send + Sync + 'static> Service<A> {
    // for starting up service
    pub async fn start_service(self: &Arc<Self>, address_stack: Vec<ListenerAddress>) {
        let handlers = address_stack.into_iter().map(|network| {
            // cloning the arc self is used to keep sharing reference in multithread.
            // same as any method that calls self
            let service = Arc::clone(self);
            tokio::spawn(async move {
                let _ = service.run_service(network).await;
            })
        });
        future::join_all(handlers).await;
    }

    // run service is the main service runtime itself
    async fn run_service(self: &Arc<Self>, service_address: ListenerAddress) -> io::Result<()> {
        let listener = service_address.bind_to_listener().await;
        println!("service is running");
        // began infinite loop
        // accepting incoming connections
        loop {
            let new_io = tokio::select! {
                new_io = listener.accept_stream() => new_io,
                // shutdown signal here to break loop
            };
            match new_io {
                Ok((downstream, socket_address)) => {
                    // get self reference
                    let service = Arc::clone(self);
                    tokio::spawn(async move {
                        // handle here
                        if let Err(e) = service.handle_connection(downstream, socket_address).await
                        {
                            println!("uds handle error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    println!("failed to accept uds connection: {:?}", e);
                }
            };
        }
    }

    // handling incoming request to here
    async fn handle_connection(
        self: &Arc<Self>,
        _downstream: Stream,
        _socket_address: Socket,
    ) -> io::Result<()> {
        println!("some message!: {}", self.service.say_hi());

        // simulate a given backend peer
        let peer = UpstreamPeer::new(
            "node 1",
            &self.name,
            PeerNetwork::Tcp("127.0.0.1:8000".to_string()),
            Some(10),
        );

        // get upstream connection
        let upstream = self.stream_session.get_connection_from_pool(&peer).await;

        // stream validation
        let stream = match upstream {
            Ok((stream, is_reused)) => {
                if is_reused {
                    println!("reusing stream from pool");
                } else {
                    println!("connection does not exist in pool, new stream created");
                }
                Some(stream)
            }
            Err(_) => {
                println!("error getting stream from pool");
                None
            }
        };

        // copy both direction and return the stream to pool
        if let Some(upstream) = stream {
            // do proxy process here

            self.stream_session
                .return_connection_to_pool(upstream, &peer)
                .await;
        };

        // get stream session for request

        // let upstream = TcpStream::connect(&upstream_addr).await?;
        //
        // let (mut downstream_read, mut downstream_write) = downstream.split();
        // let (mut upstream_read, mut upstream_write) = upstream.split();
        //
        // let client_to_upstream = async {
        //     io::copy(&mut downstream_read, &mut upstream_write).await?;
        //     upstream_write.shutdown().await
        // };
        //
        // let upstream_to_client = async {
        //     io::copy(&mut upstream_read, &mut downstream_write).await?;
        //     downstream_write.shutdown().await
        // };
        //
        // tokio::try_join!(client_to_upstream, upstream_to_client)?;
        Ok(())
    }
}
