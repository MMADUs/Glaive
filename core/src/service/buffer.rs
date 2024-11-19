use bytes::{Bytes, BytesMut};
use httparse::{Request, Status};
use std::collections::HashMap;
use tokio::io::AsyncReadExt;
use url::form_urlencoded;

use crate::service::offset::Offset;
use crate::stream::stream::Stream;

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
    // [method] stores request method
    pub method: Option<Offset>,
    // [path] stores the request uri path
    pub path: Option<Offset>,
    // [version] stores the request version
    // used to determine which request this belongs to.
    pub version: Option<String>,
    // [query_params] stores the request query parameters.
    pub query_params: HashMap<Vec<u8>, Offset>,
    // request headers.
    //
    // using [headers] to store the headers value
    // the key of both hashmap is the lowered case header name
    // [headers] makes request header to manage multiple values within the same name
    pub headers: HashMap<Vec<u8>, Vec<Offset>>,
    // using [header_cases] to store the headers name
    // used to preserve the header name within the same header name
    // using this to manage multiple variant of the same header name with just the key itself
    pub header_cases: HashMap<Vec<u8>, Vec<Vec<u8>>>,
}

impl BufferSession {
    // INIT_BUFFER_SIZE is used for allocating buffer reader
    //
    // the size is only used during session initialization
    // this prevents to use extra memory to parse larger request.
    const INIT_BUFFER_SIZE: usize = 1024;

