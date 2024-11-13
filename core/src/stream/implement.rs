use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, BufStream, ReadBuf};
use tokio::net::TcpStream;
use tokio::net::UnixStream;

use crate::stream::{wrapper::RawStreamWrapper, raw::RawStream, traits::UniqueID};

// Large read buffering helps reducing syscalls with little trade-off
// Ssl layer always does "small" reads in 16k (TLS record size) so L4 read buffer helps a lot.
const BUF_READ_SIZE: usize = 64 * 1024;
// Small write buf to match MSS. Too large write buf delays real time communication.
// This buffering effectively implements something similar to Nagle's algorithm.
// The benefit is that user space can control when to flush, where Nagle's can't be controlled.
// And userspace buffering reduce both syscalls and small packets.
const BUF_WRITE_SIZE: usize = 1460;

// NOTE: with writer buffering, users need to call flush() to make sure the data is actually
// sent. Otherwise data could be stuck in the buffer forever or get lost when stream is closed.

/// A concrete type for transport layer connection + extra fields for logging
#[derive(Debug)]
pub struct Stream {
    stream: BufStream<RawStreamWrapper>,
    rewind_read_buf: Vec<u8>,
}

impl From<TcpStream> for Stream {
    fn from(s: TcpStream) -> Self {
        Stream {
            stream: BufStream::with_capacity(
                BUF_READ_SIZE,
                BUF_WRITE_SIZE,
                RawStreamWrapper::new(RawStream::Tcp(s)),
            ),
            rewind_read_buf: Vec::new(),
        }
    }
}

#[cfg(unix)]
impl From<UnixStream> for Stream {
    fn from(s: UnixStream) -> Self {
        Stream {
            stream: BufStream::with_capacity(
                BUF_READ_SIZE,
                BUF_WRITE_SIZE,
                RawStreamWrapper::new(RawStream::Unix(s)),
            ),
            rewind_read_buf: Vec::new(),
        }
    }
}

#[cfg(unix)]
impl AsRawFd for Stream {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        self.stream.get_ref().as_raw_fd()
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let result = if !self.rewind_read_buf.is_empty() {
            let mut data_to_read = self.rewind_read_buf.as_slice();
            let result = Pin::new(&mut data_to_read).poll_read(cx, buf);
            let remaining_buf = Vec::from(data_to_read);
            let _ = std::mem::replace(&mut self.rewind_read_buf, remaining_buf);
            result
        } else {
            Pin::new(&mut self.stream).poll_read(cx, buf)
        };
        result
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.stream.get_mut()).poll_write(cx, buf);
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

impl UniqueID for Stream {
    fn id(&self) -> i32 {
        self.as_raw_fd()
    }
}
