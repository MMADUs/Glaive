use std::time::Duration;

use crate::stream::stream::Stream;
use bytes::{BufMut, Bytes, BytesMut};
use http::header::AsHeaderName;
use http::{HeaderValue, Method, StatusCode, Uri, Version};
use httparse::{Request, Status};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::task::Task;
use super::{
    case::IntoCaseHeaderName,
    keepalive::KeepaliveStatus,
    offset::{KVOffset, Offset},
    reader::BodyReader,
    request::RequestHeader,
    response::ResponseHeader,
    utils::Utils,
    writer::BodyWriter,
};

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
    // buffer used for vectored write body
    pub write_body_vec_buffer: BytesMut,
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
    // flag if response header is modified
    pub update_response_headers: bool,
    // flag if response header is ignore (not proxied to downstream)
    pub ignore_response_headers: bool,
}

impl Downstream {
    /// new downstream session
    pub fn new(s: Stream) -> Self {
        Downstream {
            stream: s,
            buffer: Bytes::new(),
            write_body_vec_buffer: BytesMut::new(),
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
            update_response_headers: true,
            ignore_response_headers: false,
        }
    }

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
                    self.request_header = Some(request_header);
                    self.body_reader.re_start();
                    self.response_header = None;
                    self.apply_session_keepalive();

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

/// implementation for downstream request header
/// helper function
impl Downstream {
    /// append request header
    pub fn append_header<N, V>(&mut self, name: N, value: V)
    where
        N: IntoCaseHeaderName,
        V: TryInto<HeaderValue>,
    {
        self.request_header
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
        self.request_header
            .as_mut()
            .expect("Request header is not read yet")
            .insert_header(name, value);
    }

    /// remove request header
    pub fn remove_header<'a, N: ?Sized>(&mut self, name: &'a N)
    where
        &'a N: AsHeaderName,
    {
        self.request_header
            .as_mut()
            .expect("Request header is not read yet")
            .remove_header(name);
    }

    /// get request headers
    pub fn get_headers<N>(&self, name: N) -> Vec<&HeaderValue>
    where
        N: AsHeaderName,
    {
        self.request_header
            .as_ref()
            .expect("Request header is not read yet")
            .get_headers(name)
    }

    /// get request header
    pub fn get_header<N>(&self, name: N) -> Option<&HeaderValue>
    where
        N: AsHeaderName,
    {
        self.request_header
            .as_ref()
            .expect("Request header is not read yet")
            .get_header(name)
    }

    /// get request version
    pub fn get_version(&self) -> &Version {
        self.request_header
            .as_ref()
            .expect("Request header is not read yet")
            .get_version()
    }

    /// get request method
    pub fn get_method(&self) -> &Method {
        self.request_header
            .as_ref()
            .expect("Request header is not read yet")
            .get_method()
    }

    /// get the raw request path
    pub fn get_raw_path(&self) -> &[u8] {
        self.request_header
            .as_ref()
            .expect("Request header is not read yet")
            .get_raw_path()
    }

    /// get the request uri
    pub fn get_uri(&self) -> &Uri {
        self.request_header
            .as_ref()
            .expect("Request header is not read yet")
            .get_uri()
    }
}

/// implementation for read operations
/// with read helper function
impl Downstream {
    /// get the parsed request headers
    /// both will result in panic if request is not readed yet
    ///
    /// the retrieved request header will be used to write to upstream
    pub fn get_request_headers(&self) -> &RequestHeader {
        self.request_header
            .as_ref()
            .expect("Request header is not read yet")
    }

