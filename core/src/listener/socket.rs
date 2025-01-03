use nix::sys::socket::{getpeername, getsockname, SockaddrStorage};
use std::net::SocketAddr as StdSockAddr;
use std::os::unix::net::SocketAddr as StdUnixSockAddr;

#[derive(Debug, Clone)]
pub enum SocketAddress {
    Tcp(StdSockAddr),
    Unix(StdUnixSockAddr),
}

impl SocketAddress {
    pub fn as_tcp(&self) -> Option<&StdSockAddr> {
        if let SocketAddress::Tcp(address) = self {
            Some(address)
        } else {
            None
        }
    }

    pub fn as_unix(&self) -> Option<&StdUnixSockAddr> {
        if let SocketAddress::Unix(address) = self {
            Some(address)
        } else {
            None
        }
    }

    pub fn set_port(&mut self, port: u16) {
        if let SocketAddress::Tcp(address) = self {
            address.set_port(port);
        }
    }

    fn from_storage(sock: &SockaddrStorage) -> Option<SocketAddress> {
        // check for ipv4 & ipv6
        if let Some(v4) = sock.as_sockaddr_in() {
            let socket = std::net::SocketAddrV4::new(v4.ip().into(), v4.port());
            let address = SocketAddress::Tcp(StdSockAddr::V4(socket));
            return Some(address);
        } else if let Some(v6) = sock.as_sockaddr_in6() {
            let socket =
                std::net::SocketAddrV6::new(v6.ip(), v6.port(), v6.flowinfo(), v6.scope_id());
            let address = SocketAddress::Tcp(StdSockAddr::V6(socket));
            return Some(address);
        }
        // check for unix socket
        let unix_socket = sock
            .as_unix_addr()
            .map(|addr| addr.path().map(StdUnixSockAddr::from_pathname))??
            .ok()?;
        let address = SocketAddress::Unix(unix_socket);
        Some(address)
    }

    pub fn frow_raw_fd(fd: std::os::unix::io::RawFd, peer_address: bool) -> Option<SocketAddress> {
        // get address from fd
        let storage = if peer_address {
            getpeername(fd)
        } else {
            getsockname(fd)
        };
        // look for socket in storage
        match storage {
            Ok(socket) => Self::from_storage(&socket),
            Err(_) => None,
        }
    }
}
