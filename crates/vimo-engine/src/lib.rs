//! # vimo-engine
//!
//! The Vimo Core engine: the object that owns the project, the render graph,
//! the playback clock, the resource pools and the export pipeline.
//!
//! The engine is the boundary the FFI layer talks to. It accepts commands and
//! emits events; it never blocks on I/O and never panics.
//!
//! ```
//! use vimo_engine::Engine;
//! use vimo_foundation::time::Time;
//! use vimo_manifest::EffectRegistry;
//! use vimo_project::Project;
//!
//! let engine = Engine::open(
//!     Project::new(),
//!     EffectRegistry::new(),
//!     vimo_device::DeviceProfile::default(),
//!     std::sync::Arc::new(vimo_engine::clock::ManualClock::new()),
//! );
//! assert_eq!(engine.clock().position(), Time::ZERO);
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod audio;
pub mod clock;
pub mod engine;
pub mod export;
pub mod prefetch;
pub mod render_thread;

pub use audio::{AudioSource, PlaybackAudio, SilentAudioSource};
pub use clock::{ManualClock, MasterClock, MonotonicClock, PlaybackState, TimeSource};
pub use engine::{Engine, EngineEvent, PlaybackStats};
pub use export::{ExportPipeline, ExportPreset, ExportProgress, ExportState};
pub use prefetch::{ActiveSource, PrefetchRequest, Prefetcher};
pub use render_thread::{RenderCommand, RenderContext, RenderThread};
