use std::io::{self, Error, ErrorKind};
use std::mem;
use std::os::unix::io::{AsRawFd, RawFd};
use libc::{self, c_int, c_void, socklen_t};

/// Safe wrapper for setting socket options
pub fn set_socket_option<T>(fd: RawFd, level: c_int, option: c_int, value: &T) -> io::Result<()> {
    let result = unsafe {
        libc::setsockopt(
            fd,
            level,
            option,
            (value as *const T) as *const c_void,
            mem::size_of::<T>() as socklen_t,
        )
    };
    
    if result == -1 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Enable SO_REUSEPORT on the socket
pub fn set_reuse_port(fd: RawFd) -> io::Result<()> {
    let value: c_int = 1;
    set_socket_option(fd, libc::SOL_SOCKET, 15, &value) // 15 is SO_REUSEPORT
}

/// Set TCP Fast Open
pub fn set_tcp_fastopen(fd: RawFd, qlen: c_int) -> io::Result<()> {
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_FASTOPEN, &qlen)
}

/// Set TCP Quick ACK mode
pub fn set_tcp_quickack(fd: RawFd, value: bool) -> io::Result<()> {
    let value: c_int = value as c_int;
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_QUICKACK, &value)
}

/// Set TCP_NODELAY (disable Nagle's algorithm)
pub fn set_tcp_nodelay(fd: RawFd, value: bool) -> io::Result<()> {
    let value: c_int = value as c_int;
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_NODELAY, &value)
}

/// Set TCP_DEFER_ACCEPT
pub fn set_tcp_defer_accept(fd: RawFd, timeout: i32) -> io::Result<()> {
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_DEFER_ACCEPT, &timeout)
}

/// Set TCP window clamp
pub fn set_tcp_window_clamp(fd: RawFd, size: i32) -> io::Result<()> {
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_WINDOW_CLAMP, &size)
}

/// Set socket priority
pub fn set_socket_priority(fd: RawFd, priority: i32) -> io::Result<()> {
    set_socket_option(fd, libc::SOL_SOCKET, libc::SO_PRIORITY, &priority)
}

/// Configure TCP keepalive settings
pub fn configure_keepalive(fd: RawFd, config: KeepAliveConfig) -> io::Result<()> {
    // Enable keepalive
    let value: c_int = 1;
    set_socket_option(fd, libc::SOL_SOCKET, libc::SO_KEEPALIVE, &value)?;
    
    // Set keepalive parameters
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, &config.idle_time)?;
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, &config.interval)?;
    set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_KEEPCNT, &config.probe_count)?;
    
    Ok(())
}

/// Configuration for TCP keepalive
#[derive(Debug, Clone, Copy)]
pub struct KeepAliveConfig {
    pub idle_time: i32,   // Time before sending keepalive probes
    pub interval: i32,    // Time between keepalive probes
    pub probe_count: i32, // Number of probes before connection is considered dead
}

impl Default for KeepAliveConfig {
    fn default() -> Self {
        Self {
            idle_time: 60,  // 60 seconds
            interval: 10,   // 10 seconds
            probe_count: 6, // 6 probes
        }
    }
}

/// Example usage function that applies common optimizations
pub fn apply_socket_optimizations(fd: RawFd) -> io::Result<()> {
    // Enable port reuse
    set_reuse_port(fd)?;
    
    // TCP optimizations
    set_tcp_fastopen(fd, 65535)?;  // Large queue length for high-load scenarios
    set_tcp_quickack(fd, true)?;
    set_tcp_nodelay(fd, true)?;
    set_tcp_defer_accept(fd, 1)?;
    set_tcp_window_clamp(fd, 4 * 1024 * 1024)?; // 4MB window
    
    // Configure keepalive with default settings
    configure_keepalive(fd, KeepAliveConfig::default())?;
    
    // Set high priority
    set_socket_priority(fd, 6)?;
    
    Ok(())
}

// For high-performance server
pub fn optimize_server_socket(fd: RawFd) -> io::Result<()> {
    // Enable port reuse for better load balancing
    set_reuse_port(fd)?;
    
    // Large accept queue for high-load scenarios
    set_tcp_fastopen(fd, 65535)?;
    
    // Conservative keepalive for resource management
    configure_keepalive(fd, KeepAliveConfig {
        idle_time: 180,  // 3 minutes
        interval: 30,    // 30 seconds
        probe_count: 3,  // 3 probes
    })?;
    
    // Optimize for throughput
    set_tcp_window_clamp(fd, 8 * 1024 * 1024)?;  // 8MB window
    
    // Defer accept for better resource usage
    set_tcp_defer_accept(fd, 1)?;
    
    Ok(())
}

// For interactive client
pub fn optimize_client_socket(fd: RawFd) -> io::Result<()> {
    // Enable quick response
    set_tcp_quickack(fd, true)?;
    set_tcp_nodelay(fd, true)?;
    
    // Smaller window for better memory usage
    set_tcp_window_clamp(fd, 1 * 1024 * 1024)?;  // 1MB window
    
    // Aggressive keepalive for faster connection loss detection
    configure_keepalive(fd, KeepAliveConfig {
        idle_time: 30,   // 30 seconds
        interval: 5,     // 5 seconds
        probe_count: 4,  // 4 probes
    })?;
    
    Ok(())
}

// For bulk transfer client
pub fn optimize_bulk_transfer_socket(fd: RawFd) -> io::Result<()> {
    // Disable quick ACK and NODELAY for better throughput
    set_tcp_quickack(fd, false)?;
    set_tcp_nodelay(fd, false)?;
    
    // Large window for maximum throughput
    set_tcp_window_clamp(fd, 16 * 1024 * 1024)?;  // 16MB window
    
    // Relaxed keepalive for long-running transfers
    configure_keepalive(fd, KeepAliveConfig {
        idle_time: 300,  // 5 minutes
        interval: 60,    // 1 minute
        probe_count: 2,  // 2 probes
    })?;
    
    Ok(())
}
