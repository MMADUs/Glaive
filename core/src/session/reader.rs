use bytes::{BufMut, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt};

use super::offset::Offset;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadState {
    // read state
    //
    // the initialized state
    Start,
    // read complete (total size)
    Completed(usize),
    // read done but interrupted (size read)
    Done(usize),

    // read mode
    //
    // read partially (size read, remaining size)
    Partial(usize, usize),
    // read chunked
    // (size read, next to read in current buf start, read in current buf start, remaining size)
    Chunked(usize, usize, usize, usize),
    // read until connection closed
    // (size read)
    HTTP10(usize),
}

impl ReadState {
    pub fn finish(&self, additional_bytes: usize) -> Self {
        match self {
            ReadState::Partial(read, to_read) => ReadState::Completed(read + to_read),
            ReadState::Chunked(read, _, _, _) => ReadState::Completed(read + additional_bytes),
            ReadState::HTTP10(read) => ReadState::Completed(read + additional_bytes),
            _ => self.clone(), /* invalid transaction */
        }
    }

    pub fn done(&self, additional_bytes: usize) -> Self {
        match self {
            ReadState::Partial(read, _) => ReadState::Done(read + additional_bytes),
            ReadState::Chunked(read, _, _, _) => ReadState::Done(read + additional_bytes),
            ReadState::HTTP10(read) => ReadState::Done(read + additional_bytes),
            _ => self.clone(), /* invalid transaction */
        }
    }

    pub fn partial_chunk(&self, bytes_read: usize, bytes_to_read: usize) -> Self {
        match self {
            ReadState::Chunked(read, _, _, _) => {
                ReadState::Chunked(read + bytes_read, 0, 0, bytes_to_read)
            }
            _ => self.clone(), /* invalid transaction */
        }
    }

    pub fn multi_chunk(&self, bytes_read: usize, buf_start_index: usize) -> Self {
        match self {
            ReadState::Chunked(read, _, buf_end, _) => {
                ReadState::Chunked(read + bytes_read, buf_start_index, *buf_end, 0)
            }
            _ => self.clone(), /* invalid transaction */
        }
    }

    pub fn partial_chunk_head(&self, head_end: usize, head_size: usize) -> Self {
        match self {
            /* inform reader to read more to form a legal chunk */
            ReadState::Chunked(read, _, _, _) => ReadState::Chunked(*read, 0, head_end, head_size),
            _ => self.clone(), /* invalid transaction */
        }
    }

    pub fn new_buf(&self, buf_end: usize) -> Self {
        match self {
            ReadState::Chunked(read, _, _, _) => ReadState::Chunked(*read, 0, buf_end, 0),
            _ => self.clone(), /* invalid transaction */
        }
    }
}

const BODY_BUFFER_SIZE: usize = 1024 * 64;
const PARTIAL_CHUNK_HEAD_LIMIT: usize = 1024 * 8;

#[derive(Debug)]
pub struct BodyReader {
    pub read_state: ReadState,
    pub body_buffer: Option<BytesMut>,
    pub body_size: usize,
    pub rewind_buf_size: usize,
}

impl BodyReader {
    pub fn new() -> Self {
        BodyReader {
            read_state: ReadState::Start,
            body_buffer: None,
            body_size: BODY_BUFFER_SIZE,
            rewind_buf_size: 0,
        }
    }

    // check if reader is still on start
    pub fn is_start(&self) -> bool {
        matches!(self.read_state, ReadState::Start)
    }

    // set state to start
    pub fn re_start(&mut self) {
        self.read_state = ReadState::Start;
    }

    // get sliced body by offset
    pub fn get_sliced_body(&self, offset: &Offset) -> &[u8] {
        offset.get(self.body_buffer.as_ref().unwrap())
    }

    // check if the reader is finished
    pub fn is_finished(&self) -> bool {
        matches!(self.read_state, ReadState::Completed(_) | ReadState::Done(_))
    }

    // check if the parsed body is empty
    pub fn is_body_empty(&self) -> bool {
        self.read_state == ReadState::Completed(0)
    }

    // initialize buffer
    fn set_buffer(&mut self, buf_to_rewind: &[u8]) {
        let mut buffer = BytesMut::with_capacity(self.body_size);

        if !buf_to_rewind.is_empty() {
            self.rewind_buf_size = buf_to_rewind.len();
            buffer.put_slice(buf_to_rewind);
        }

        if self.body_size > buf_to_rewind.len() {
            unsafe {
                buffer.set_len(self.body_size);
            }
        }

        self.body_buffer = Some(buffer);
    }

