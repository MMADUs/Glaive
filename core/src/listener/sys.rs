use libc::{self, c_int, c_void, socklen_t, c_ulonglong};
use std::io::{self, Error, ErrorKind};
use std::mem;
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

// safe wrapper for setting socket options
pub fn set_socket_option<T: Copy>(fd: c_int, opt: c_int, val: c_int, payload: T) -> io::Result<()> {
    let result = unsafe {
        let payload = &payload as *const T as *const c_void;
        libc::setsockopt(
            fd,
            opt,
            val,
            payload as *const _,
            mem::size_of::<T>() as socklen_t,
        )
    };

    if result == -1 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}


fn ip_bind_addr_no_port(fd: RawFd, val: bool) -> io::Result<()> {
    set_socket_option(
        fd,
        libc::IPPROTO_IP,
        libc::IP_BIND_ADDRESS_NO_PORT,
        val as c_int,
    )
}

fn ip_local_port_range(fd: RawFd, low: u16, high: u16) -> io::Result<()> {
    const IP_LOCAL_PORT_RANGE: i32 = 51;
    let range: u32 = (low as u32) | ((high as u32) << 16);

    let result = set_socket_option(fd, libc::IPPROTO_IP, IP_LOCAL_PORT_RANGE, range as c_int);
    match result {
        Err(e) if e.raw_os_error() != Some(libc::ENOPROTOOPT) => Err(e),
        _ => Ok(()), // no error or ENOPROTOOPT
    }
}

// SO_REUSEPORT: set socket option for reuseable port
pub fn set_reuse_port(fd: RawFd) -> io::Result<()> {
    let value: c_int = 1;
    set_socket_option(fd, libc::SOL_SOCKET, 15, &value) // 15 is SO_REUSEPORT
}

// SO_KEEPALIVE: set socket option for keepalive
pub fn set_so_keepalive(fd: RawFd, val: bool) -> io::Result<()> {
    set_socket_option(fd, libc::SOL_SOCKET, libc::SO_KEEPALIVE, val as c_int)
}

// SO_KEEPALIVE_IDLE: set keepalive idle timeout?
fn set_so_keepalive_idle(fd: RawFd, val: Duration) -> io::Result<()> {
    set_socket_option(
        fd,
        libc::IPPROTO_TCP,
        libc::TCP_KEEPIDLE,
        val.as_secs() as c_int,
    )
}

// SO_KEEPALIVE_INTERVAL: set keepalive interval
fn set_so_keepalive_interval(fd: RawFd, val: Duration) -> io::Result<()> {
    set_socket_option(
        fd,
        libc::IPPROTO_TCP,
        libc::TCP_KEEPINTVL,
        val.as_secs() as c_int,
    )
}

fn set_so_keepalive_count(fd: RawFd, val: usize) -> io::Result<()> {
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_KEEPCNT, val as c_int)
}

fn set_keepalive(fd: RawFd, ka: &KeepAliveConfig) -> io::Result<()> {
    set_so_keepalive(fd, true)?;
    set_so_keepalive_idle(fd, ka.idle_time)?;
    set_so_keepalive_interval(fd, ka.interval)?;
    set_so_keepalive_count(fd, ka.probe_count)
}

pub fn set_recv_buf(fd: RawFd, val: usize) -> io::Result<()> {
    set_socket_option(fd, libc::SOL_SOCKET, libc::SO_RCVBUF, val as c_int)
}

fn set_tcp_fastopen_connect(fd: RawFd) -> io::Result<()> {
    set_socket_option(
        fd,
        libc::IPPROTO_TCP,
        libc::TCP_FASTOPEN_CONNECT,
        1 as c_int,
    )
}

fn set_tcp_fastopen_backlog(fd: RawFd, backlog: usize) -> io::Result<()> {
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_FASTOPEN, backlog as c_int)
}

// fn set_dscp(fd: RawFd, value: u8) -> Result<()> {
//     use super::socket::SocketAddr;
//     use pingora_error::OkOrErr;
//
//     let sock = SocketAddr::from_raw_fd(fd, false);
//     let addr = sock
//         .as_ref()
//         .and_then(|s| s.as_inet())
//         .or_err(SocketError, "failed to set dscp, invalid IP socket")?;
//
//     if addr.is_ipv6() {
//         set_opt(fd, libc::IPPROTO_IPV6, libc::IPV6_TCLASS, value as c_int)
//             .or_err(SocketError, "failed to set dscp (IPV6_TCLASS)")
//     } else {
//         set_opt(fd, libc::IPPROTO_IP, libc::IP_TOS, value as c_int)
//             .or_err(SocketError, "failed to set dscp (IP_TOS)")
//     }
// }

// SO_TCP_FASTOPEN: set tcp fast open
pub fn set_tcp_fastopen(fd: RawFd, qlen: c_int) -> io::Result<()> {
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_FASTOPEN, &qlen)
}

