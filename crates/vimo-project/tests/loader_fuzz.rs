//! Fuzz-style tests for the project loader.
//!
//! These run in normal `cargo test` (no nightly needed) and use a
//! deterministic PRNG so that a failure is always reproducible. The
//! `cargo-fuzz` targets in `fuzz/` cover the same code with libFuzzer's
//! coverage-guided exploration.
//!
//! The invariant under test: **the loader never panics**, whatever it is
//! given.

use vimo_manifest::EffectRegistry;
use vimo_project::codec;

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

    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next_u8()).collect()
    }
}

fn registry() -> EffectRegistry {
    EffectRegistry::new()
}

#[test]
fn random_bytes_never_panic() {
    let reg = registry();
    let mut rng = Rng::new(0x1234_5678_9abc_def0);
    for _ in 0..2_000 {
        let len = (rng.next_u64() % 512) as usize;
        let data = rng.bytes(len);
        let _ = codec::read(&data, &reg);
    }
}

#[test]
fn truncated_files_never_panic() {
    let reg = registry();
    let mut project = vimo_project::Project::new();
    project.assets.push(vimo_project::model::Asset::video("a", vimo_foundation::time::Time::from_secs(5)));
    project.tracks.push(vimo_project::model::Track::new(
        "V1",
        vimo_project::model::TrackKind::Video,
    ));
    let full = codec::write(&project, &reg, codec::CURRENT_VERSION).expect("write");

    for cut in 0..full.len() {
        let _ = codec::read(&full[..cut], &reg);
    }
}

#[test]
fn corrupted_files_never_panic() {
    let reg = registry();
    let mut project = vimo_project::Project::new();
    project.assets.push(vimo_project::model::Asset::video("a", vimo_foundation::time::Time::from_secs(5)));
    project.tracks.push(vimo_project::model::Track::new(
        "V1",
        vimo_project::model::TrackKind::Video,
    ));
    let full = codec::write(&project, &reg, codec::CURRENT_VERSION).expect("write");

    let mut rng = Rng::new(0xdead_beef_1234_5678);
    for _ in 0..2_000 {
        let mut data = full.clone();
        let flips = 1 + (rng.next_u64() % 8) as usize;
        for _ in 0..flips {
            let index = (rng.next_u64() as usize) % data.len();
            data[index] = rng.next_u8();
        }
        let _ = codec::read(&data, &reg);
    }
}

#[test]
fn absurd_sections_are_rejected() {
    let reg = registry();
    // A section that claims to be 4 GB long.
    let mut data = Vec::new();
    data.extend_from_slice(b"VIMO");
    data.extend_from_slice(&codec::CURRENT_VERSION.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.push(1);
    data.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(codec::read(&data, &reg).is_err());
}

#[test]
fn every_version_roundtrips() {
    let reg = registry();
    let mut project = vimo_project::Project::new();
    project.assets.push(vimo_project::model::Asset::video("a", vimo_foundation::time::Time::from_secs(5)));
    project.tracks.push(vimo_project::model::Track::new(
        "V1",
        vimo_project::model::TrackKind::Video,
    ));
    for version in 1..=codec::CURRENT_VERSION {
        let bytes = codec::write(&project, &reg, version).expect("write");
        let (loaded, report) = codec::read(&bytes, &reg).expect("read");
        assert_eq!(report.source_version, version);
        assert_eq!(loaded.assets.len(), 1);
        assert_eq!(loaded.tracks.len(), 1);
    }
}
