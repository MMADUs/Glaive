use std::time::Duration;

use crate::stream::stream::Stream;
use bytes::{Bytes, BytesMut};
use http::header::AsHeaderName;
use http::{HeaderValue, Method, StatusCode, Uri, Version};
use httparse::{Request, Status};
use nix::NixPath;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::case::IntoCaseHeaderName;
use super::offset::{KVOffset, Offset};
use super::reader::BodyReader;
use super::request::RequestHeader;
use super::response::ResponseHeader;
use super::utils::{KeepaliveStatus, Utils};
use super::writer::BodyWriter;

const INIT_BUFFER_SIZE: usize = 1024;
const MAX_BUFFER_SIZE: usize = 8192;
const MAX_HEADERS_COUNT: usize = 256;

/// http 1.x downstream session
/// having a full control of the downstream here.
pub struct Downstream {
    // downstream (client) socket stream
    pub stream: Stream,
    // raw buffer for stream parse
    pub buffer: Bytes,
    // raw buffer request headers & body offset
    pub buf_headers_offset: Option<Offset>,
    pub buf_body_offset: Option<Offset>,
    // request & response headers
    pub request_headers: Option<RequestHeader>,
    pub response_headers: Option<ResponseHeader>,
    // request body reader & writer
    pub body_writer: BodyWriter,
    pub body_reader: BodyReader,
    // tracks size of bytes write and read to downstream
    pub buffer_size_sent: usize,
    pub buffer_size_read: usize,
    // connection keepalive timeout
    pub keepalive_timeout: KeepaliveStatus,
    // buffer write & read timeout
    pub read_timeout: Option<Duration>,
    pub write_timeout: Option<Duration>,
    // flag if request is an upgrade request
    pub upgrade: bool,
    // flag if response header is ignore (not proxied to downstream)
    pub ignore_response_headers: bool,
}

impl Downstream {
    pub fn new(s: Stream) -> Self {
        Downstream {
            stream: s,
            buffer: Bytes::new(),
            buf_headers_offset: None,
            buf_body_offset: None,
            request_headers: None,
            response_headers: None,
            body_writer: BodyWriter::new(),
            body_reader: BodyReader::new(),
            buffer_size_sent: 0,
            buffer_size_read: 0,
            keepalive_timeout: KeepaliveStatus::Off,
            read_timeout: None,
            write_timeout: None,
            upgrade: false,
            ignore_response_headers: false,
        }
    }

    /// # PARSE OPERATIONS
    ///
    /// read request from downstream and parse to session type
    pub async fn read_request(&mut self) -> tokio::io::Result<()> {
        self.buffer.clear();
        let mut read_buffer = BytesMut::with_capacity(INIT_BUFFER_SIZE);
        let mut read_buf_size = 0;

        loop {
            if read_buf_size > MAX_BUFFER_SIZE {
                return Err(tokio::io::Error::new(
                    tokio::io::ErrorKind::Other,
                    format!("request larger than {}", MAX_BUFFER_SIZE),
                ));
            }

            // TODO: add read timeouts here
            let len = match self.stream.read_buf(&mut read_buffer).await {
                Ok(0) if read_buf_size > 0 => {
                    return Err(tokio::io::Error::new(
                        tokio::io::ErrorKind::Other,
                        "connection closed",
                    ));
                }
                Ok(0) => return Ok(()),
                Ok(n) => n,
                Err(e) => return Err(e),
            };

            read_buf_size += len;

            let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS_COUNT];
            let mut request = Request::new(&mut headers);

            match request.parse(&read_buffer) {
                Ok(Status::Complete(size)) => {
                    let headers_offset = Offset::new(0, size);
                    let body_offset = Offset::new(size, read_buf_size);

                    self.buf_headers_offset = Some(headers_offset);
                    self.buf_body_offset = Some(body_offset);

                    let base = read_buffer.as_ptr() as usize;

                    let mut headers_offset = Vec::<KVOffset>::with_capacity(request.headers.len());

                    for header in request.headers.iter() {
                        if !header.name.is_empty() {
                            let name_start = header.name.as_ptr() as usize - base;
                            let value_start = header.value.as_ptr() as usize - base;

                            headers_offset.push(KVOffset::new(
                                name_start,
                                header.name.len(),
                                value_start,
                                header.value.len(),
                            ));
                        }
                    }

                    let version = match request.version {
                        Some(1) => Version::HTTP_11,
                        Some(0) => Version::HTTP_10,
                        _ => Version::HTTP_09,
                    };

                    let mut request_header = RequestHeader::build(
                        request.method.unwrap_or(""),
                        request.path.unwrap_or(""),
                        version,
                        Some(request.headers.len()),
                    );

                    let buffer_bytes = read_buffer.freeze();

                    for header in headers_offset {
                        let header_name = header.get_key_bytes(&buffer_bytes);
                        let header_value = header.get_value_bytes(&buffer_bytes);
                        let header_value =
                            unsafe { http::HeaderValue::from_maybe_shared_unchecked(header_value) };
                        request_header.append_header(header_name, header_value);
                    }

                    self.buffer = buffer_bytes;
                    self.request_headers = Some(request_header);

                    return Ok(());
                }
                Ok(Status::Partial) => continue,
                Err(e) => {
                    return Err(tokio::io::Error::new(
                        tokio::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                }
            }
        }
    }

    /// # REQUEST HEADERS OPERATIONS
    ///
    /// append request header
    pub fn append_header<N, V>(&mut self, name: N, value: V)
    where
        N: IntoCaseHeaderName,
        V: TryInto<HeaderValue>,
    {
        self.request_headers
            .as_mut()
            .expect("Request header is not read yet")
            .append_header(name, value);
    }

    /// insert request header
    pub fn insert_header<N, V>(&mut self, name: N, value: V)
    where
        N: IntoCaseHeaderName,
        V: TryInto<HeaderValue>,
    {
        self.request_headers
            .as_mut()
            .expect("Request header is not read yet")
            .insert_header(name, value);
    }

    /// remove request header
    pub fn remove_header<'a, N: ?Sized>(&mut self, name: &'a N)
    where
        &'a N: AsHeaderName,
    {
        self.request_headers
            .as_mut()
            .expect("Request header is not read yet")
            .remove_header(name);
    }