    pub fn get_mut_request_headers(&mut self) -> &mut RequestHeader {
        self.request_header
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

            // check if request is an upgrade
            let version = *self.get_version();
            if version == Version::HTTP_11 && self.is_request_upgrade() {
                self.body_reader.with_until_closed_read(body_bytes);
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
                self.buf_read_size += bytes.len();
                Ok(Some(bytes))
            }
            None => Ok(None),
        }
    }

    /// check if the reading process of the request body is finished
    pub fn is_reading_request_body_finished(&mut self) -> bool {
        self.set_request_body_reader();
        self.body_reader.is_finished()
    }

    /// check if the request body is empty
    pub fn is_request_body_empty(&mut self) -> bool {
        self.set_request_body_reader();
        self.body_reader.is_body_empty()
    }

    /// force close request body reader
    pub fn force_close_request_body_reader(&mut self) {
        if self.upgrade && !self.body_reader.is_finished() {
            // reset to close
            self.body_reader.with_content_length_read(0, b"");
        }
    }

    /// read process for idle
    pub async fn idle(&mut self) -> tokio::io::Result<usize> {
        let mut buf: [u8; 1] = [0; 1];
        self.stream.read(&mut buf).await
    }

    /// read the request body or idle when calling it the next time
    pub async fn read_request_body_or_idle(
        &mut self,
    ) -> tokio::io::Result<Option<Bytes>> {
        if self.is_reading_request_body_finished() {
            let read = self.idle().await?;
            if read == 0 {
                return Err(tokio::io::Error::new(
                    tokio::io::ErrorKind::Other,
                    "connection closed",
                ));
            } else {
                return Err(tokio::io::Error::new(
                    tokio::io::ErrorKind::Other,
                    "connect error",
                ));
            }
        } else {
            self.read_request_body_bytes().await
        }
    }

    /// the function associated to read the downstream request
    pub async fn read_downstream_request(&mut self) -> tokio::io::Result<Task> {
        match self.read_request_body_or_idle().await {
            Ok(req_body) => {
                // if request upgrade, mark as done
                if req_body.is_none() && self.is_request_upgrade() {
                    return Ok(Task::Done)
                }
                // proceed request body
                let end_stream = self.is_reading_request_body_finished();
                Ok(Task::Body(req_body, end_stream))
            },
            Err(e) => Err(e),
        }
    }
}

/// implementation for write opeations
/// with write helper function
impl Downstream {
    /// after receiving response header from upstream
    /// set the response body write mode
    pub async fn set_response_body_writer(&mut self, resp_header: &ResponseHeader) {
        // the response 204, 304, HEAD does not have body
        if matches!(
            resp_header.metadata.status,
            StatusCode::NO_CONTENT | StatusCode::NOT_MODIFIED
        ) || self.get_method() == &Method::HEAD
        {
            self.body_writer.with_content_length_write(0);
            return;
        }

        // 1xx response, ignore body write
        if resp_header.metadata.status.is_informational()
            && resp_header.metadata.status != StatusCode::SWITCHING_PROTOCOLS
        {
            return;
        }

        // check if session is upgrade
        if self.is_session_upgrade(resp_header) {
            self.body_writer.with_until_closed_write();
            return;
        }

        // check if response is transfer encoding
        let transfer_encoding_value = resp_header.get_header(http::header::TRANSFER_ENCODING);
        let transfer_encoding = Utils::is_header_value_chunk_encoding(transfer_encoding_value);

        // if transfer encoding, set writer to chunk encoding
        if transfer_encoding {
            self.body_writer.with_chunked_encoding_write();
            return;
        }

        // check if response has content length
        let content_length_value = resp_header.get_header(http::header::CONTENT_LENGTH);
        let content_length = Utils::get_content_length_value(content_length_value);

        match content_length {
            Some(length) => self.body_writer.with_content_length_write(length),
            None => self.body_writer.with_until_closed_write(),
        }
    }

    /// send a response headers to downstream
    /// this can be called multiple times e.g during error and writing resposne from upstream
    pub async fn write_response_headers(
        &mut self,
        mut resp_header: ResponseHeader,
    ) -> tokio::io::Result<()> {
        // check if response headers should be ignored
        if resp_header.metadata.status.is_informational()
            && self.is_ignoring_response_headers(resp_header.get_status_code().as_u16())
        {
            return Ok(());
        }

        // check if response headers already sent, cannot send again
        if let Some(response_headers) = self.response_header.as_ref() {
            if !response_headers.metadata.status.is_informational() || self.upgrade {
                return Ok(());
            }
        }

        // check if response headers should be updated
        if !resp_header.metadata.status.is_informational() && self.update_response_headers {
            let timestamp = std::time::SystemTime::now();
            let http_date = httpdate::fmt_http_date(timestamp);
            resp_header.insert_header(http::header::DATE, http_date);

            let connection_value = if self.is_session_keepalive() {
                "keep-alive"
            } else {
                "close"
            };
            resp_header.insert_header(http::header::CONNECTION, connection_value);
        }

        if resp_header.metadata.status == 101 {
            // close connection when upgrade happens
            self.set_keepalive(None);
        }

        if resp_header.metadata.status == 101 || !resp_header.metadata.status.is_informational() {
            if self.is_session_upgrade(&resp_header) {
                self.upgrade = true;
            } else {
                self.body_reader.with_content_length_read(0, b"");
            }
            self.set_response_body_writer(&resp_header);
        };

        // check if we should flush
        let flush = resp_header.metadata.status.is_informational()
            || resp_header
                .get_header(http::header::CONTENT_LENGTH)
                .is_none();

        let mut resp_buf = resp_header.build_to_buffer();
        // TODO: add write timeouts here
        match self.stream.write_all(&mut resp_buf).await {
            Ok(()) => {
                if flush || self.body_writer.finished() {
                    self.stream.flush().await?;
                }
                self.response_header = Some(resp_header);
                self.buf_write_size += resp_buf.len();
                Ok(())
            }
            Err(e) => {
                let error = format!("error writing response header: {:>}", e);
                Err(tokio::io::Error::new(tokio::io::ErrorKind::Other, error))
            }
        }
    }

