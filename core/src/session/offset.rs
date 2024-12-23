use bytes::Bytes;

// the type for the buffer offset management
// a regular offset can be use anywhere
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Offset(pub usize, pub usize);

impl Offset {
    // new buffer Offset
    pub fn new(start: usize, len: usize) -> Self {
        println!("new offset ops > start: {}, end: {}, buf len: {}", start, start + len, len);
        Offset(start, start + len)
    }

    // return a sub-slice of the buffer based on offset
    pub fn get<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        println!("offset slice ops > start: {}, end: {}, buf len: {}", self.0, self.1, buf.len());
        &buf[self.0..self.1]
    }

    // return the buffer slice based on offset
    pub fn get_bytes(&self, buf: &Bytes) -> Bytes {
        buf.slice(self.0..self.1)
    }

    // return the size of the slice reference
    pub fn len(&self) -> usize {
        self.1 - self.0
    }

    // return true if the length is zero
    pub fn is_empty(&self) -> bool {
        self.1 == self.0
    }
}

// header buffer offset
// stores as key value offset
#[derive(Clone)]
pub struct KVOffset {
    key: Offset,
    value: Offset,
}

impl KVOffset {
    // new Key Value Offset
    pub fn new(key_start: usize, key_len: usize, value_start: usize, value_len: usize) -> Self {
        KVOffset {
            key: Offset(key_start, key_start + key_len),
            value: Offset(value_start, value_start + value_len),
        }
    }

    // return the name as raw bytes
    pub fn get_key<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        self.key.get(buf)
    }

    // return the value as raw bytes
    pub fn get_value<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        self.value.get(buf)
    }

    // return the name as sliced bytes
    pub fn get_key_bytes(&self, buf: &Bytes) -> Bytes {
        self.key.get_bytes(buf)
    }

    // return the value as sliced bytes
    pub fn get_value_bytes(&self, buf: &Bytes) -> Bytes {
        self.value.get_bytes(buf)
    }

    // return a reference to the value
    pub fn value(&self) -> &Offset {
        &self.value
    }
}
