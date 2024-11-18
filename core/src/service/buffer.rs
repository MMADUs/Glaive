use bytes::{BufMut, Bytes, BytesMut};
use http::header;
use httparse::{Request, Status};
use std::collections::HashMap;
use tokio::io::AsyncReadExt;
use url::form_urlencoded;

use crate::service::offset::KVRef;
use crate::stream::stream::Stream;

pub struct BufferSession {
    pub stream: Stream,
    pub buffer: Bytes,
    pub method: Option<String>,
    pub path: Option<Bytes>,  // Now using Bytes instead of String
    pub version: Option<String>,
    pub query_params: Vec<(Bytes, Bytes)>,  // Zero-copy query params as Bytes pairs
    pub headers: HashMap<Bytes, Bytes>,
}

impl BufferSession {
    const INIT_BUFFER_SIZE: usize = 1024;
    const MAX_HEADER_SIZE: usize = 8192;
    const MAX_HEADERS_COUNT: usize = 256;

    pub fn new(stream: Stream) -> Self {
        BufferSession {
            stream,
            buffer: Bytes::new(),
            method: None,
            path: None,
            version: None,
            query_params: Vec::new(),
            headers: HashMap::new(),
        }
    }

    // Parse the HTTP request from the stream buffer
    pub async fn read_stream(&mut self) -> tokio::io::Result<Option<usize>> {
        let mut init_buffer = BytesMut::with_capacity(Self::INIT_BUFFER_SIZE);
        let mut already_read: usize = 0;

        loop {
            if already_read > Self::MAX_HEADER_SIZE {
                return Err(tokio::io::Error::new(
                    tokio::io::ErrorKind::Other,
                    "request larger than {Self::MAX_HEADER_SIZE}",
                ));
            }

            let len = match self.stream.read_buf(&mut init_buffer).await {
                Ok(0) if already_read > 0 => {
                    return Err(tokio::io::Error::new(
                        tokio::io::ErrorKind::Other,
                        "connection closed",
                    ));
                }
                Ok(0) => return Ok(None),
                Ok(n) => n,
                Err(e) => return Err(e),
            };

            already_read += len;

            let mut headers = [httparse::EMPTY_HEADER; Self::MAX_HEADERS_COUNT];
            let mut req = Request::new(&mut headers);

            match req.parse(&init_buffer) {
                Ok(Status::Complete(s)) => {
                    // Zero-copy parsing
                    let base = init_buffer.as_ptr() as usize;
                    let mut header_refs = Vec::<KVRef>::with_capacity(req.headers.len());

                    for header in req.headers.iter() {
                        if !header.name.is_empty() {
                            header_refs.push(KVRef::new(
                                header.name.as_ptr() as usize - base,
                                header.name.as_bytes().len(),
                                header.value.as_ptr() as usize - base,
                                header.value.len(),
                            ));
                        }
                    }

                    // Version parsing with explicit HTTP version support
                    self.version = match req.version {
                        Some(0) => Some("HTTP/1.0".to_string()),
                        Some(1) => Some("HTTP/1.1".to_string()),
                        Some(2) => Some("HTTP/2.0".to_string()),
                        _ => Some("HTTP/0.9".to_string()),
                    };

                    // Method parsing
                    self.method = req.method.map(|m| m.to_uppercase());

                    // Path parsing with query parameter extraction
                    if let Some(p) = req.path {
                        let path_start = p.as_ptr() as usize - base;
                        let path_len = p.len();
                        self.path = Some(init_buffer.slice(path_start..(path_start + path_len)));

                        // Query parameter parsing as zero-copy pairs
                        if let Some(query_str) = p.split('?').nth(1) {
                            for (key, value) in form_urlencoded::parse(query_str.as_bytes()) {
                                self.query_params.push((
                                    Bytes::copy_from_slice(key.as_bytes()),
                                    Bytes::copy_from_slice(value.as_bytes()),
                                ));
                            }
                        }
                    }

                    let freezed_buf = init_buffer.freeze();

                    for header in header_refs {
                        let header_name = header.get_name_bytes(&freezed_buf);
                        let header_value = header.get_value_bytes(&freezed_buf);
                        self.headers.insert(header_name, header_value);
                    }

                    // Freeze buffer to prevent modifications
                    self.buffer = freezed_buf;
                    return Ok(Some(s));
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

    // // Insert or update a header by key
    // pub fn insert_header(&mut self, key: &str, value: &str) {
    //     self.headers.insert(key.to_string(), value.to_string());
    // }
    //
    // // Remove a header by key
    // pub fn remove_header(&mut self, key: &str) {
    //     self.headers.remove(key);
    // }
    //
    // // Parse form parameters (application/x-www-form-urlencoded)
    // pub fn parse_request_params(&mut self, body: &[u8]) {
    //     let form_params = form_urlencoded::parse(body)
    //         .into_owned()
    //         .collect::<HashMap<String, String>>();
    //
    //     self.request_params = form_params;
    // }
    //
    // // Serialize the request back into a buffer
    // pub fn to_buffer(&self) -> BytesMut {
    //     let mut buffer = BytesMut::with_capacity(self.buffer.len());
    //
    //     // Rebuild the request line
    //     if let Some(method) = &self.method {
    //         buffer.put_slice(method.as_bytes());
    //         buffer.put_slice(b" ");
    //     }
    //
    //     if let Some(path) = &self.path {
    //         buffer.put_slice(path.as_bytes());
    //     }
    //
    //     if let Some(version) = &self.version {
    //         buffer.put_slice(b" ");
    //         buffer.put_slice(version.as_bytes());
    //         buffer.put_slice(b"\r\n");
    //     }
    //
    //     // Rebuild the headers
    //     for (key, value) in &self.headers {
    //         buffer.put_slice(key.as_bytes());
    //         buffer.put_slice(b": ");
    //         buffer.put_slice(value.as_bytes());
    //         buffer.put_slice(b"\r\n");
    //     }
    //
    //     // Add a blank line after headers
    //     buffer.put_slice(b"\r\n");
    //
    //     // Optionally, add request body (form parameters)
    //     if !self.request_params.is_empty() {
    //         let body = form_urlencoded::Serializer::new(String::new())
    //             .extend_pairs(self.request_params.iter())
    //             .finish();
    //         buffer.put_slice(body.as_bytes());
    //     }
    //
    //     buffer
    // }
}

