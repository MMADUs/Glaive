use bytes::{Bytes, BytesMut};

#[derive(Debug)]
pub struct StorageBuffer {
    buffer: BytesMut,
    capacity: usize,
    truncated: bool,
}

impl StorageBuffer {
    pub fn new(size: usize) -> Self {
        StorageBuffer {
            buffer: BytesMut::new(),
            capacity: size,
            truncated: false,
        }
    }

    /// write to buffer
    pub fn write_buffer(&mut self, bytes: &Bytes) {
        if !self.truncated && (self.buffer.len() + bytes.len() <= self.capacity) {
            self.buffer.extend_from_slice(bytes);
        } else {
            self.truncated = true;
        }
    }

    /// clear buffer
    pub fn clear_buffer(&mut self) {
        self.truncated = false;
        self.buffer.clear();
    }

    /// check if buffer is empty
    pub fn is_buffer_empty(&self) -> bool {
        self.buffer.len() == 0
    }

    /// check if buffer is truncated
    pub fn is_buffer_truncated(&self) -> bool {
        self.truncated
    }

    /// get buffer from storage
    pub fn get_buffer(&self) -> Option<Bytes> {
        if !self.is_buffer_empty() {
            let buffer = self.buffer.clone().freeze();
            Some(buffer)
        } else {
            None
        }
    }
}