// SO_TCP_QUICKACK: set tcp quick acknowledge
pub fn set_tcp_quickack(fd: RawFd, value: bool) -> io::Result<()> {
    let value: c_int = value as c_int;
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_QUICKACK, &value)
}

// SO_TCP_NODELAY: set tcp no delay (disable Nagle's algorithm)
pub fn set_tcp_nodelay(fd: RawFd, value: bool) -> io::Result<()> {
    let value: c_int = value as c_int;
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_NODELAY, &value)
}

// Set TCP_DEFER_ACCEPT
pub fn set_tcp_defer_accept(fd: RawFd, timeout: i32) -> io::Result<()> {
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_DEFER_ACCEPT, &timeout)
}

// Set TCP window clamp
pub fn set_tcp_window_clamp(fd: RawFd, size: i32) -> io::Result<()> {
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_WINDOW_CLAMP, &size)
}

// Set socket priority
pub fn set_socket_priority(fd: RawFd, priority: i32) -> io::Result<()> {
    set_socket_option(fd, libc::SOL_SOCKET, libc::SO_PRIORITY, &priority)
}

// Configuration for TCP keepalive
#[derive(Debug, Clone, Copy)]
pub struct KeepAliveConfig {
    // time before senidng keepalive probes
    pub idle_time: Duration,
    // time between keepalive probes
    pub interval: Duration,
    // number of probes before connection is considered dead
    pub probe_count: usize,
}

// new keep alive custom configuration
impl KeepAliveConfig {
    pub fn new(idle: usize, interval: usize, count: usize) -> Self {
        KeepAliveConfig {
            idle_time: Duration::from_secs(idle as u64),
            interval: Duration::from_secs(interval as u64),
            probe_count: count,
        }
    }
}

// default configuration for keep alive configuration
impl Default for KeepAliveConfig {
    fn default() -> Self {
        Self {
            idle_time: Duration::from_secs(60), // 60 seconds
            interval: Duration::from_secs(10),  // 10 seconds
            probe_count: 6,                     // 6 probes
        }
    }
}

// /// Example usage function that applies common optimizations
// pub fn apply_socket_optimizations(fd: RawFd) -> io::Result<()> {
//     // Enable port reuse
//     set_reuse_port(fd)?;
//
//     // TCP optimizations
//     set_tcp_fastopen(fd, 65535)?;  // Large queue length for high-load scenarios
//     set_tcp_quickack(fd, true)?;
//     set_tcp_nodelay(fd, true)?;
//     set_tcp_defer_accept(fd, 1)?;
//     set_tcp_window_clamp(fd, 4 * 1024 * 1024)?; // 4MB window
//
//     // Configure keepalive with default settings
//     configure_keepalive(fd, KeepAliveConfig::default())?;
//
//     // Set high priority
//     set_socket_priority(fd, 6)?;
//
//     Ok(())
// }
//
// // For high-performance server
// pub fn optimize_server_socket(fd: RawFd) -> io::Result<()> {
//     // Enable port reuse for better load balancing
//     set_reuse_port(fd)?;
//
//     // Large accept queue for high-load scenarios
//     set_tcp_fastopen(fd, 65535)?;
//
//     // Conservative keepalive for resource management
//     configure_keepalive(fd, KeepAliveConfig {
//         idle_time: 180,  // 3 minutes
//         interval: 30,    // 30 seconds
//         probe_count: 3,  // 3 probes
//     })?;
//
//     // Optimize for throughput
//     set_tcp_window_clamp(fd, 8 * 1024 * 1024)?;  // 8MB window
//
//     // Defer accept for better resource usage
//     set_tcp_defer_accept(fd, 1)?;
//
//     Ok(())
// }
//
// // For interactive client
// pub fn optimize_client_socket(fd: RawFd) -> io::Result<()> {
//     // Enable quick response
//     set_tcp_quickack(fd, true)?;
//     set_tcp_nodelay(fd, true)?;
//
//     // Smaller window for better memory usage
//     set_tcp_window_clamp(fd, 1 * 1024 * 1024)?;  // 1MB window
//
//     // Aggressive keepalive for faster connection loss detection
//     configure_keepalive(fd, KeepAliveConfig {
//         idle_time: 30,   // 30 seconds
//         interval: 5,     // 5 seconds
//         probe_count: 4,  // 4 probes
//     })?;
//
//     Ok(())
// }
//
// // For bulk transfer client
// pub fn optimize_bulk_transfer_socket(fd: RawFd) -> io::Result<()> {
//     // Disable quick ACK and NODELAY for better throughput
//     set_tcp_quickack(fd, false)?;
//     set_tcp_nodelay(fd, false)?;
//
//     // Large window for maximum throughput
//     set_tcp_window_clamp(fd, 16 * 1024 * 1024)?;  // 16MB window
//
//     // Relaxed keepalive for long-running transfers
//     configure_keepalive(fd, KeepAliveConfig {
//         idle_time: 300,  // 5 minutes
//         interval: 60,    // 1 minute
//         probe_count: 2,  // 2 probes
//     })?;
//
//     Ok(())
// }
