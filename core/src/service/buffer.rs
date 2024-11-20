use bytes::{Bytes, BytesMut};
use httparse::{Request, Status};
use nix::NixPath;
use std::ascii::AsciiExt;
use std::collections::HashMap;
use tokio::io::AsyncReadExt;
use url::form_urlencoded;

use crate::service::offset::Offset;
use crate::stream::stream::Stream;

pub struct SessionHeader {
    pub raw_headers: BytesMut,
    pub header_value: HashMap<Vec<u8>, Vec<Offset>>,
    pub header_case: HashMap<Vec<u8>, Vec<Vec<u8>>>,
}

impl SessionHeader {
    // constant used for header formatter.
    const CRLF: &[u8; 2] = b"\r\n";
    const DOUBLE_CRLF: &[u8; 4] = b"\r\n\r\n";
    const HEADER_DELIMITER: &[u8; 2] = b": ";
    const EMPTY_SPACE: &[u8; 1] = b" ";

    pub fn new() -> Self {
        SessionHeader {
            raw_headers: BytesMut::new(),
            header_value: HashMap::new(),
            header_case: HashMap::new(),
        }
    }

    pub fn init_buffer(&mut self, headers: BytesMut) {
        self.raw_headers = headers;
        println!("headers buffer: {:?}", self.raw_headers);
    }

    // initialize header from stream
    pub fn init_header(&mut self, name: &[u8], value: &[u8], base: usize) {
        let header_start = value.as_ptr() as usize - base;
        let header_end = header_start + value.len();
        let header_offset = Offset::new(header_start, header_end);

        let header_key = name.to_ascii_lowercase();

        // Store header value
        self.header_value
            .entry(header_key.clone())
            .or_insert_with(Vec::new)
            .push(header_offset);

        // Store original header case
        self.header_case
            .entry(header_key)
            .or_insert_with(Vec::new)
            .push(name.to_vec());
    }

    // append header
    pub fn append_header(&mut self, name: &[u8], value: &[u8]) {
        let raw_end = self.raw_headers.len();
        let header_size = value.len();
        let header_offset = Offset::new(raw_end, raw_end + header_size);

        let header_key = name.to_ascii_lowercase();

        // Store header value
        self.header_value
            .entry(header_key.clone())
            .or_insert_with(Vec::new)
            .push(header_offset.clone());

        // Store original header case
        self.header_case
            .entry(header_key)
            .or_insert_with(Vec::new)
            .push(name.to_vec());

        self.raw_headers.extend_from_slice(value);
    }

    pub fn get_raw_headers(&self, name: &[u8]) -> Option<Vec<&[u8]>> {
        let lowercase_name = name.to_ascii_lowercase();
        self.header_value
            .get(&lowercase_name)
            .map(|refs| refs.iter().map(|r| r.get(&self.raw_headers)).collect())
    }

    pub fn get_headers(&self, name: &str) -> Option<Vec<&str>> {
        self.get_raw_headers(name.as_bytes()).map(|values| {
            values
                .iter()
                .filter_map(|v| std::str::from_utf8(v).ok())
                .collect()
        })
    }

    pub fn get_raw_header(&self, name: &[u8]) -> Option<&[u8]> {
        let lowercase_name = name.to_ascii_lowercase();
        self.header_value
            .get(&lowercase_name)
            .and_then(|refs| refs.first().map(|r| r.get(&self.raw_headers)))
    }

    pub fn get_header(&self, name: &str) -> Option<&str> {
        self.get_raw_header(name.as_bytes())
            .and_then(|v| std::str::from_utf8(v).ok())
    }

    // remove header
    pub fn remove_header(&mut self, name: &[u8]) {
        let header_key = name.to_ascii_lowercase();
        self.header_value.remove(&header_key);
        self.header_case.remove(&header_key);
    }

    // wire structured header to buffers
    pub fn build(&mut self) -> BytesMut {
        let mut header_buffer = BytesMut::with_capacity(0);

        for (header_key, header_names) in &self.header_case {
            if let Some(values) = self.header_value.get(header_key) {
                for (index, header_value_offset) in values.iter().enumerate() {
                    let header_name = &header_names[index];
                    let header_value = header_value_offset.get(&self.raw_headers);
                    header_buffer.extend_from_slice(header_name);
                    header_buffer.extend_from_slice(Self::HEADER_DELIMITER);
                    header_buffer.extend_from_slice(header_value);
                    header_buffer.extend_from_slice(Self::CRLF);
                }
            }
        }
        header_buffer
    }
}

