//! A small, bounds-checked binary reader.
//!
//! Every read is checked. A truncated or corrupt file produces a
//! [`VimoError`](vimo_foundation::VimoError) with [`ErrorKind::Format`](vimo_foundation::ErrorKind::Format),
//! never a panic and never an out-of-bounds access.

use vimo_foundation::error::{malformed, VimoResult};

/// Maximum length of a length-prefixed string or blob.
///
/// A corrupt length field must not be able to make the reader allocate an
/// arbitrary amount of memory.
pub const MAX_BLOB_LEN: u32 = 16 * 1024 * 1024;

/// Little-endian binary reader.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// A reader over `data`.
    #[must_use]
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    /// Current read offset.
    #[must_use]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Number of bytes left.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Whether the reader is exhausted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Read `n` raw bytes.
    pub fn bytes(&mut self, n: usize) -> VimoResult<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(|| malformed("read: length overflow"))?;
        if end > self.data.len() {
            return Err(malformed("read: unexpected end of data"));
        }
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    /// Read a `u8`.
    pub fn u8(&mut self) -> VimoResult<u8> {
        Ok(self.bytes(1)?[0])
    }

    /// Read a `bool` (any non-zero byte is `true`).
    pub fn bool(&mut self) -> VimoResult<bool> {
        Ok(self.u8()? != 0)
    }

    /// Read a little-endian `u16`.
    pub fn u16(&mut self) -> VimoResult<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    /// Read a little-endian `u32`.
    pub fn u32(&mut self) -> VimoResult<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Read a little-endian `u64`.
    pub fn u64(&mut self) -> VimoResult<u64> {
        let b = self.bytes(8)?;
        Ok(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    /// Read a little-endian `i32`.
    pub fn i32(&mut self) -> VimoResult<i32> {
        Ok(self.u32()? as i32)
    }

    /// Read a little-endian `i64`.
    pub fn i64(&mut self) -> VimoResult<i64> {
        Ok(self.u64()? as i64)
    }

    /// Read a little-endian `f32`.
    pub fn f32(&mut self) -> VimoResult<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// Read a length-prefixed UTF-8 string.
    pub fn str(&mut self) -> VimoResult<String> {
        let len = self.u32()?;
        if len > MAX_BLOB_LEN {
            return Err(malformed("read: string too long"));
        }
        let bytes = self.bytes(len as usize)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| malformed("read: invalid UTF-8 string"))
    }

    /// Read a length-prefixed byte blob.
    pub fn blob(&mut self) -> VimoResult<&'a [u8]> {
        let len = self.u32()?;
        if len > MAX_BLOB_LEN {
            return Err(malformed("read: blob too long"));
        }
        self.bytes(len as usize)
    }

    /// Read a length-prefixed array of `u32`.
    pub fn u32_vec(&mut self) -> VimoResult<Vec<u32>> {
        let len = self.u32()?;
        if len > MAX_BLOB_LEN / 4 {
            return Err(malformed("read: array too long"));
        }
        let mut out = Vec::with_capacity(len as usize);
        for _ in 0..len {
            out.push(self.u32()?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::writer::Writer;

    #[test]
    fn roundtrips_primitives() {
        let mut w = Writer::new();
        w.u8(7);
        w.u16(1000);
        w.u32(70_000);
        w.u64(u64::MAX);
        w.i64(-5);
        w.f32(1.5);
        w.bool(true);
        w.str("héllo");
        let bytes = w.finish();

        let mut r = Reader::new(&bytes);
        assert_eq!(r.u8().unwrap(), 7);
        assert_eq!(r.u16().unwrap(), 1000);
        assert_eq!(r.u32().unwrap(), 70_000);
        assert_eq!(r.u64().unwrap(), u64::MAX);
        assert_eq!(r.i64().unwrap(), -5);
        assert_eq!(r.f32().unwrap(), 1.5);
        assert!(r.bool().unwrap());
        assert_eq!(r.str().unwrap(), "héllo");
        assert!(r.is_empty());
    }

    #[test]
    fn truncated_input_is_an_error_not_a_panic() {
        let bytes = [1u8, 2, 3];
        let mut r = Reader::new(&bytes);
        assert!(r.u32().is_err());
    }

    #[test]
    fn absurd_length_is_rejected() {
        let mut w = Writer::new();
        w.u32(u32::MAX);
        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        assert!(r.str().is_err());
    }
}
