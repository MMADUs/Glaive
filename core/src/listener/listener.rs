use std::net::{SocketAddr as StdSocketAddr, SocketAddrV4, SocketAddrV6, ToSocketAddrs};
use tokio::net::unix::SocketAddr as UnixSocketAddr;
use tokio::net::TcpSocket;
use tokio::net::{TcpListener, UnixListener};

use crate::stream::{stream::Stream, types::StreamType};

// generic stuff to make the accept implementation working somehow
pub trait DynSocketAddr: Send + Sync {}

#[cfg(unix)]
impl DynSocketAddr for UnixSocketAddr {}
impl DynSocketAddr for SocketAddrV4 {}
impl DynSocketAddr for SocketAddrV6 {}

pub type Socket = Box<dyn DynSocketAddr>;

// the main listener type
// the listener is returned right after binding connection stream
// any implementation here is used for any event after connection established
#[derive(Debug)]
pub enum Listener {
    Tcp(TcpListener),
    Unix(UnixListener),
}

impl From<TcpListener> for Listener {
    fn from(s: TcpListener) -> Self {
        Self::Tcp(s)
    }
}

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
                    // parsing tcp stream to dynamic stream concrete type
                    let stream_type = StreamType::from(tcp_downstream);
                    let dyn_stream_type: Stream = Box::new(stream_type);
                    Ok((dyn_stream_type, socket_address))
                }
                Err(e) => {
                    println!("unable to accept downstream connection: {}", e);
                    Err(())
                }
            },
            Self::Unix(unix_listener) => match unix_listener.accept().await {
                Ok((unix_downstream, socket_addr)) => {
                    let socket_address: Socket = Box::new(socket_addr);
                    // parsing unix stream to dynamic stream concrete type
                    let stream_type = StreamType::from(unix_downstream);
                    let dyn_stream_type: Stream = Box::new(stream_type);
                    Ok((dyn_stream_type, socket_address))
                }
                Err(e) => {
                    print!("unable to accept downstream connection: {}", e);
                    Err(())
                }
            },
        }
    }
}

const LISTENER_BACKLOG: u32 = 65535;

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
            Self::Tcp(address) => {
                // create socket address from string
                let socket_address = match address.to_socket_addrs() {
                    Ok(mut socket_addr) => match socket_addr.next() {
                        Some(address) => address,
                        None => panic!("could not resolve address"),
                    },
                    Err(e) => panic!("{}", e),
                };
                // new tcp socket based on the parsed socket address
                let tcp_socket = match socket_address {
                    StdSocketAddr::V4(_) => TcpSocket::new_v4(),
                    StdSocketAddr::V6(_) => TcpSocket::new_v6(),
                };
                // unwrap result
                let tcp_socket = match tcp_socket {
                    Ok(socket) => socket,
                    Err(e) => panic!("{}", e),
                };
                // set tcp socket reuse address to true
                if let Err(e) = tcp_socket.set_reuseaddr(true) {
                    panic!("{}", e);
                }
                // TODO: optimize tcp from low syscall here before binding the socket
                // bind tcp socket to the socket address
                if let Err(e) = tcp_socket.bind(socket_address) {
                    panic!("{}", e);
                }
                // listen to tcp socket
                tcp_socket
                    .listen(LISTENER_BACKLOG)
                    .map(Listener::from)
                    .unwrap_or_else(|e| panic!("{}", e))
            }
            Self::Unix(path) => {
                // remove existing socket path
                match std::fs::remove_file(path) {
                    Ok(()) => (),
                    Err(e) => panic!("{}", e),
                }
                // new unix listener
                let unix_listener = match UnixListener::bind(path) {
                    Ok(listener) => listener,
                    Err(e) => panic!("{}", e),
                };
                // convert tokio unix listener to std listener
                let std_listener = match unix_listener.into_std() {
                    Ok(std) => std,
                    Err(e) => panic!("{}", e),
                };
                // get unix std listener socket
                let socket: socket2::Socket = std_listener.into();
                // set listener backlog
                if let Err(e) = socket.listen(LISTENER_BACKLOG as i32) {
                    panic!("{}", e);
                }
                // convert back to tokio unix listener
                UnixListener::from_std(socket.into())
                    .map(Listener::from)
                    .unwrap_or_else(|e| panic!("{}", e))
            }
        }
    }
}
