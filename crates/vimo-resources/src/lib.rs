//! # vimo-resources
//!
//! Resource management for Vimo Core: pools, caches and memory accounting.
//!
//! The goal is simple to state and hard to achieve: **the steady state must
//! not allocate**. Everything the engine allocates per frame comes from a
//! pool, and every cache is bounded by bytes rather than by entry count.
//!
//! ```
//! use vimo_resources::FrameCache;
//! use vimo_resources::frame_cache::FrameKey;
//!
//! let mut cache = FrameCache::new(1024 * 1024);
//! cache.insert(FrameKey::new(1, 0), 4096, 42, vimo_foundation::time::Time::ZERO);
//! assert!(cache.get(FrameKey::new(1, 0)).is_some());
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod frame_cache;
pub mod memory;
pub mod pools;

pub use frame_cache::{CacheEntry, CacheStats, FrameCache, FrameKey};
pub use memory::{trim_policy, Duration, MemoryTracker, PressureLevel, Tick, TrimRequest};
pub use pools::{
    BufferPool, DecoderHandle, DecoderPool, TextureAllocator, TextureHandle, TexturePool,
};