    // MAX_HEADER_SIZE is used to limit the maximum request bytes
    //
    // the size is only used during session initialization
    // this prevents a larger request being parsed to session
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
            method: None,
            path: None,
            version: None,
            query_params: HashMap::new(),
            headers: HashMap::new(),
            header_cases: HashMap::new(),
        }
    }

    // helper function to convert header name to lowercase
    // this is used to generate a key for the headers
    fn to_lowercase(name: &[u8]) -> Vec<u8> {
        name.iter().map(|b| b.to_ascii_lowercase()).collect()
    }

    // the function that is responsible to parse socket stream to buffer
    // after callling this, the session will be usable.
    pub async fn read_stream(&mut self) -> tokio::io::Result<Option<usize>> {
        // initialize buffer
        // small alocation for read purposes
        let mut init_buffer = BytesMut::with_capacity(Self::INIT_BUFFER_SIZE);
        let mut already_read: usize = 0;

        loop {
            // throw error if request is larger than expected.
            if already_read > Self::MAX_HEADER_SIZE {
                return Err(tokio::io::Error::new(
                    tokio::io::ErrorKind::Other,
                    format!("request larger than {}", Self::MAX_HEADER_SIZE),
                ));
            }

            // read socket stream to buffer
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

            // build a new request session for parsing
            let mut headers = [httparse::EMPTY_HEADER; Self::MAX_HEADERS_COUNT];
            let mut req = Request::new(&mut headers);

            // parse request session
            match req.parse(&init_buffer) {
                // when parsing request is completed.
                Ok(Status::Complete(s)) => {
                    // store session method as offset
                    if let Some(m) = req.method {
                        let method_start = m.as_ptr() as usize - init_buffer.as_ptr() as usize;
                        let method_end = m.len();
                        let method_offset = Offset::new(method_start, method_end);
                        self.method = Some(method_offset);
                    }

                    // store uri path and parse query parameters as offset
                    if let Some(p) = req.path {
                        // path offset
                        let path_start = p.as_ptr() as usize - init_buffer.as_ptr() as usize;
                        let path_end = p.len();
                        let path_offset = Offset::new(path_start, path_end);
                        self.path = Some(path_offset);

                        if let Some(query_start) = p.find('?') {
                            let query_str = &p[query_start + 1..];
                            let pairs = form_urlencoded::parse(query_str.as_bytes());

                            for (key, value) in pairs {
                                // param offset
                                let value_start =
                                    value.as_ptr() as usize - init_buffer.as_ptr() as usize;
                                let value_end = value.len();
                                let value_offset = Offset::new(value_start, value_end);
                                // get param key
                                let param_key = key.as_bytes().to_vec();
                                self.query_params.insert(param_key, value_offset);
                            }
                        }
                    }

                    // store version as string
                    // version is only used as request identifier anyway.
                    self.version = match req.version {
                        Some(0) => Some("HTTP/1.0".to_string()),
                        Some(1) => Some("HTTP/1.1".to_string()),
                        Some(2) => Some("HTTP/2.0".to_string()),
                        _ => Some("HTTP/0.9".to_string()),
                    };

                    // store headers as the specificied rules
                    for header in req.headers.iter() {
                        if !header.name.is_empty() {
                            // get each header offset
                            let header_start =
                                header.value.as_ptr() as usize - init_buffer.as_ptr() as usize;
                            let header_end = header.value.len();
                            let header_offset = Offset::new(header_start, header_end);

                            // get header key
                            let lowercase_key = Self::to_lowercase(header.name.as_bytes());

                            // store header value as buffer offset
                            self.headers
                                .entry(lowercase_key.clone())
                                .or_insert_with(Vec::new)
                                .push(header_offset);

                            // store original header name in cases
                            self.header_cases
                                .entry(lowercase_key)
                                .or_insert_with(Vec::new)
                                .push(header.name.as_bytes().to_vec());
                        }
                    }

                    // freeze the session buffer
                    // the [freeze] is used to turn [BytesMut] into [Bytes]
                    // after freeze, buffer will be unmodified.
                    self.buffer = init_buffer.freeze();
                    // return the parsed size.
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

    // function used to get the request method
    //
    // [get_raw_method] returns the raw method as bytes
    // [get_method] returns the method as &str
    pub fn get_raw_method(&self) -> Option<&[u8]> {
        self.method.as_ref().map(|m| m.get(&self.buffer))
    }

    pub fn get_method(&self) -> Option<&str> {
        self.get_raw_method()
            .and_then(|m| std::str::from_utf8(m).ok())
    }

    // function used to get the request uri path
    //
    // [get_raw_path] returns the raw path as bytes
    // [get_path] returns the path as &str
    pub fn get_raw_path(&self) -> Option<&[u8]> {
        self.path.as_ref().map(|p| p.get(&self.buffer))
    }

    pub fn get_path(&self) -> Option<&str> {
        self.get_raw_path()
            .and_then(|p| std::str::from_utf8(p).ok())
    }

    // function used to get the request version
    // mainly used as identifier
    pub fn get_version(&self) -> Option<&str> {
        self.version.as_ref().map(|v| v.as_str())
    }

    // function used to get the request query parameters
    //
    // [get_raw_query_param] returns the raw query parameters as bytes
    // [get_query_param] returns the query parameters as &str
    pub fn get_raw_query_param(&self, name: &[u8]) -> Option<&[u8]> {
        self.query_params.get(name).map(|v| v.get(&self.buffer))
    }

    pub fn get_query_param(&self, name: &str) -> Option<&str> {
        self.get_raw_query_param(name.as_bytes())
            .and_then(|v| std::str::from_utf8(v).ok())
    }

    // function used to insert new query parameter to the session
    //
    // this creates new buffer with more allocation
    // operation is not zero-copy at all.
    // TODO: operation is not optimized, since it creates new buffer every time.
    pub fn insert_query_param(&mut self, name: &[u8], value: &[u8]) {
        // create new buffer with extra allocation
        // we allocate based on the new value bytes len
        let alloc = self.buffer.len() + value.len();
        let mut new_buffer = BytesMut::with_capacity(alloc);

        // append new buffer with more allocated for the new value
        new_buffer.extend_from_slice(&self.buffer);

        // append the new value to the new buffer
        new_buffer.extend_from_slice(value);

        // get the offset and store it
        let param_start = new_buffer.len();
        let param_end = value.len();
        let param_offset = Offset::new(param_start, param_end);
        self.query_params.insert(name.to_vec(), param_offset);

        // freeze the newly created buffer
        self.buffer = new_buffer.freeze();
    }

    // function used to remove query parameter
    // TODO: also remove it from the buffer.
    pub fn remove_query_param(&mut self, name: &[u8]) -> bool {
        self.query_params.remove(name).is_some()
    }

    // function used to get all the headers
    // since a single header name can have multiple diffrent values
    //
    // [get_raw_headers] returns all the raw headers as bytes
    // [get_headers] returns all the headers as &str
    pub fn get_raw_headers(&self, name: &[u8]) -> Option<Vec<&[u8]>> {
        let lowercase_name = Self::to_lowercase(name);
        self.headers
            .get(&lowercase_name)
            .map(|refs| refs.iter().map(|r| r.get(&self.buffer)).collect())
    }

    pub fn get_headers(&self, name: &str) -> Option<Vec<&str>> {
        self.get_raw_headers(name.as_bytes()).map(|values| {
            values
                .iter()
                .filter_map(|v| std::str::from_utf8(v).ok())
                .collect()
        })
    }

    // function used to get the request header
    // since a single header name can have multiple diffrent values
    // this only returs the first value in the value list
    //
    // [get_raw_header] returns the raw header as bytes
    // [get_header] returns the header as &str
    pub fn get_raw_header(&self, name: &[u8]) -> Option<&[u8]> {
        let lowercase_name = Self::to_lowercase(name);
        self.headers
            .get(&lowercase_name)
            .and_then(|refs| refs.first().map(|r| r.get(&self.buffer)))
    }

    pub fn get_header(&self, name: &str) -> Option<&str> {
        self.get_raw_header(name.as_bytes())
            .and_then(|v| std::str::from_utf8(v).ok())
    }

    // function used to get the original header case name
    // this is probably occasionally used.
    pub fn get_original_cases(&self, name: &[u8]) -> Option<&Vec<Vec<u8>>> {
        let lowercase_name = Self::to_lowercase(name);
        self.header_cases.get(&lowercase_name)
    }

    // function used to append new header to the session
    //
    // this creates new buffer with more allocation
    // operation is not zero-copy at all.
    // TODO: operation is not optimized, since it creates new buffer every time.
    pub fn append_header(&mut self, name: &[u8], value: &[u8]) {
        // create new buffer with extra allocation
        // we allocate based on the new value bytes len
        let alloc = self.buffer.len() + value.len();
        let mut new_buffer = BytesMut::with_capacity(alloc);

        // append new buffer with more allocated for the new value
        new_buffer.extend_from_slice(&self.buffer);

        // append the new value to the new buffer
        new_buffer.extend_from_slice(value);

        // get the offset and store it
        let header_start = new_buffer.len();
        let header_end = value.len();
        let value_ref = Offset::new(header_start, header_end);

        // get header key
        let header_key = Self::to_lowercase(name);

        // store header value
        self.headers
            .entry(header_key.clone())
            .or_insert_with(Vec::new)
            .push(value_ref);

        // store original header case name
        self.header_cases
            .entry(header_key)
            .or_insert_with(Vec::new)
            .push(name.to_vec());

        // freeze the newly created buffer
        self.buffer = new_buffer.freeze();
    }

    // function insert is similiar to append
    // but insert completely overwrite existing headers
    pub fn insert_header(&mut self, name: &[u8], value: &[u8]) {
        self.remove_header(name);
        self.append_header(name, value);
    }

    // function used to remove header
    // TODO: also remove it from the buffer.
    pub fn remove_header(&mut self, name: &[u8]) -> bool {
        // get header key
        let header_key = Self::to_lowercase(name);
        // remove header
        self.header_cases.remove(&header_key).is_some()
            && self.headers.remove(&header_key).is_some()
    }

    // function used to retrive the entire headers
    //
    // headers is served as raw bytes
    // returns the header original case name and the value
    pub fn from_raw_headers(&self) -> Vec<(Vec<u8>, Vec<&[u8]>)> {
        self.headers
            .iter()
            .map(|(name, refs)| {
                let values = refs.iter().map(|r| r.get(&self.buffer)).collect();
                (name.clone(), values)
            })
            .collect()
    }
}
