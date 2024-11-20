use futures::future;
use http::method;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

use crate::listener::listener::{ListenerAddress, NetworkStack, Socket};
use crate::pool::stream::StreamManager;
use crate::service::buffer::BufferSession;
use crate::service::peer::{PeerNetwork, UpstreamPeer};
use crate::stream::stream::Stream;

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
                service.run_service(network).await;
            })
        });
        future::join_all(handlers).await;
    }

    // run service is the main service runtime itself
    async fn run_service(self: &Arc<Self>, service_address: ListenerAddress) {
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
                Ok((mut downstream, socket_address)) => {
                    // get self reference
                    let service = Arc::clone(self);
                    tokio::spawn(async move {
                        // handle here
                        // service.handle_connection(downstream, socket_address).await
                        service.test_handle(downstream).await
                    });
                }
                Err(e) => {
                    println!("failed to accept uds connection: {:?}", e);
                }
            };
        }
    }

    async fn test_handle(&self, socket: Stream) -> tokio::io::Result<()> {
        let mut session = BufferSession::new(socket);
        let _ = session.read_stream().await;

        println!("BEFORE");
        println!("debug raw bytes: {:?}", session.buffer);
        println!("");

        let lossy_string = String::from_utf8_lossy(&session.buffer);
        let cleaned_string = lossy_string.replace("\r\n", " ");

        println!("debug bytes as str: {:?}", cleaned_string);
        // request version
        let version = session.get_version();
        println!("version: {}", version.unwrap_or(""));
        // request method
        let method = session.get_method();
        println!("method: {}", method.unwrap_or(""));
        // request path
        let path = session.get_path();
        println!("path: {}", path.unwrap_or(""));
        // get header
        let appid = session.get_header("appid");
        println!("appid header: {}", appid.unwrap_or(""));
        // remove header
        {
            let _ = session.remove_header("tes".as_bytes());
            let deleted_header = session.get_header("tes");
            println!("deleted header: {}", deleted_header.unwrap_or(""));
        }
        // insert header
        {
            session.append_header("sepuh".as_bytes(), "jeremy".as_bytes());
            session.append_header("sepuh".as_bytes(), "jeremy".as_bytes());    
            let sepuh = session.get_header("sepuh");
            println!("sepuh header: {}", sepuh.unwrap_or(""));
        }
        // // get query param
        // let page = session.get_query_param("page");
        // println!("query param page: {}", page.unwrap_or(""));
        // let limit = session.get_query_param("limit");
        // println!("query param limit: {}", limit.unwrap_or(""));
        // // insert query param
        // {
        //     session.insert_query_param("test".as_bytes(), "test-val".as_bytes());
        //     let test = session.get_query_param("test");
        //     println!("insert query test: {}", test.unwrap_or(""));
        // }
        // // remove query param
        // {
        //     session.remove_query_param("tes".as_bytes());
        //     let deleted_query = session.get_query_param("tes");
        //     println!("deleted query: {}", deleted_query.unwrap_or(""));
        // }

        println!("AFTER");
        println!("debug raw bytes: {:?}", session.buffer);
        println!("");

        let lossy_string = String::from_utf8_lossy(&session.buffer);
        let cleaned_string = lossy_string.replace("\r\n", " ");

        println!("debug bytes as str: {:?}", cleaned_string);

        // let mut buffer = vec![0; 1024];
        //
        // // Read the request
        // let n = socket.read(&mut buffer).await?;
        // println!("Received {} bytes", n);
        //
        // // Print raw request for debugging
        // println!("Raw request:\n{}", String::from_utf8_lossy(&buffer[..n]));
        //
        // // Create JSON response
        // let json_response = r#"{
        // "message": "Hello from Rust Tokio server!",
        // "status": "success"
        // }"#;
        //
        // // Create HTTP response
        // let response = format!(
        //     "HTTP/1.1 200 OK\r\n\
        //  Content-Type: application/json\r\n\
        //  Content-Length: {}\r\n\
        //  Access-Control-Allow-Origin: *\r\n\
        //  Connection: close\r\n\
        //  \r\n\
        //  {}",
        //     json_response.len(),
        //     json_response
        // );
        //
        // // Write response back to socket
        // socket.write_all(response.as_bytes()).await?;
        // socket.flush().await?;

        println!("Sent response successfully\n");

        Ok(())
    }

    // handling incoming request to here
    async fn handle_connection(self: &Arc<Self>, downstream: Stream, _socket_address: Socket) {
        println!("some message!: {}", self.service.say_hi());

        // simulate a given backend peer
        let peer = UpstreamPeer::new(
            "node 1",
            &self.name,
            PeerNetwork::Tcp("127.0.0.1:8000".to_string()),
            Some(10),
        );

        // get upstream connection
        match self.stream_session.get_connection_from_pool(&peer).await {
            Ok((upstream, is_reused)) => {
                if is_reused {
                    println!("reusing stream from pool");
                } else {
                    println!("connection does not exist in pool, new stream created");
                }

                // let mut client_session = SessionBuffer::new(downstream);
                // let mut server_session = SessionBuffer::new(upstream);
                //
                // match self
                //     .copy_bidirectional(&mut client_session, &mut server_session)
                //     .await
                // {
                //     Ok(_) => println!("copy bidirectional succeed"),
                //     Err(_) => println!("copy bidirectional failed"),
                // }
            }
            Err(_) => println!("error getting stream from pool"),
        }

        // // stream validation
        // let stream = match upstream {
        //     Ok((stream, is_reused)) => {
        //         if is_reused {
        //             println!("reusing stream from pool");
        //         } else {
        //             println!("connection does not exist in pool, new stream created");
        //         }
        //         Some(stream)
        //     }
        //     Err(_) => {
        //         println!("error getting stream from pool");
        //         None
        //     }
        // };
        //
        // if let Some(upstream) = stream {
        //     self.handle_request(downstream, upstream).await;
        //
        //     // self.stream_session
        //     //     .return_connection_to_pool(upstream, &peer)
        //     //     .await;
        // };
    }
}
