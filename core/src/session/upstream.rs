use std::time::Duration;

use bytes::{Bytes, BytesMut};
use http::{header::AsHeaderName, HeaderValue, StatusCode, Version};
use httparse::{Response, Status};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::stream::stream::Stream;

use super::{
    case::IntoCaseHeaderName,
    keepalive::KeepaliveStatus,
    offset::{KVOffset, Offset},
    reader::BodyReader,
    request::RequestHeader,
    response::ResponseHeader,
    task::Task,
    utils::Utils,
    writer::BodyWriter,
};

const INIT_BUFFER_SIZE: usize = 1024;
const MAX_BUFFER_SIZE: usize = 8192;
const MAX_HEADERS_COUNT: usize = 256;

/// http 1.x upstream session
/// having a full control of the upstream here.
pub struct Upstream {
    // downstream (client) socket stream
    pub stream: Stream,
    // raw buffer for stream parse
    pub buffer: Bytes,
    // raw buffer request headers & body offset
    pub buf_headers_offset: Option<Offset>,
    pub buf_body_offset: Option<Offset>,
    // request & response headers
    pub request_header: Option<RequestHeader>,
    pub response_header: Option<ResponseHeader>,
    // request body reader & writer
    pub body_writer: BodyWriter,
    pub body_reader: BodyReader,
    // tracks size of buffer write and read to downstream
    pub buf_write_size: usize,
    pub buf_read_size: usize,
    // connection keepalive timeout
    pub keepalive_timeout: KeepaliveStatus,
    // buffer write & read timeout
    pub read_timeout: Option<Duration>,
    pub write_timeout: Option<Duration>,
    // flag if request is an upgrade request
    pub upgrade: bool,
}

impl Upstream {
    /// new upstream
    pub fn new(s: Stream) -> Self {
        Upstream {
            stream: s,
            buffer: Bytes::new(),
            buf_headers_offset: None,
            buf_body_offset: None,
            request_header: None,
            response_header: None,
            body_writer: BodyWriter::new(),
            body_reader: BodyReader::new(),
            buf_write_size: 0,
            buf_read_size: 0,
            keepalive_timeout: KeepaliveStatus::Off,
            read_timeout: None,
            write_timeout: None,
            upgrade: false,
        }
    }

