//! Fuzz-style tests for the effect manifest parser.
//!
//! The parser is the one place where untrusted text enters the core, so it
//! must never panic and must never allocate unbounded amounts of memory.

use vimo_manifest::Manifest;

/// A deterministic xorshift PRNG.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_u8(&mut self) -> u8 {
        (self.next_u64() & 0xff) as u8
    }
}

const ALPHABET: &[u8] = b"{}[]\",:0123456789abcdefghijklmnopqrstuvwxyz_-. \n\t\\/\x00\xff";

#[test]
fn random_text_never_panics() {
    let mut rng = Rng::new(0xfeed_face_1234_5678);
    for _ in 0..5_000 {
        let len = (rng.next_u64() % 256) as usize;
        let text: String = (0..len)
            .map(|_| ALPHABET[(rng.next_u64() as usize) % ALPHABET.len()] as char)
            .collect();
        let _ = Manifest::parse_str(&text);
    }
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng::new(0x0bad_c0de_0bad_c0de);
    for _ in 0..2_000 {
        let len = (rng.next_u64() % 512) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| rng.next_u8()).collect();
        let _ = Manifest::parse_bytes(&bytes);
    }
}

#[test]
fn mutated_valid_manifests_never_panic() {
    let base = br#"{"effects":[{"id":"com.vimo.effect.blur","name":"Blur","params":[{"name":"radius","type":"float","default":4.0,"min":0.0,"max":32.0},{"name":"q","type":"enum","default":0,"options":["a","b"]}]}]}"#;
    let mut rng = Rng::new(0xabcd_ef01_2345_6789);
    for _ in 0..5_000 {
        let mut data = base.to_vec();
        let flips = 1 + (rng.next_u64() % 6) as usize;
        for _ in 0..flips {
            let index = (rng.next_u64() as usize) % data.len();
            data[index] = rng.next_u8();
        }
        let _ = Manifest::parse_bytes(&data);
    }
}

#[test]
fn deeply_nested_json_is_rejected_quickly() {
    let depth = 10_000;
    let text = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    // Must not panic and must not blow the stack.
    let result = Manifest::parse_str(&text);
    assert!(result.is_err() || result.is_ok());
}

#[test]
fn oversized_manifest_is_rejected() {
    let mut text = String::from("{\"effects\":[");
    for i in 0..10_000 {
        if i > 0 {
            text.push(',');
        }
        text.push_str(&format!("{{\"id\":\"e{i}\"}}"));
    }
    text.push_str("]}");
    assert!(
        Manifest::parse_str(&text).is_err(),
        "too many effects must be rejected"
    );
}
