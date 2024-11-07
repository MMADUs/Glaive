use tokio::{
    net::{ TcpListener, UnixListener },
    io::{ self },
};

enum ListenerType {
    TCP,
    UDS,
}

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
                Ok((_downstream, _addr)) => {
                    println!("new tcp connection accepted");
                    tokio::spawn(async move {
                        // handle here
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
                Ok((_downstream, _addr)) => {
                    println!("new uds connection accepted");
                    tokio::spawn(async move {
                        // handle here
                    });
                }
                Err(e) => {
                    println!("failed to accept uds connection: {}", e);
                }
            };
        }
    }
}
