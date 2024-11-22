use crate::stream::stream::Stream;
use bytes::{Bytes, BytesMut};
use httparse::{Request, Status};
use http::Version;
use tokio::io::AsyncReadExt;

use super::offset::{KVOffset, Offset};
use super::reader::BodyReader;
use super::request::RequestHeader;
use super::writer::BodyWriter;

const INIT_BUFFER_SIZE: usize = 1024;
const MAX_BUFFER_SIZE: usize = 8192;
const MAX_HEADERS_COUNT: usize = 256;

pub struct Downstream {
    pub stream: Stream,
    pub buffer: Bytes,
    pub buf_headers_offset: Option<Offset>,
    pub buf_body_offset: Option<Offset>,
    pub headers: Option<RequestHeader>,
    pub body_writer: BodyWriter,
    pub body_reader: BodyReader,
}

impl Downstream {
    pub fn new(s: Stream) -> Self {
        Downstream {
            stream: s,
            buffer: Bytes::new(),
            buf_headers_offset: None,
            buf_body_offset: None,
            headers: None,
            body_writer: BodyWriter::new(),
            body_reader: BodyReader::new(),
        }
    }

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
                        let header_value = unsafe {
                            http::HeaderValue::from_maybe_shared_unchecked(header_value)
                        };
                        request_header.append_header(header_name, header_value);
                    }

                    self.buffer = buffer_bytes;
                    self.headers = Some(request_header);

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