    /// write response body to downstream
    pub async fn write_response_body(&mut self, buffer: &[u8]) -> tokio::io::Result<Option<usize>> {
        // TODO: add write timeouts here
        let size_written = self.body_writer.write_body(&mut self.stream, buffer).await;

        if let Ok(Some(written)) = size_written {
            self.buf_write_size += written;
        }

        size_written
    }

    /// write response body with the write body vec buffer
    pub async fn vectored_write_response_body(&mut self) -> tokio::io::Result<Option<usize>> {
        if self.write_body_vec_buffer.is_empty() {
            return Ok(None);
        }

        // TODO: add timeouts here
        let size_written = self
            .body_writer
            .write_body(&mut self.stream, &self.write_body_vec_buffer)
            .await;

        if let Ok(Some(written)) = size_written {
            self.buf_write_size += written;
        }

        self.write_body_vec_buffer.clear();

        size_written
    }

    /// when write finished, this is executed
    /// used to flush the remaining data in buffer
    pub async fn finish_writing_response_body(&mut self) -> tokio::io::Result<Option<usize>> {
        let res = self.body_writer.finish(&mut self.stream).await?;
        self.stream.flush().await?;
        self.force_close_request_body_reader();
        Ok(res)
    }

    /// task handler for write response to downstream
    pub async fn write_response_to_downstream(&mut self, task: Task) -> tokio::io::Result<bool> {
        let end_stream = match task {
            Task::Header(resp_header, end_stream) => {
                self.write_response_headers(resp_header).await?;
                end_stream
            }
            Task::Body(resp_body, end_stream) => match resp_body {
                Some(body) => {
                    if !body.is_empty() {
                        self.write_response_body(&body).await?;
                    }
                    end_stream
                }
                None => end_stream,
            },
            Task::Trailer(_) => true,
            Task::Done => true,
            Task::Failed(e) => return Err(e),
        };
        // check if end of stream
        if end_stream {
            self.finish_writing_response_body().await?;
        }
        Ok(end_stream)
    }

    /// task handler for vectored write response to downstream
    pub async fn vectored_write_response_to_downstream(
        &mut self,
        tasks: Vec<Task>,
    ) -> tokio::io::Result<bool> {
        let mut end_stream = false;
        // proceed all tasks
        for task in tasks.into_iter() {
            end_stream = match task {
                Task::Header(resp_header, end_stream) => {
                    self.write_response_headers(resp_header).await?;
                    end_stream
                }
                Task::Body(resp_body, end_stream) => match resp_body {
                    Some(body) => {
                        // append bytes before write
                        if !body.is_empty() {
                            self.write_body_vec_buffer.put_slice(&body);
                        }
                        end_stream
                    }
                    None => end_stream,
                },
                Task::Trailer(_) => true,
                Task::Done => true,
                Task::Failed(e) => {
                    self.vectored_write_response_body().await?;
                    self.stream.flush().await?;
                    return Err(e);
                }
            }
        }
        // write from tasks
        self.vectored_write_response_body().await?;
        // check if end of the stream
        if end_stream {
            self.finish_writing_response_body().await?;
        }
        Ok(end_stream)
    }

    /// the associated function for writing response to downstream
    pub async fn write_downstream_response(
        &mut self,
        mut tasks: Vec<Task>,
    ) -> tokio::io::Result<bool> {
        match tasks.len() {
            0 => Ok(true), // quick return if no task
            1 => self.write_response_to_downstream(tasks.pop().unwrap()).await,
            _ => self.vectored_write_response_to_downstream(tasks).await,
        }
    }
}

/// implementation for extra utilities
/// helper function to work with downstream
impl Downstream {
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

    /// check if request is expect continue
    pub fn is_request_expect_continue(&self) -> bool {
        let req_header = self
            .request_header
            .as_ref()
            .expect("Request is not read yet");
        Utils::is_request_expect_continue(req_header)
    }

    /// check if we should ignore response headers
    pub fn is_ignoring_response_headers(&self, status_code: u16) -> bool {
        self.ignore_response_headers
            && status_code != 100
            && !(status_code == 100 && self.is_request_expect_continue())
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
    /// TODO: get actual values
    pub fn get_keepalive_value(&self) -> (Option<u64>, Option<usize>) {
        (None, None)
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
