use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use bytes::{BufMut, BytesMut, Bytes};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::stream::stream::Stream;

#[derive(Debug)]
pub struct Headers {
    headers: HashMap<String, String>,
}

impl Headers {
    pub fn new() -> Self {
        Headers {
            headers: HashMap::new()
        }
    }

    #[inline]
    pub fn get(&self, name: &str) -> Option<&String> {
        self.headers.get(&name.to_lowercase())
    }
    
    #[inline]
    pub fn set(&mut self, name: String, value: String) {
        self.headers.insert(name.to_lowercase(), value);
    }
    
    #[inline]
    pub fn remove(&mut self, name: &str) -> Option<String> {
        self.headers.remove(&name.to_lowercase())
    }
}

pub struct SessionBuffer {
    stream: Stream,
    // Single buffer for both reading and writing
    buffer: BytesMut,
}

impl SessionBuffer {
    pub fn new(stream: Stream) -> Self {
        Self {
            stream,
            // Larger initial capacity for efficient header and body handling
            buffer: BytesMut::with_capacity(64 * 1024), // 64KB
        }
    }

    // Optimized header reading
    pub async fn read_headers(&mut self) -> Result<Headers, Box<dyn Error>> {
        let mut headers = Headers::new();
        let mut header_end_pos = None;
        
        loop {
            // Check if we already have the complete headers in buffer
            if let Some(pos) = self.find_headers_end() {
                header_end_pos = Some(pos);
                break;
            }

            // Read more data if needed
            let bytes_read = self.stream.read_buf(&mut self.buffer).await?;
            if bytes_read == 0 {
                return Err("Unexpected EOF while reading headers".into());
            }
        }

        // Process headers
        let header_end = header_end_pos.unwrap();
        let header_data = self.buffer.split_to(header_end + 4); // +4 for \r\n\r\n
        let header_str = String::from_utf8_lossy(&header_data);

        // Parse headers efficiently
        for line in header_str.lines() {
            if line.is_empty() || line == "\r" {
                continue;
            }

            if let Some((name, value)) = line.split_once(':') {
                headers.set(
                    name.trim().to_owned(),
                    value.trim().to_owned()
                );
            }
        }

        Ok(headers)
    }

    // Helper method to find the end of headers
    #[inline]
    fn find_headers_end(&self) -> Option<usize> {
        let mut pos = 0;
        while pos + 4 <= self.buffer.len() {
            if &self.buffer[pos..pos + 4] == b"\r\n\r\n" {
                return Some(pos);
            }
            pos += 1;
        }
        None
    }

    // Optimized header writing
    pub async fn write_headers(&mut self, headers: &Headers) -> Result<(), Box<dyn Error>> {
        // Pre-calculate capacity to avoid reallocations
        let mut estimated_size = 0;
        for (name, value) in &headers.headers {
            estimated_size += name.len() + value.len() + 4; // 4 for ": " and "\r\n"
        }
        estimated_size += 2; // Final "\r\n"

        // Ensure we have enough capacity
        if self.buffer.capacity() - self.buffer.len() < estimated_size {
            self.buffer.reserve(estimated_size);
        }

        // Write headers efficiently
        for (name, value) in &headers.headers {
            self.buffer.put_slice(name.as_bytes());
            self.buffer.put_slice(b": ");
            self.buffer.put_slice(value.as_bytes());
            self.buffer.put_slice(b"\r\n");
        }
        self.buffer.put_slice(b"\r\n");

        // Write to stream
        self.stream.write_all(&self.buffer).await?;
        self.buffer.clear();
        Ok(())
    }

    // Optimized body reading with zero-copy
    pub async fn read_body(&mut self) -> Result<Option<Arc<Bytes>>, String> {
        // First use any data remaining in buffer from header reading
        if !self.buffer.is_empty() {
            let data = self.buffer.split().freeze();
            return Ok(Some(Arc::new(data)));
        }

        // Read new data if buffer is empty
        let bytes_read = match self.stream.read_buf(&mut self.buffer).await {
            Ok(bytes) => bytes,
            Err(e) => return Err(e.to_string()),
        };
        if bytes_read == 0 {
            return Ok(None); // EOF
        }

        let data = self.buffer.split().freeze();
        Ok(Some(Arc::new(data)))
    }

    // Optimized body writing with zero-copy
    pub async fn write_body(&mut self, data: Arc<Bytes>) -> Result<(), String> {
        if let Err(e) = self.stream.write_all(&data).await {
            return Err(e.to_string())
        }
        Ok(())
    }

    // Flush any remaining data
    pub async fn flush(&mut self) -> Result<(), String> {
        if !self.buffer.is_empty() {
            if let Err(e) = self.stream.write_all(&self.buffer).await {
                return Err(e.to_string())
            }
            self.buffer.clear();
        }
        if let Err(e) = self.stream.flush().await {
            return Err(e.to_string())
        }
        Ok(())
    }

    // pop out stream from session buffer
    pub async fn extract_stream(self) -> Stream {
        self.stream
    }
}
