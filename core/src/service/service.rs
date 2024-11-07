use crate::listener::listener::{ListenerAddress, NetworkStack, Socket, Stream};
use futures::future;
use std::sync::Arc;
use tokio::io::{self};

// TESTING traits for customization soon
pub trait ServiceType {
    fn say_hi(&self) -> String;
}

// used to build service
// each service can serve on multiple network
// many service is served to the main server
pub struct Service<A> {
    name: String,
    service: A,
    network: NetworkStack,
}

// service implementation mainly for managing service
impl<A> Service<A> {
    // new service
    pub fn new(name: &str, service_type: A) -> Self {
        Service {
            name: name.to_string(),
            service: service_type,
            network: NetworkStack::new(),
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
    pub async fn start_service(address_stack: Vec<ListenerAddress>, service_type: Service<A>) {
        let service_type = Arc::new(service_type);
        let handlers = address_stack.into_iter().map(|network| {
            let service_handler = service_type.clone();
            tokio::spawn(async move {
                let _ = Self::run_service(network, service_handler).await;
            })
        });
        future::join_all(handlers).await;
    }

    // run service is the main service runtime itself
    async fn run_service(
        service_address: ListenerAddress,
        service_type: Arc<Service<A>>,
    ) -> io::Result<()> {
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
                    let service_handler = service_type.clone();
                    tokio::spawn(async move {
                        // handle here
                        if let Err(e) =
                            Self::handle_connection(downstream, socket_address, service_handler)
                                .await
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
        _downstream: Stream,
        _socket_address: Socket,
        service_type: Arc<Service<A>>,
    ) -> io::Result<()> {
        println!("some message!: {}", service_type.service.say_hi());
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

