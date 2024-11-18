use bytes::Bytes;

/// A `BufRef` is a reference to a buffer of bytes. It removes the need for self-referential data
/// structures. It is safe to use as long as the underlying buffer does not get mutated.
///
/// # Panics
///
/// This will panic if an index is out of bounds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BufRef(pub usize, pub usize);

impl BufRef {
    /// Return a sub-slice of `buf`.
    pub fn get<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        &buf[self.0..self.1]
    }

    /// Return a slice of `buf`. This operation is O(1) and increases the reference count of `buf`.
    pub fn get_bytes(&self, buf: &Bytes) -> Bytes {
        buf.slice(self.0..self.1)
    }

    /// Return the size of the slice reference.
    pub fn len(&self) -> usize {
        self.1 - self.0
    }

    /// Return true if the length is zero.
    pub fn is_empty(&self) -> bool {
        self.1 == self.0
    }
}

impl BufRef {
    /// Initialize a `BufRef` that can reference a slice beginning at index `start` and has a
    /// length of `len`.
    pub fn new(start: usize, len: usize) -> Self {
        BufRef(start, start + len)
    }
}

/// A `KVRef` contains a key name and value pair, stored as two [BufRef] types.
#[derive(Clone)]
pub struct KVRef {
    name: BufRef,
    value: BufRef,
}

impl KVRef {
    /// Like [BufRef::get] for the name.
    pub fn get_name<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        self.name.get(buf)
    }

    /// Like [BufRef::get] for the value.
    pub fn get_value<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        self.value.get(buf)
    }

    /// Like [BufRef::get_bytes] for the name.
    pub fn get_name_bytes(&self, buf: &Bytes) -> Bytes {
        self.name.get_bytes(buf)
    }

    /// Like [BufRef::get_bytes] for the value.
    pub fn get_value_bytes(&self, buf: &Bytes) -> Bytes {
        self.value.get_bytes(buf)
    }

    /// Return a new `KVRef` with name and value start indices and lengths.
    pub fn new(name_s: usize, name_len: usize, value_s: usize, value_len: usize) -> Self {
        KVRef {
            name: BufRef(name_s, name_s + name_len),
            value: BufRef(value_s, value_s + value_len),
        }
    }

    /// Return a reference to the value.
    pub fn value(&self) -> &BufRef {
        &self.value
    }
}
