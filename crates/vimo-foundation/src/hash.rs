//! A fast, non-cryptographic hasher (FxHash style).
//!
//! `std`'s SipHash is DoS-resistant but costs ~1 ns per byte, which shows up in
//! the render graph where we hash project state every frame. Vimo hashes only
//! internally-generated data, so a cheap multiply-xor hash is the right
//! trade-off. This is the same construction used by `rustc` and `rustc-hash`.

use core::hash::{BuildHasherDefault, Hasher};

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

/// The Fx hasher.
#[derive(Debug, Clone, Copy, Default)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    /// Create a hasher with the default seed.
    #[must_use]
    pub fn new() -> Self {
        FxHasher { hash: 0 }
    }

    #[inline]
    fn add_to_hash(&mut self, i: u64) {
        self.hash = (self.hash.rotate_left(5) ^ i).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add_to_hash(u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut buf = [0u8; 8];
            buf[..rest.len()].copy_from_slice(rest);
            self.add_to_hash(u64::from_le_bytes(buf));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add_to_hash(u64::from(i));
    }
    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add_to_hash(u64::from(i));
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add_to_hash(u64::from(i));
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add_to_hash(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add_to_hash(i as u64);
    }
    #[inline]
    fn write_i8(&mut self, i: i8) {
        self.add_to_hash(i as u64);
    }
    #[inline]
    fn write_i16(&mut self, i: i16) {
        self.add_to_hash(i as u64);
    }
    #[inline]
    fn write_i32(&mut self, i: i32) {
        self.add_to_hash(i as u64);
    }
    #[inline]
    fn write_i64(&mut self, i: i64) {
        self.add_to_hash(i as u64);
    }
    #[inline]
    fn write_isize(&mut self, i: isize) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

/// A [`BuildHasher`] producing [`FxHasher`]s.
pub type FxBuildHasher = BuildHasherDefault<FxHasher>;

/// Hash a single `u64` with FxHash. Useful for content fingerprints.
#[must_use]
pub fn hash_u64(v: u64) -> u64 {
    let mut h = FxHasher::new();
    h.write_u64(v);
    h.finish()
}

/// Combine two hashes into one.
#[must_use]
pub fn hash_combine(a: u64, b: u64) -> u64 {
    let mut h = FxHasher::new();
    h.write_u64(a);
    h.write_u64(b);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::hash::Hash;

    #[test]
    fn deterministic() {
        let mut a = FxHasher::new();
        42u64.hash(&mut a);
        let mut b = FxHasher::new();
        42u64.hash(&mut b);
        assert_eq!(a.finish(), b.finish());
    }

    #[test]
    fn differs_for_close_values() {
        let mut a = FxHasher::new();
        1u64.hash(&mut a);
        let mut b = FxHasher::new();
        2u64.hash(&mut b);
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn handles_bytes_of_various_lengths() {
        let mut h1 = FxHasher::new();
        h1.write(b"abcdefgh");
        let mut h2 = FxHasher::new();
        h2.write(b"abcdefg");
        assert_ne!(h1.finish(), h2.finish());
    }
}