    /// read response from upstream and parse to session type
    pub async fn read_response(&mut self) -> tokio::io::Result<()> {
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
            let mut response = Response::new(&mut headers);

            let mut response_parser = httparse::ParserConfig::default();
            response_parser.allow_spaces_after_header_name_in_responses(true);
            response_parser.allow_obsolete_multiline_headers_in_responses(true);

            match response_parser.parse_response(&mut response, &read_buffer) {
                Ok(Status::Complete(size)) => {
                    let headers_offset = Offset::new(0, size);
                    let body_offset = Offset::new(size, read_buf_size);

                    self.buf_headers_offset = Some(headers_offset);
                    self.buf_body_offset = Some(body_offset);

                    let base = read_buffer.as_ptr() as usize;

                    let mut headers_offset = Vec::<KVOffset>::with_capacity(response.headers.len());

                    for header in response.headers.iter() {
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

                    let version = match response.version {
                        Some(1) => Version::HTTP_11,
                        Some(0) => Version::HTTP_10,
                        _ => Version::HTTP_09,
                    };

                    let mut response_header = ResponseHeader::build(
                        response.code.unwrap(),
                        version,
                        Some(response.headers.len()),
                    );

                    response_header.set_reason_phrase(response.reason);

                    let buffer_bytes = read_buffer.freeze();

                    for header in headers_offset {
                        let header_name = header.get_key_bytes(&buffer_bytes);
                        let header_value = header.get_value_bytes(&buffer_bytes);
                        let header_value =
                            unsafe { http::HeaderValue::from_maybe_shared_unchecked(header_value) };
                        response_header.append_header(header_name, header_value);
                    }

                    self.buffer = buffer_bytes;
                    self.response_header = Some(response_header);
                    self.response_header = None;

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
}

/// implementation for upstream response header
/// helper function
impl Upstream {
    /// append response header
    pub fn append_header<N, V>(&mut self, name: N, value: V)
    where
        N: IntoCaseHeaderName,
        V: TryInto<HeaderValue>,
    {
        self.response_header
            .as_mut()
            .expect("Response header is not read yet")
            .append_header(name, value);
    }

    /// insert response header
    pub fn insert_header<N, V>(&mut self, name: N, value: V)
    where
        N: IntoCaseHeaderName,
        V: TryInto<HeaderValue>,
    {
        self.response_header
            .as_mut()
            .expect("Response header is not read yet")
            .insert_header(name, value);
    }

    /// remove response header
    pub fn remove_header<'a, N: ?Sized>(&mut self, name: &'a N)
    where
        &'a N: AsHeaderName,
    {
        self.response_header
            .as_mut()
            .expect("Response header is not read yet")
            .remove_header(name);
    }

    /// get response headers
    pub fn get_headers<N>(&self, name: N) -> Vec<&HeaderValue>
    where
        N: AsHeaderName,
    {
        self.response_header
            .as_ref()
            .expect("Response header is not read yet")
            .get_headers(name)
    }

    /// get response header
    pub fn get_header<N>(&self, name: N) -> Option<&HeaderValue>
    where
        N: AsHeaderName,
    {
        self.response_header
            .as_ref()
            .expect("Response header is not read yet")
            .get_header(name)
    }

    /// get response version
    pub fn get_version(&self) -> &Version {
        self.response_header
            .as_ref()
            .expect("Response header is not read yet")
            .get_version()
    }

    /// get response status code
    pub fn get_status_code(&self) -> &StatusCode {
        self.response_header
            .as_ref()
            .expect("Response header is not read yet")
            .get_status_code()
    }

    /// get response reason phrase
    pub fn get_reason_phrase(&self) -> Option<&str> {
        self.response_header
            .as_ref()
            .expect("Response header is not read yet")
            .get_reason_phrase()
    }
}

/// implementation for read operations
/// with read helper function
impl Upstream {
    /// get the parsed response headers
    /// both will result in panic if response is not readed yet
    ///
    /// the retrieved response header will be used to write to downstream
    pub fn get_response_headers(&self) -> &ResponseHeader {
        self.response_header
            .as_ref()
            .expect("Response header is not read yet")
    }

    pub fn get_mut_response_headers(&mut self) -> &mut ResponseHeader {
        self.response_header
            .as_mut()
            .expect("Response header is not read yet")
    }

    /// set response body reader
    pub fn set_response_body_reader(&mut self) {
        if self.body_reader.is_start() {
            let body_bytes = self.buf_body_offset.as_ref().unwrap().get(&self.buffer[..]);

            // check if request is HEAD
            if let Some(req_header) = self.request_header.as_ref() {
                let method = req_header.get_method();
                if method == http::Method::HEAD {
                    self.body_reader.with_content_length_read(0, body_bytes);
                    return;
                }
            }

            // check if status is an upgrade
            let upgrade = {
                let status_code = self.get_status_code();
                let code = status_code.as_u16();
                match code {
                    101 => self.is_request_upgrade(),
                    100..=199 => {
                        // Ignore informational headers
                        return;
                    }
                    204 | 304 => {
                        // No body for these status codes
                        self.body_reader.with_content_length_read(0, body_bytes);
                        return;
                    }
                    _ => false,
                }
            };

            if upgrade {
                self.body_reader.with_until_closed_read(body_bytes);
                return;
            }

            // check if request is transfer encoding
            let transfer_encoding_value = self.get_header(http::header::TRANSFER_ENCODING);
            let transfer_encoding = Utils::is_header_value_chunk_encoding(transfer_encoding_value);

            if transfer_encoding {
                self.body_reader.with_chunked_read(body_bytes);
                return;
            }

            // check if request has content length
            let content_length_value = self.get_header(http::header::CONTENT_LENGTH);
            let content_length = Utils::get_content_length_value(content_length_value);

            match content_length {
                Some(length) => self
                    .body_reader
                    .with_content_length_read(length, body_bytes),
                None => self.body_reader.with_until_closed_read(body_bytes),
            }
        }
    }

    /// read the response body after setting the read mode
    /// the readed body buffer is iniside the body reader, so we received the offset
    pub async fn read_response_body(&mut self) -> tokio::io::Result<Option<Offset>> {
        self.set_response_body_reader();
        // TODO: add read timeouts here
        self.body_reader.read_body(&mut self.stream).await
    }

    /// get the sliced response body with offset
    /// received the offset after reading the response body
    pub async fn read_sliced_response_body(&self, offset: &Offset) -> &[u8] {
        self.body_reader.get_sliced_body(offset)
    }

    /// read the response body into bytes
    /// this will be used to retrieve the actual body to downstream later
    pub async fn read_response_body_bytes(&mut self) -> tokio::io::Result<Option<Bytes>> {
        let offset = self.read_response_body().await?;
        match offset {
            Some(offset) => {
                let sliced = self.read_sliced_response_body(&offset).await;
                let bytes = Bytes::copy_from_slice(sliced);
                self.buf_read_size += bytes.len();
                Ok(Some(bytes))
            }
            None => Ok(None),
        }
    }

    /// check if the reading process of the response body is finished
    pub fn is_reading_response_body_finished(&mut self) -> bool {
        self.set_response_body_reader();
        self.body_reader.is_finished()
    }

    /// check if the response body is empty
    pub fn is_response_body_empty(&mut self) -> bool {
        self.set_response_body_reader();
        self.body_reader.is_body_empty()
    }

    /// force close response body reader
    pub fn force_close_response_body_reader(&mut self) {
        if self.upgrade && !self.body_reader.is_finished() {
            // reset to close
            self.body_reader.with_content_length_read(0, b"");
        }
    }

    /// check if we should read response header
    pub fn should_read_response_header(&self) -> bool {
        let status_code = self.get_status_code();
        match status_code.as_u16() {
            101 => false,
            100..=199 => true,
            _ => false,
        }
    }

    /// function associated to read upstream buffer
    pub async fn read_upstream_response(&mut self) -> tokio::io::Result<Task> {
        if self.response_header.is_none() && self.should_read_response_header() {
            self.read_response().await?;
            let resp_header = self.response_header.clone().unwrap();
            let end_of_body = self.is_reading_response_body_finished();
            // send header task
            Ok(Task::Header(resp_header, end_of_body))
        } else if self.is_reading_response_body_finished() {
            // send task is done
            Ok(Task::Done)
        } else {
            let body = self.read_response_body_bytes().await?;
            let end_of_body = self.is_reading_response_body_finished();
            // send body task
            Ok(Task::Body(body, end_of_body))
        }
    }
}

/// implementation for write opeations
/// with write helper function
impl Upstream {
    /// set request body writer mode
    pub fn set_request_body_writer(&mut self, req_header: &RequestHeader) {
        // check if request is an upgrade
        if Utils::is_request_upgrade(req_header) {
            self.body_writer.with_until_closed_write();
            return;
        }

        // check if request is transfer encoding
        let transfer_encoding_value = req_header.get_header(http::header::TRANSFER_ENCODING);
        let transfer_encoding = Utils::is_header_value_chunk_encoding(transfer_encoding_value);

        if transfer_encoding {
            self.body_writer.with_chunked_encoding_write();
            return;
        }

        // check if response has content length
        let content_length_value = req_header.get_header(http::header::CONTENT_LENGTH);
        let content_length = Utils::get_content_length_value(content_length_value);

        match content_length {
            Some(length) => self.body_writer.with_content_length_write(length),
            None => self.body_writer.with_until_closed_write(),
        }
    }

    /// write request header to upstream
    pub async fn write_request_header(
        &mut self,
        req_header: RequestHeader,
    ) -> tokio::io::Result<()> {
        self.set_request_body_writer(&req_header);
        let mut req_buf = req_header.build_to_buffer();
        // TODO: add write timeout here
        match self.stream.write_all(&mut req_buf).await {
            Ok(()) => {
                self.stream.flush().await?;
                self.request_header = Some(req_header);
                self.buf_write_size += req_buf.len();
                Ok(())
            }
            Err(e) => {
                let error = format!("error writing response header: {:>}", e);
                Err(tokio::io::Error::new(tokio::io::ErrorKind::Other, error))
            }
        }
    }

    /// write request body to upstream
    pub async fn write_request_body(&mut self, buffer: &[u8]) -> tokio::io::Result<Option<usize>> {
        // TODO: add write timeouts here
        let size_written = self.body_writer.write_body(&mut self.stream, buffer).await;

        if let Ok(Some(sent)) = size_written {
            self.buf_write_size += sent;
        }

        size_written
    }

    /// when write finished, this is executed
    /// used to flush the remaining data in buffer
    pub async fn finish_writing_request_body(&mut self) -> tokio::io::Result<Option<usize>> {
        let res = self.body_writer.finish(&mut self.stream).await?;
        self.stream.flush().await?;
        self.force_close_response_body_reader();
        Ok(res)
    }

    /// the associated function for writing request to upstream
    pub async fn write_upstream_request(&mut self, task: Task) -> tokio::io::Result<bool> {
        let end_stream = match task {
            Task::Body(req_body, end_stream) => {
                if let Some(body) = req_body {
                    self.write_request_body(&body).await?;
                }
                end_stream
            }
            // should never happend, sender only send body
            _ => panic!("unexpected request to write"),
        };
        // check if end of stream
        if end_stream {
            self.finish_writing_request_body().await?;
        }
        Ok(end_stream)
    }
}

/// implementation for extra utilities
/// helper function to work with upstream
impl Upstream {
    /// check if the request is an request upgrade
    pub fn is_request_upgrade(&self) -> bool {
        let req_header = self
            .request_header
            .as_ref()
            .expect("Request is not read yet");
        Utils::is_request_upgrade(req_header)
    }

    /// check if the session (request & response) is an upgrade
    pub fn is_session_upgrade(&self, resp_header: &ResponseHeader) -> bool {
        match self.is_request_upgrade() {
            true => Utils::is_response_upgrade(resp_header),
            false => false,
        }
    }

    /// set downstream keepalive timeout
    pub fn set_keepalive(&mut self, seconds: Option<u64>) {
        match seconds {
            Some(sec) => {
                if sec > 0 {
                    self.keepalive_timeout = KeepaliveStatus::Timeout(Duration::from_secs(sec));
                } else {
                    self.keepalive_timeout = KeepaliveStatus::Infinite;
                }
            }
            None => {
                self.keepalive_timeout = KeepaliveStatus::Off;
            }
        }
    }

    /// check if downstream session keepalive is active
    pub fn is_session_keepalive(&self) -> bool {
        !matches!(self.keepalive_timeout, KeepaliveStatus::Off)
    }

    /// check if connection keepalive
    pub fn is_connection_keepalive(&self) -> Option<bool> {
        if let Some(value) = self.get_header(http::header::CONNECTION) {
            return Utils::is_connection_keepalive(value);
        }
        None
    }

    /// get keepalive value from socket stream
    pub fn get_keepalive_value(&self) -> (Option<u64>, Option<usize>) {
        let Some(keepalive_header) = self.get_header("Keep-Alive") else {
            return (None, None);
        };

        let Ok(header_value) = std::str::from_utf8(keepalive_header.as_bytes()) else {
            return (None, None);
        };

        let mut timeout = None;
        let mut max = None;

        for param in header_value.split(",") {
            let parts = param.split_once("=").map(|(k, v)| (k.trim(), v));
            match parts {
                Some(("timeout", timeout_value)) => timeout = timeout_value.trim().parse().ok(),
                Some(("max", max_value)) => max = max_value.trim().parse().ok(),
                _ => {}
            }
        }

        (timeout, max)
    }

    /// apply session keepalive timeout
    pub fn apply_session_keepalive(&mut self) {
        if let Some(keepalive) = self.is_connection_keepalive() {
            if keepalive {
                let (timeout, _max_use) = self.get_keepalive_value();

                match timeout {
                    Some(timeout) => self.set_keepalive(Some(timeout)),
                    None => self.set_keepalive(Some(0)), // infinite ?
                }
            } else {
                self.set_keepalive(None);
            }
        } else if *self.get_version() == http::Version::HTTP_11 {
            self.set_keepalive(Some(0)); // on by default for http 1.1
        } else {
            self.set_keepalive(None); // off by default for http 1.0
        }
    }

    /// consume stream to be returned to connection pool
    pub fn return_stream(self) -> Stream {
        self.stream
    }
}
