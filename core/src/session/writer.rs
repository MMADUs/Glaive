use bytes::{Buf, Bytes};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::stream::write_vec::AsyncWriteVec;

#[derive(Debug)]
pub enum WriteState {
    // write state
    //
    // the initialized state
    Start,
    // state when writing is completed
    // (written size)
    Completed(usize),

    // write mode
    //
    // writing body by content length
    // (total size to write, written size)
    ContentLength(usize, usize),
    // writing body by chunked encoding
    // (written size)
    ChunkedEncoding(usize),
    // writing body until closed
    // (written size)
    HTTP10(usize),
}

const LAST_CHUNK: &[u8; 5] = b"0\r\n\r\n";

#[derive(Debug)]
pub struct BodyWriter {
    write_state: WriteState,
}

impl BodyWriter {
    pub fn new() -> Self {
        BodyWriter {
            write_state: WriteState::Start,
        }
    }

    // set writing mode to write with content length
    pub fn with_content_length_write(&mut self, cl: usize) {
        self.write_state = WriteState::ContentLength(cl, 0);
    }

    // set writing mode to chunked encdoing
    pub fn with_chunked_encoding_write(&mut self) {
        self.write_state = WriteState::ChunkedEncoding(0);
    }

    // set writing mode to write until closed
    pub fn with_until_closed_write(&mut self) {
        self.write_state = WriteState::HTTP10(0);
    }

    // NOTE on buffering/flush stream when writing the body
    // Buffering writes can reduce the syscalls hence improves efficiency of the system
    // But it hurts real time communication
    // So we only allow buffering when the body size is known ahead, which is less likely
    // to be real time interaction

    pub async fn write_body<S>(
        &mut self,
        stream: &mut S,
        buffer: &[u8],
    ) -> tokio::io::Result<Option<usize>>
    where
        S: AsyncWrite + Unpin + Send,
    {
        match self.write_state {
            WriteState::Completed(_) => Ok(None),
            WriteState::ContentLength(_, _) => self.write_by_content_length(stream, buffer).await,
            WriteState::ChunkedEncoding(_) => self.write_by_chunked_encoding(stream, buffer).await,
            WriteState::HTTP10(_) => self.write_until_closed(stream, buffer).await,
            WriteState::Start => panic!("state is uninitialized"),
        }
    }

    pub fn finished(&self) -> bool {
        match self.write_state {
            WriteState::Completed(_) => true,
            WriteState::ContentLength(total, written) => written >= total,
            _ => false,
        }
    }

    async fn write_by_content_length<S>(
        &mut self,
        stream: &mut S,
        buffer: &[u8],
    ) -> tokio::io::Result<Option<usize>>
    where
        S: AsyncWrite + Unpin + Send,
    {
        match self.write_state {
            WriteState::ContentLength(total, written) => {
                if written >= total {
                    // already written full length
                    return Ok(None);
                }

                let mut to_write = total - written;

                if to_write < buffer.len() {
                    println!("Trying to write data over content-length: {total}");
                } else {
                    to_write = buffer.len();
                }

                let res = stream.write_all(&buffer[..to_write]).await;
                match res {
                    Ok(()) => {
                        self.write_state = WriteState::ContentLength(total, written + to_write);
                        if self.finished() {
                            stream.flush().await?;
                        }
                        Ok(Some(to_write))
                    }
                    Err(e) => Err(tokio::io::Error::new(
                        tokio::io::ErrorKind::Other,
                        format!("error writing: {:?}", e),
                    )),
                }
            }
            _ => panic!("executing wrong write state"),
        }
    }

    async fn write_by_chunked_encoding<S>(
        &mut self,
        stream: &mut S,
        buffer: &[u8],
    ) -> tokio::io::Result<Option<usize>>
    where
        S: AsyncWrite + Unpin + Send,
    {
        match self.write_state {
            WriteState::ChunkedEncoding(written) => {
                let chunk_size = buffer.len();

                let chuck_size_buf = format!("{:X}\r\n", chunk_size);
                let mut output_buf = Bytes::from(chuck_size_buf)
                    .chain(buffer)
                    .chain(&b"\r\n"[..]);

                stream.write_vec_all(&mut output_buf).await?;
                stream.flush().await?;

                self.write_state = WriteState::ChunkedEncoding(written + chunk_size);
                Ok(Some(chunk_size))
            }
            _ => panic!("executing wrong write state"),
        }
    }

    async fn write_until_closed<S>(
        &mut self,
        stream: &mut S,
        buffer: &[u8],
    ) -> tokio::io::Result<Option<usize>>
    where
        S: AsyncWrite + Unpin + Send,
    {
        match self.write_state {
            WriteState::HTTP10(written) => {
                let res = stream.write_all(buffer).await;
                match res {
                    Ok(()) => {
                        self.write_state = WriteState::HTTP10(written + buffer.len());
                        stream.flush().await?;
                        Ok(Some(buffer.len()))
                    }
                    Err(e) => Err(tokio::io::Error::new(
                        tokio::io::ErrorKind::Other,
                        format!("error writing: {:?}", e),
                    )),
                }
            }
            _ => panic!("executing wrong write state"),
        }
    }

    pub async fn finish<S>(&mut self, stream: &mut S) -> tokio::io::Result<Option<usize>>
    where
        S: AsyncWrite + Unpin + Send,
    {
        match self.write_state {
            WriteState::Completed(_) => Ok(None),
            WriteState::ContentLength(_, _) => self.finish_content_length(stream),
            WriteState::ChunkedEncoding(_) => self.finish_chunked_encoding(stream).await,
            WriteState::HTTP10(_) => self.finish_until_closed(stream),
            WriteState::Start => Ok(None),
        }
    }

    fn finish_content_length<S>(&mut self, _stream: S) -> tokio::io::Result<Option<usize>> {
        match self.write_state {
            WriteState::ContentLength(total, written) => {
                self.write_state = WriteState::Completed(written);
                if written < total {
                    return Err(tokio::io::Error::new(
                        tokio::io::ErrorKind::Other,
                        "premature body",
                    ));
                }
                Ok(Some(written))
            }
            _ => panic!("executing worng write state"),
        }
    }

    async fn finish_chunked_encoding<S>(
        &mut self,
        stream: &mut S,
    ) -> tokio::io::Result<Option<usize>>
    where
        S: AsyncWrite + Unpin + Send,
    {
        match self.write_state {
            WriteState::ChunkedEncoding(written) => {
                let res = stream.write_all(&LAST_CHUNK[..]).await;
                self.write_state = WriteState::Completed(written);
                match res {
                    Ok(()) => Ok(Some(written)),
                    Err(e) => Err(tokio::io::Error::new(
                        tokio::io::ErrorKind::Other,
                        format!("error writing: {:?}", e),
                    )),
                }
            }
            _ => panic!("executing wrong write state"),
        }
    }

    fn finish_until_closed<S>(&mut self, _stream: &mut S) -> tokio::io::Result<Option<usize>> {
        match self.write_state {
            WriteState::HTTP10(written) => {
                self.write_state = WriteState::Completed(written);
                Ok(Some(written))
            }
            _ => panic!("executing wrong write state"),
        }
    }
}