    // set read mode to chunked
    pub fn with_chunked_read(&mut self, buf_to_rewind: &[u8]) {
        self.read_state = ReadState::Chunked(0, 0, 0, 0);
        self.set_buffer(buf_to_rewind);
    }

    // set read mode with content length
    pub fn with_content_length_read(&mut self, length: usize, buf_to_rewind: &[u8]) {
        match length {
            0 => self.read_state = ReadState::Completed(0),
            _ => {
                self.set_buffer(buf_to_rewind);
                self.read_state = ReadState::Partial(0, length);
            }
        }
    }

    // set read mode with to http 1.0
    pub fn with_until_closed_read(&mut self, buf_to_rewind: &[u8]) {
        self.set_buffer(buf_to_rewind);
        self.read_state = ReadState::HTTP10(0);
    }

    // read the body with by the given read mode
    pub async fn read_body<S>(&mut self, stream: &mut S) -> tokio::io::Result<Option<Offset>>
    where
        S: AsyncRead + Unpin + Send,
    {
        match self.read_state {
            ReadState::Completed(_) => Ok(None),
            ReadState::Done(_) => Ok(None),
            ReadState::Partial(_, _) => self.read_partially(stream).await,
            ReadState::Chunked(_, _, _, _) => self.read_chunked(stream).await,
            ReadState::HTTP10(_) => self.read_until_closed(stream).await,
            ReadState::Start => panic!("reader is not initialized"),
        }
    }

    pub async fn read_partially<S>(&mut self, stream: &mut S) -> tokio::io::Result<Option<Offset>>
    where
        S: AsyncRead + Unpin + Send,
    {
        let buffer = self.body_buffer.as_deref_mut().unwrap();
        let mut n = self.rewind_buf_size;
        self.rewind_buf_size = 0; // we only need to read rewind data once

        if n == 0 {
            /* Need to actually read */
            n = stream.read(buffer).await?;
        }

        match self.read_state {
            ReadState::Partial(read, to_read) => {
                if n == 0 {
                    // connection closed
                    self.read_state = ReadState::Done(read);
                    Err(tokio::io::Error::new(
                        tokio::io::ErrorKind::Other,
                        "connection closed",
                    ))
                } else if n >= to_read {
                    // just giving warnings here
                    if n > to_read {
                        print!("send more data than expected");
                    }
                    // read successful
                    self.read_state = ReadState::Completed(read + to_read);
                    Ok(Some(Offset::new(0, to_read)))
                } else {
                    // left remaining
                    self.read_state = ReadState::Partial(read + n, to_read - n);
                    Ok(Some(Offset::new(0, n)))
                }
            }
            _ => panic!("executing wrong read mode"),
        }
    }

    pub async fn read_until_closed<S>(
        &mut self,
        stream: &mut S,
    ) -> tokio::io::Result<Option<Offset>>
    where
        S: AsyncRead + Unpin + Send,
    {
        let buffer = self.body_buffer.as_deref_mut().unwrap();
        let mut n = self.rewind_buf_size;
        self.rewind_buf_size = 0; // we only need to read rewind data once

        if n == 0 {
            /* Need to actually read */
            n = stream.read(buffer).await?;
        }

        match self.read_state {
            ReadState::HTTP10(read) => {
                if n == 0 {
                    // read until connection is closed successful
                    self.read_state = ReadState::Completed(read);
                    Ok(None)
                } else {
                    // read does not last until closed
                    self.read_state = ReadState::HTTP10(read + n);
                    Ok(Some(Offset::new(0, n)))
                }
            }
            _ => panic!("executing wrong read mode"),
        }
    }

