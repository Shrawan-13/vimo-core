//! A small, bounds-checked binary writer.

use vimo_foundation::error::VimoResult;

/// Little-endian binary writer used by the project codec.
#[derive(Debug, Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    /// A new, empty writer.
    #[must_use]
    pub fn new() -> Self {
        Writer { buf: Vec::new() }
    }

    /// A new writer with reserved capacity.
    #[must_use]
    pub fn with_capacity(cap: usize) -> Self {
        Writer {
            buf: Vec::with_capacity(cap),
        }
    }

    /// Write a `u8`.
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    /// Write a `bool` as one byte.
    pub fn bool(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }

    /// Write a little-endian `u16`.
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Write a little-endian `u32`.
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Write a little-endian `u64`.
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Write a little-endian `i32`.
    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Write a little-endian `i64`.
    pub fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// Write a little-endian `f32` (IEEE-754 bit pattern).
    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_bits().to_le_bytes());
    }

    /// Write raw bytes.
    pub fn bytes(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }

    /// Write a length-prefixed UTF-8 string.
    pub fn str(&mut self, v: &str) {
        self.u32(v.len() as u32);
        self.buf.extend_from_slice(v.as_bytes());
    }

    /// Write a length-prefixed byte slice.
    pub fn blob(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.buf.extend_from_slice(v);
    }

    /// Append a section: tag, length, payload.
    pub fn section(&mut self, tag: u8, payload: &[u8]) {
        self.u8(tag);
        self.u32(payload.len() as u32);
        self.buf.extend_from_slice(payload);
    }

    /// The bytes written so far.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    /// Consume the writer and return the bytes.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }

    /// Number of bytes written so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether nothing has been written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

/// Write a project-like structure to any sink. Helper for tests and tooling.
pub fn write_to<W: std::io::Write>(w: &mut W, bytes: &[u8]) -> VimoResult<()> {
    w.write_all(bytes).map_err(|e| {
        vimo_foundation::error::VimoError::detailed(
            vimo_foundation::ErrorKind::Io,
            "codec",
            e.to_string(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_little_endian() {
        let mut w = Writer::new();
        w.u32(0x0102_0304);
        assert_eq!(w.as_slice(), &[0x04, 0x03, 0x02, 0x01]);
    }
}
