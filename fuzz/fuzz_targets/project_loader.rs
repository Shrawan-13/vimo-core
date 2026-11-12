//! Fuzz the project loader.
//!
//! Run with:
//!
//! ```sh
//! cargo +nightly fuzz run project_loader
//! ```
//!
//! The invariant: the loader never panics and never reads out of bounds.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vimo_manifest::EffectRegistry;
use vimo_project::codec;

fuzz_target!(|data: &[u8]| {
    let registry = EffectRegistry::new();
    // The loader must return an error or a project, never panic.
    let _ = codec::read(data, &registry);
});
