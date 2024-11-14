use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{self, AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::net::UnixStream;

// raw stream type
// add more stream to be included in the types
#[derive(Debug)]
pub enum RawStream {
    Tcp(TcpStream),
    Unix(UnixStream),
}

// as raw fd (file descriptor) implementation for raw stream
// this used to extract file descriptors from raw stream
// returns file descriptor
impl AsRawFd for RawStream {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        match self {
            RawStream::Tcp(s) => s.as_raw_fd(),
            RawStream::Unix(s) => s.as_raw_fd(),
        }
    }
}

// async read implementation for raw stream type
impl AsyncRead for RawStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        unsafe {
            match &mut Pin::get_unchecked_mut(self) {
                RawStream::Tcp(s) => Pin::new_unchecked(s).poll_read(cx, buf),
                RawStream::Unix(s) => Pin::new_unchecked(s).poll_read(cx, buf),
            }
        }
    }
}

// async write implementation for raw stream type
impl AsyncWrite for RawStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        unsafe {
            match &mut Pin::get_unchecked_mut(self) {
                RawStream::Tcp(s) => Pin::new_unchecked(s).poll_write(cx, buf),
                RawStream::Unix(s) => Pin::new_unchecked(s).poll_write(cx, buf),
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        unsafe {
            match &mut Pin::get_unchecked_mut(self) {
                RawStream::Tcp(s) => Pin::new_unchecked(s).poll_flush(cx),
                RawStream::Unix(s) => Pin::new_unchecked(s).poll_flush(cx),
            }
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        unsafe {
            match &mut Pin::get_unchecked_mut(self) {
                RawStream::Tcp(s) => Pin::new_unchecked(s).poll_shutdown(cx),
                RawStream::Unix(s) => Pin::new_unchecked(s).poll_shutdown(cx),
            }
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        unsafe {
            match &mut Pin::get_unchecked_mut(self) {
                RawStream::Tcp(s) => Pin::new_unchecked(s).poll_write_vectored(cx, bufs),
                RawStream::Unix(s) => Pin::new_unchecked(s).poll_write_vectored(cx, bufs),
            }
        }
    }

    fn is_write_vectored(&self) -> bool {
        match self {
            RawStream::Tcp(s) => s.is_write_vectored(),
            RawStream::Unix(s) => s.is_write_vectored(),
        }
    }
}
