use bytes::{BufMut, Bytes, BytesMut};
use http::header;
use httparse::{Request, Status};
use tokio::io::AsyncReadExt;
use url::form_urlencoded;

use crate::service::offset::{BufRef, KVRef};
use crate::stream::stream::Stream;

pub struct BufferSession {
    pub stream: Stream,
    pub buffer: Bytes,
    pub method: Option<BufRef>,
    pub path: Option<BufRef>,
    pub version: Option<String>,
    pub query_params: Vec<KVRef>,
    pub headers: Vec<KVRef>,
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
            headers: Vec::new(),
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
                    format!("request larger than {}", Self::MAX_HEADER_SIZE),
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
                    // Store method as BufRef
                    if let Some(m) = req.method {
                        let method_start = m.as_ptr() as usize - init_buffer.as_ptr() as usize;
                        self.method = Some(BufRef::new(method_start, m.len()));
                    }

                    // Store path and parse query parameters as BufRefs
                    if let Some(p) = req.path {
                        let path_start = p.as_ptr() as usize - init_buffer.as_ptr() as usize;
                        self.path = Some(BufRef::new(path_start, p.len()));

                        if let Some(query_start) = p.find('?') {
                            let query_str = &p[query_start + 1..];

                            // Parse query parameters maintaining zero-copy
                            let pairs = form_urlencoded::parse(query_str.as_bytes());
                            for (key, value) in pairs {
                                let key_start =
                                    key.as_ptr() as usize - init_buffer.as_ptr() as usize;
                                let value_start =
                                    value.as_ptr() as usize - init_buffer.as_ptr() as usize;

                                self.query_params.push(KVRef::new(
                                    key_start,
                                    key.len(),
                                    value_start,
                                    value.len(),
                                ));
                            }
                        }
                    }

                    // Store version
                    self.version = match req.version {
                        Some(0) => Some("HTTP/1.0".to_string()),
                        Some(1) => Some("HTTP/1.1".to_string()),
                        Some(2) => Some("HTTP/2.0".to_string()),
                        _ => Some("HTTP/0.9".to_string()),
                    };

                    // Store headers as KVRefs
                    for header in req.headers.iter() {
                        if !header.name.is_empty() {
                            let name_start =
                                header.name.as_ptr() as usize - init_buffer.as_ptr() as usize;
                            let value_start =
                                header.value.as_ptr() as usize - init_buffer.as_ptr() as usize;

                            self.headers.push(KVRef::new(
                                name_start,
                                header.name.len(),
                                value_start,
                                header.value.len(),
                            ));
                        }
                    }

                    // Store the final buffer
                    self.buffer = init_buffer.freeze();
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

    pub fn get_method(&self) -> Option<&[u8]> {
        self.method.as_ref().map(|m| m.get(&self.buffer))
    }

    pub fn get_method_str(&self) -> Option<&str> {
        self.get_method()
            .and_then(|m| std::str::from_utf8(m).ok())
    }

    pub fn get_path(&self) -> Option<&[u8]> {
        self.path.as_ref().map(|p| p.get(&self.buffer))
    }

    pub fn get_path_str(&self) -> Option<&str> {
        self.get_path()
            .and_then(|p| std::str::from_utf8(p).ok())
    }

    pub fn get_version(&self) -> Option<&str> {
        self.version.as_ref().map(|v| v.as_str())
    }

    pub fn get_query_param(&self, name: &[u8]) -> Option<&[u8]> {
        self.query_params
            .iter()
            .find(|kv| kv.get_name(&self.buffer) == name)
            .map(|kv| kv.get_value(&self.buffer))
    }

    pub fn get_query_param_str(&self, name: &str) -> Option<&str> {
        self.get_query_param(name.as_bytes())
            .and_then(|v| std::str::from_utf8(v).ok())
    }

    pub fn insert_query_param(&mut self, name: &[u8], value: &[u8]) {
        // Create a new buffer that includes the new data
        let mut new_buffer = BytesMut::with_capacity(self.buffer.len() + name.len() + value.len());
        new_buffer.extend_from_slice(&self.buffer);
        
        // Add new data at the end and get their offsets
        let name_start = new_buffer.len();
        new_buffer.extend_from_slice(name);
        
        let value_start = new_buffer.len();
        new_buffer.extend_from_slice(value);

        // Create new KVRef and add to query_params
        self.query_params.push(KVRef::new(
            name_start,
            name.len(),
            value_start,
            value.len(),
        ));

        // Update the buffer
        self.buffer = new_buffer.freeze();
    }

    pub fn remove_query_param(&mut self, name: &[u8]) -> bool {
        if let Some(index) = self.query_params
            .iter()
            .position(|kv| kv.get_name(&self.buffer) == name) 
        {
            self.query_params.remove(index);
            true
        } else {
            false
        }
    }

    // Headers Operations
    pub fn get_header(&self, name: &[u8]) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|kv| kv.get_name(&self.buffer) == name)
            .map(|kv| kv.get_value(&self.buffer))
    }

    pub fn get_header_str(&self, name: &str) -> Option<&str> {
        self.get_header(name.as_bytes())
            .and_then(|v| std::str::from_utf8(v).ok())
    }

    pub fn insert_header(&mut self, name: &[u8], value: &[u8]) {
        // Remove existing header if it exists
        self.remove_header(name);

        // Create a new buffer that includes the new data
        let mut new_buffer = BytesMut::with_capacity(self.buffer.len() + name.len() + value.len());
        new_buffer.extend_from_slice(&self.buffer);
        
        // Add new data at the end and get their offsets
        let name_start = new_buffer.len();
        new_buffer.extend_from_slice(name);
        
        let value_start = new_buffer.len();
        new_buffer.extend_from_slice(value);

        // Create new KVRef and add to headers
        self.headers.push(KVRef::new(
            name_start,
            name.len(),
            value_start,
            value.len(),
        ));

        // Update the buffer
        self.buffer = new_buffer.freeze();
    }

    pub fn remove_header(&mut self, name: &[u8]) -> bool {
        if let Some(index) = self.headers
            .iter()
            .position(|kv| kv.get_name(&self.buffer) == name) 
        {
            self.headers.remove(index);
            true
        } else {
            false
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