// buffer session is used to work with stream buffer
//
// the socket stream is parsed into an alocated buffer
// the session is responsible to manage the socket stream buffer
//
// the buffer is designed for zero-copy mechanism
// mostly storing buffer offset, instead of copying data from buffer
// offset is used to slice a reference to the actual buffer
pub struct BufferSession {
    // [stream] is used to store the established socket stream
    pub stream: Stream,
    // [buffer] is used as the socket
    // read and write operation involves the buffer itself
    pub buffer: Bytes,
    // [raw_header_offset] & [raw_body_offset] tracks offset
    // stores raw buffer offset for appending values with offset
    pub raw_header_offset: Option<Offset>,
    pub raw_body_offset: Option<Offset>,
    // new headers
    pub headers: SessionHeader,
}

impl BufferSession {
    // these size is only used during session initialization
    //
    // [INIT_BUFFER_SIZE] this prevents to use extra memory to parse larger request.
    // [MAX_HEADER_SIZE] this prevents a larger request being parsed to session
    const INIT_BUFFER_SIZE: usize = 1024;
    const MAX_HEADER_SIZE: usize = 8192;

    // MAX_HEADERS_COUNT is a sized of maximum headers to be parsed
    const MAX_HEADERS_COUNT: usize = 256;

    // session initialization
    // pass in the socket stream into session
    //
    // NOTE: we need to call [Self::read_stream] to parse stream to buffer
    // otherwise the session is unusable.
    pub fn new(stream: Stream) -> Self {
        BufferSession {
            stream,
            buffer: Bytes::new(),
            raw_header_offset: None,
            raw_body_offset: None,
            headers: SessionHeader::new(),
        }
    }

    // the function that is responsible to parse socket stream to buffer
    // after callling this, the session will be usable.
    pub async fn read_stream(&mut self) -> tokio::io::Result<Option<usize>> {
        // initialize buffer
        // small alocation for read purposes
        let mut init_buffer = BytesMut::with_capacity(Self::INIT_BUFFER_SIZE);
        // tracks the size of the entire already-read buffer from socket stream
        let mut read_buf_size: usize = 0;

        loop {
            // throw error if request is larger than expected.
            if read_buf_size > Self::MAX_HEADER_SIZE {
                return Err(tokio::io::Error::new(
                    tokio::io::ErrorKind::Other,
                    format!("request larger than {}", Self::MAX_HEADER_SIZE),
                ));
            }

            // read socket stream to buffer
            let len = match self.stream.read_buf(&mut init_buffer).await {
                Ok(0) if read_buf_size > 0 => {
                    return Err(tokio::io::Error::new(
                        tokio::io::ErrorKind::Other,
                        "connection closed",
                    ));
                }
                Ok(0) => return Ok(None),
                Ok(n) => n,
                Err(e) => return Err(e),
            };

            println!("raw socket request: {:?}", init_buffer);

            read_buf_size += len;

            // build a new request header parser for parsing session headers
            let mut headers = [httparse::EMPTY_HEADER; Self::MAX_HEADERS_COUNT];
            let mut req = Request::new(&mut headers);

            // parse request headers
            match req.parse(&init_buffer) {
                // when parsing headers completed
                // sents [s] as the end of raw headers offset
                Ok(Status::Complete(s)) => {
                    // get raw offset
                    let raw_header_offset = Offset::new(0, s);
                    let raw_body_offset = Offset::new(s, read_buf_size);
                    // store raw offset
                    self.raw_header_offset = Some(raw_header_offset);
                    self.raw_body_offset = Some(raw_body_offset);

                    // base pointer of the initialized buffer
                    let base = init_buffer.as_ptr() as usize;
                    println!("base: {:?}", base);
                    println!("already read: {:?}", read_buf_size);
                    println!("headers end: {:?}", s);

                    for header in req.headers.iter() {
                        if !header.name.is_empty() {
                            self.headers
                                .init_header(header.name.as_bytes(), header.value, base);
                        }
                    }

                    // place the header buffer seperately
                    let headers_buffer = init_buffer.split_to(s);
                    self.headers.init_buffer(headers_buffer);

                    return Ok(Some(s));
                }
                // the partial is reached when request is not fully parsed
                // continue the loop to parse more request.
                Ok(Status::Partial) => continue,
                // error during request parse
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
