use bytes::Buf;
use futures::ready;
use std::future::Future;
use std::io::IoSlice;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io;
use tokio::io::AsyncWrite;

/*
    the missing write_buf https://github.com/tokio-rs/tokio/pull/3156#issuecomment-738207409
    https://github.com/tokio-rs/tokio/issues/2610
    In general vectored write is lost when accessing the trait object: Box<S: AsyncWrite>
*/

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct WriteVec<'a, W, B> {
    writer: &'a mut W,
    buf: &'a mut B,
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct WriteVecAll<'a, W, B> {
    writer: &'a mut W,
    buf: &'a mut B,
}

pub trait AsyncWriteVec {
    fn poll_write_vec<B: Buf>(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut B,
    ) -> Poll<io::Result<usize>>;

    fn write_vec<'a, B>(&'a mut self, src: &'a mut B) -> WriteVec<'a, Self, B>
    where
        Self: Sized,
        B: Buf,
    {
        WriteVec {
            writer: self,
            buf: src,
        }
    }

    fn write_vec_all<'a, B>(&'a mut self, src: &'a mut B) -> WriteVecAll<'a, Self, B>
    where
        Self: Sized,
        B: Buf,
    {
        WriteVecAll {
            writer: self,
            buf: src,
        }
    }
}

impl<W, B> Future for WriteVec<'_, W, B>
where
    W: AsyncWriteVec + Unpin,
    B: Buf,
{
    type Output = io::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let me = &mut *self;
        Pin::new(&mut *me.writer).poll_write_vec(ctx, me.buf)
    }
}

impl<W, B> Future for WriteVecAll<'_, W, B>
where
    W: AsyncWriteVec + Unpin,
    B: Buf,
{
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let me = &mut *self;
        while me.buf.has_remaining() {
            let n = ready!(Pin::new(&mut *me.writer).poll_write_vec(ctx, me.buf))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
        }
        Poll::Ready(Ok(()))
    }
}

/* from https://github.com/tokio-rs/tokio/blob/master/tokio-util/src/lib.rs#L177 */
impl<T> AsyncWriteVec for T
where
    T: AsyncWrite,
{
    fn poll_write_vec<B: Buf>(
        self: Pin<&mut Self>,
        ctx: &mut Context,
        buf: &mut B,
    ) -> Poll<io::Result<usize>> {
        const MAX_BUFS: usize = 64;

        if !buf.has_remaining() {
            return Poll::Ready(Ok(0));
        }

        let n = if self.is_write_vectored() {
            let mut slices = [IoSlice::new(&[]); MAX_BUFS];
            let cnt = buf.chunks_vectored(&mut slices);
            ready!(self.poll_write_vectored(ctx, &slices[..cnt]))?
        } else {
            ready!(self.poll_write(ctx, buf.chunk()))?
        };

        buf.advance(n);

        Poll::Ready(Ok(n))
    }
}