    /// get request headers
    pub fn get_headers<N>(&self, name: N) -> Vec<&HeaderValue>
    where
        N: AsHeaderName,
    {
        self.request_headers
            .as_ref()
            .expect("Request header is not read yet")
            .get_headers(name)
    }

    /// get request header
    pub fn get_header<N>(&self, name: N) -> Option<&HeaderValue>
    where
        N: AsHeaderName,
    {
        self.request_headers
            .as_ref()
            .expect("Request header is not read yet")
            .get_header(name)
    }

    /// get request version
    pub fn get_version(&self) -> &Version {
        self.request_headers
            .as_ref()
            .expect("Request header is not read yet")
            .get_version()
    }

    /// get request method
    pub fn get_method(&self) -> &Method {
        self.request_headers
            .as_ref()
            .expect("Request header is not read yet")
            .get_method()
    }

    /// get the raw request path
    pub fn get_raw_path(&self) -> &[u8] {
        self.request_headers
            .as_ref()
            .expect("Request header is not read yet")
            .get_raw_path()
    }

    /// get the request uri
    pub fn get_uri(&self) -> &Uri {
        self.request_headers
            .as_ref()
            .expect("Request header is not read yet")
            .get_uri()
    }

    /// # READ OPERATIONS
    ///
    /// get the parsed request headers
    /// both will result in panic if request is not readed yet
    ///
    /// the retrieved request header will be used to write to upstream
    pub fn get_request_headers(&self) -> &RequestHeader {
        self.request_headers
            .as_ref()
            .expect("Request header is not read yet")
    }

    pub fn get_mut_request_headers(&mut self) -> &mut RequestHeader {
        self.request_headers
            .as_mut()
            .expect("Request header is not read yet")
    }

    /// set the request body read mode
    /// this is mandatory before the body read process
    pub fn set_request_body_reader(&mut self) {
        // if reader is still on start we set the reader
        if self.body_reader.is_start() {
            // // reset retry buffer
            // if let Some(buffer) = self.retry_buffer.as_mut() {
            //     buffer.clear();
            // }

            let body_bytes = self.buf_body_offset.as_ref().unwrap().get(&self.buffer[..]);
            // if self.req_header().version == Version::HTTP_11 && self.is_upgrade_req() {
            //     self.body_reader.init_http10(preread_body);
            //     return;
            // }

            let transfer_encoding_value = self.get_header(http::header::TRANSFER_ENCODING);
            let transfer_encoding = Utils::is_header_value_chunk_encoding(transfer_encoding_value);

            if transfer_encoding {
                self.body_reader.with_chunked_read(body_bytes);
                return;
            }

            let content_length_value = self.get_header(http::header::CONTENT_LENGTH);
            let content_length = Utils::get_content_length_value(content_length_value);

            match content_length {
                Some(length) => self
                    .body_reader
                    .with_content_length_read(length, body_bytes),
                None => {
                    let version = *self.get_version();
                    match version {
                        Version::HTTP_11 => {
                            self.body_reader.with_content_length_read(0, body_bytes)
                        }
                        _ => self.body_reader.with_until_closed_read(body_bytes),
                    }
                }
            }
        }
    }

