use std::net::{SocketAddr as StdSocketAddr, SocketAddrV4, SocketAddrV6};
use tokio::net::unix::SocketAddr as UnixSocketAddr;
use tokio::{
    io::{self, AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream, UnixListener, UnixStream},
};

// generic stuff to make the accept implementation working somehow
pub trait DynSocketAddr: Send + Sync {}

#[cfg(unix)]
impl DynSocketAddr for UnixSocketAddr {}
impl DynSocketAddr for SocketAddrV4 {}
impl DynSocketAddr for SocketAddrV6 {}

pub trait DynStream: AsyncRead + AsyncWrite + Unpin + Send + Sync {
    fn split(self) -> (io::ReadHalf<Self>, io::WriteHalf<Self>)
    where
        Self: Sized,
    {
        io::split(self)
    }
}

impl DynStream for TcpStream {}
impl DynStream for UnixStream {}

pub type Stream = Box<dyn DynStream>;
pub type Socket = Box<dyn DynSocketAddr>;

// the main listener type
// the listener is returned right after binding connection stream
// any implementation here is used for any event after connection established
#[derive(Debug)]
pub enum Listener {
    Tcp(TcpListener),
    #[cfg(unix)]
    Unix(UnixListener),
}

impl From<TcpListener> for Listener {
    fn from(s: TcpListener) -> Self {
        Self::Tcp(s)
    }
}

#[cfg(unix)]
impl From<UnixListener> for Listener {
    fn from(s: UnixListener) -> Self {
        Self::Unix(s)
    }
}

impl Listener {
    // used for accepting connection stream
    // since it contains tcp and udp, its better to seperate this
    pub async fn accept_stream(&self) -> Result<(Stream, Socket), ()> {
        match self {
            Self::Tcp(tcp_listener) => match tcp_listener.accept().await {
                Ok((tcp_downstream, socket_addr)) => {
                    let socket_address: Socket = match socket_addr {
                        StdSocketAddr::V4(addr) => Box::new(addr),
                        StdSocketAddr::V6(addr) => Box::new(addr),
                    };
                    Ok((Box::new(tcp_downstream) as Stream, socket_address))
                }
                Err(e) => {
                    println!("unable to accept downstream connection: {}", e);
                    Err(())
                }
            },
            Self::Unix(unix_listener) => match unix_listener.accept().await {
                Ok((unix_downstream, socket_addr)) => {
                    let socket_address: Socket = Box::new(socket_addr);
                    Ok((Box::new(unix_downstream) as Stream, socket_address))
                }
                Err(e) => {
                    print!("unable to accept downstream connection: {}", e);
                    Err(())
                }
            },
        }
    }
}

// listener address is a choice
// wether to use tcp or unix, and will be bind by the implementation
#[derive(Clone)]
pub enum ListenerAddress {
    Tcp(String),
    Unix(String),
}

impl ListenerAddress {
    pub async fn bind_to_listener(&self) -> Listener {
        match self {
            Self::Tcp(address) => TcpListener::bind(address)
                .await
                .map(Listener::from)
                .unwrap_or_else(|e| panic!("{}", e)),
            #[cfg(unix)]
            Self::Unix(path) => UnixListener::bind(path)
                .map(Listener::from)
                .unwrap_or_else(|e| panic!("{}", e)),
        }
    }
}

// the network stack is used for network configurations
// this held many network for 1 service
pub struct NetworkStack {
    pub address_stack: Vec<ListenerAddress>,
}

impl NetworkStack {
    pub fn new() -> Self {
        NetworkStack {
            address_stack: Vec::new(),
        }
    }

    // add tcp address to network list
    pub fn new_tcp_address(&mut self, addr: &str) {
        let tcp_address = ListenerAddress::Tcp(addr.to_string());
        self.address_stack.push(tcp_address);
    }

    // add unix socket path to network list
    pub fn new_unix_path(&mut self, path: &str) {
        let unix_path = ListenerAddress::Unix(path.to_string());
        self.address_stack.push(unix_path);
    }
}