    pub async fn read_chunked<S>(&mut self, stream: &mut S) -> tokio::io::Result<Option<Offset>>
    where
        S: AsyncRead + Unpin + Send,
    {
        match self.read_state {
            ReadState::Chunked(
                total_read,
                existing_buf_start,
                mut existing_buf_end,
                mut expecting_from_io,
            ) => {
                if existing_buf_start == 0 {
                    // read a new buf from IO
                    let buffer = self.body_buffer.as_deref_mut().unwrap();

                    if existing_buf_end == 0 {
                        existing_buf_end = self.rewind_buf_size;
                        self.rewind_buf_size = 0; // we only need to read rewind data once

                        if existing_buf_end == 0 {
                            existing_buf_end = stream.read(buffer).await?;
                        }
                    } else {
                        buffer
                            .copy_within(existing_buf_end - expecting_from_io..existing_buf_end, 0);

                        let new_bytes = stream.read(&mut buffer[expecting_from_io..]).await?;

                        /* more data is read, extend the buffer */
                        existing_buf_end = expecting_from_io + new_bytes;
                        expecting_from_io = 0;
                    }
                    self.read_state = self.read_state.new_buf(existing_buf_end);
                }

                if existing_buf_end == 0 {
                    self.read_state = self.read_state.done(0);
                    println!("total read: {:?}", total_read);
                    Err(tokio::io::Error::new(
                        tokio::io::ErrorKind::Other,
                        "connection closed",
                    ))
                } else {
                    if expecting_from_io > 0 {
                        // partial chunk payload, will read more
                        if expecting_from_io >= existing_buf_end + 2 {
                            // not enough
                            self.read_state = self.read_state.partial_chunk(
                                existing_buf_end,
                                expecting_from_io - existing_buf_end,
                            );
                            return Ok(Some(Offset::new(0, existing_buf_end)));
                        }

                        /* could be expecting DATA + CRLF or just CRLF */
                        let payload_size = if expecting_from_io > 2 {
                            expecting_from_io - 2
                        } else {
                            0
                        };

                        /* expecting_from_io < existing_buf_end + 2 */
                        if expecting_from_io >= existing_buf_end {
                            self.read_state = self
                                .read_state
                                .partial_chunk(payload_size, expecting_from_io - existing_buf_end);
                            return Ok(Some(Offset::new(0, payload_size)));
                        }

                        /* expecting_from_io < existing_buf_end */
                        self.read_state =
                            self.read_state.multi_chunk(payload_size, expecting_from_io);
                        return Ok(Some(Offset::new(0, payload_size)));
                    }
                    self.parse_chunked_buf(existing_buf_start, existing_buf_end)
                }
            }
            _ => panic!("executing wrong read mode"),
        }
    }

    fn parse_chunked_buf(
        &mut self,
        buf_index_start: usize,
        buf_index_end: usize,
    ) -> tokio::io::Result<Option<Offset>> {
        let buffer = &self.body_buffer.as_ref().unwrap()[buf_index_start..buf_index_end];
        let chunk_status = httparse::parse_chunk_size(buffer);
        match chunk_status {
            Ok(status) => {
                match status {
                    httparse::Status::Complete((payload_index, chunk_size)) => {
                        // TODO: Check chunk_size overflow
                        let chunk_size = chunk_size as usize;

                        if chunk_size == 0 {
                            /* terminating chunk. TODO: trailer */
                            self.read_state = self.read_state.finish(0);
                            return Ok(None);
                        }

                        // chunk-size CRLF [payload_index] byte*[chunk_size] CRLF
                        let data_end_index = payload_index + chunk_size;
                        let chunk_end_index = data_end_index + 2;

                        if chunk_end_index >= buffer.len() {
                            // no multi chunk in this buf
                            let actual_size = if data_end_index > buffer.len() {
                                buffer.len() - payload_index
                            } else {
                                chunk_size
                            };

                            self.read_state = self
                                .read_state
                                .partial_chunk(actual_size, chunk_end_index - buffer.len());

                            return Ok(Some(Offset::new(
                                buf_index_start + payload_index,
                                actual_size,
                            )));
                        }

                        /* got multiple chunks, return the first */
                        self.read_state = self
                            .read_state
                            .multi_chunk(chunk_size, buf_index_start + chunk_end_index);

                        Ok(Some(Offset::new(
                            buf_index_start + payload_index,
                            chunk_size,
                        )))
                    }
                    httparse::Status::Partial => {
                        if buffer.len() > PARTIAL_CHUNK_HEAD_LIMIT {
                            self.read_state = self.read_state.done(0);
                            Err(tokio::io::Error::new(
                                tokio::io::ErrorKind::Other,
                                "chunk is over limit",
                            ))
                        } else {
                            self.read_state = self
                                .read_state
                                .partial_chunk_head(buf_index_end, buffer.len());
                            Ok(Some(Offset::new(0, 0)))
                        }
                    }
                }
            }
            Err(e) => {
                self.read_state = self.read_state.done(0);
                Err(tokio::io::Error::new(
                    tokio::io::ErrorKind::Other,
                    e.to_string(),
                ))
            }
        }
    }
}