    /// read the request body after setting the read mode
    /// the readed body buffer is iniside the body reader, so we received the offset
    pub async fn read_request_body(&mut self) -> tokio::io::Result<Option<Offset>> {
        self.set_request_body_reader();
        // TODO: add read timeouts here
        self.body_reader.read_body(&mut self.stream).await
    }

    /// get the sliced request body with offset
    /// received the offset after reading the request body
    pub async fn read_sliced_request_body(&self, offset: &Offset) -> &[u8] {
        self.body_reader.get_sliced_body(offset)
    }

    /// read the request body into bytes
    /// this will be used to retrieve the actual body to upstream later
    pub async fn read_request_body_bytes(&mut self) -> tokio::io::Result<Option<Bytes>> {
        let offset = self.read_request_body().await?;
        match offset {
            Some(offset) => {
                let sliced = self.read_sliced_request_body(&offset).await;
                let bytes = Bytes::copy_from_slice(sliced);
                Ok(Some(bytes))
            }
            None => Ok(None),
        }
    }

    /// check if the reading process of the request body is finished
    pub async fn is_reading_request_body_finished(&mut self) -> bool {
        self.set_request_body_reader();
        self.body_reader.is_finished()
    }

    /// check if the request body is empty
    pub async fn is_request_body_empty(&mut self) -> bool {
        self.set_request_body_reader();
        self.body_reader.is_body_empty()
    }

    /// # WRITE OPERATIONS
    ///
    /// send a response headers to downstream
    /// this can be called multiple times e.g during error and writing resposne from upstream
    pub async fn write_response_headers(
        &mut self,
        headers: ResponseHeader,
    ) -> tokio::io::Result<()> {
        // TODO: do some validation here before writing response headers
        let mut resp_buf = headers.build_to_buffer();
        // TODO: add write timeouts here
        match self.stream.write_all(&mut resp_buf).await {
            Ok(()) => {
                self.response_headers = Some(headers); // idk why i store resp headers.
                self.buffer_size_sent += resp_buf.len();
                Ok(())
            }
            Err(e) => {
                let error = format!("error writing response header: {:>}", e);
                Err(tokio::io::Error::new(tokio::io::ErrorKind::Other, error))
            }
        }
    }

    /// after receiving response header from upstream
    /// set the response body write mode
    pub async fn set_response_body_writer(&mut self, header: &ResponseHeader) {
        // the response 204, 304, HEAD does not have body
        if matches!(
            header.metadata.status,
            StatusCode::NO_CONTENT | StatusCode::NOT_MODIFIED
        ) || self.get_method() == &Method::HEAD
        {
            self.body_writer.with_content_length_write(0);
            return;
        }

        // 1xx response, ignore body write
        if header.metadata.status.is_informational()
            && header.metadata.status != StatusCode::SWITCHING_PROTOCOLS
        {
            return;
        }

        // check if response is transfer encoding
        let transfer_encoding_value = header.get_header(http::header::TRANSFER_ENCODING);
        let transfer_encoding = Utils::is_header_value_chunk_encoding(transfer_encoding_value);

        // if transfer encoding, set writer to chunk encoding
        if transfer_encoding {
            self.body_writer.with_chunked_encoding_write();
            return;
        }

        // check if response has content length
        let content_length_value = header.get_header(http::header::CONTENT_LENGTH);
        let content_length = Utils::get_content_length_value(content_length_value);

        match content_length {
            Some(length) => self.body_writer.with_content_length_write(length),
            None => self.body_writer.with_until_closed_write(),
        }
    }

    /// write response body to downstream
    pub async fn write_response_body(&mut self, buffer: &[u8]) -> tokio::io::Result<Option<usize>> {
        // TODO: add write timeouts here
        let size_written = self.body_writer.write_body(&mut self.stream, buffer).await;

        if let Ok(Some(sent)) = size_written {
            self.buffer_size_sent += sent;
        }

        size_written
    }

    /// when write finished, this is executed
    /// used to flush the remaining data in buffer
    pub async fn finish_writing_response_body(&mut self) -> tokio::io::Result<Option<usize>> {
        let res = self.body_writer.finish(&mut self.stream).await?;
        self.stream.flush().await?;
        // self.maybe_force_close_body_reader();
        Ok(res)
    }
}
