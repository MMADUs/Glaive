use tokio::{
    io::{self, AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream, UnixListener, UnixStream},
};

// Generic trait for our streams
trait StreamExt: AsyncRead + AsyncWrite + Unpin + Send {
    fn split(self) -> (io::ReadHalf<Self>, io::WriteHalf<Self>)
    where
        Self: Sized,
    {
        io::split(self)
    }
}

// Blanket implementation for TcpStream and UnixStream
impl StreamExt for TcpStream {}
impl StreamExt for UnixStream {}

#[derive(Debug)]
enum ListenerType {
    TCP,
    UDS,
}

#[derive(Debug)]
pub struct Listener {
    listener_type: ListenerType,
    address: String,
}

impl Listener {
    pub fn new_tcp(addr: &str) -> Self {
        Listener {
            listener_type: ListenerType::TCP,
            address: addr.to_string(),
        }
    }

    pub fn new_uds(addr: &str) -> Self {
        Listener {
            listener_type: ListenerType::UDS,
            address: addr.to_string(),
        }
    }

    pub async fn listen(&self) -> io::Result<()> {
        match self.listener_type {
            ListenerType::TCP => self.tcp_listener().await,
            ListenerType::UDS => self.uds_listener().await,
        }
    }

    async fn tcp_listener(&self) -> io::Result<()> {
        let listener = TcpListener::bind(&self.address).await?;
        println!("TCP Listening on address: {}", &self.address);
        // began infinite loop
        // accepting incoming connections
        loop {
            let new_io = tokio::select! {
                new_io = listener.accept() => new_io,
                // shutdown signal here to break loop
            };
            match new_io {
                Ok((downstream, _addr)) => {
                    println!("new tcp connection accepted");
                    tokio::spawn(async move {
                        // handle here
                        if let Err(e) = self.handle_connection(downstream).await {
                            println!("tcp handler error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    println!("failed to accept tcp connection: {}", e);
                }
            };
        }
    }

    async fn uds_listener(&self) -> io::Result<()> {
        let listener = UnixListener::bind(&self.address)?;
        println!("UDS Listening on socket: {}", &self.address);
        // began infinite loop
        // accepting incoming connections
        loop {
            let new_io = tokio::select! {
                new_io = listener.accept() => new_io,
                // shutdown signal here to break loop
            };
            match new_io {
                Ok((downstream, _addr)) => {
                    println!("new uds connection accepted");
                    tokio::spawn(async move {
                        // handle here
                        if let Err(e) = self.handle_connection(downstream).await {
                            println!("uds handle error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    println!("failed to accept uds connection: {}", e);
                }
            };
        }
    }

    async fn handle_connection<S>(&self, downstream: S) -> io::Result<()>
    where
        S: StreamExt + 'static,
    {
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
