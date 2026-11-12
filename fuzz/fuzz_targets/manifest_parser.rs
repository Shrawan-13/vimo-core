//! Fuzz the effect manifest parser.
//!
//! Run with:
//!
//! ```sh
//! cargo +nightly fuzz run manifest_parser
//! ```
//!
//! The invariant: the parser never panics and never allocates unbounded
//! amounts of memory.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vimo_manifest::Manifest;

fuzz_target!(|data: &[u8]| {
    let _ = Manifest::parse_bytes(data);
});
