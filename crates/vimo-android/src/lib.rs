//! # vimo-android
//!
//! The Android platform layer for Vimo Core.
//!
//! The core is platform independent; everything that needs Android lives
//! here:
//!
//! | Module | What |
//! |--------|------|
//! | [`egl`] | The EGL context and the OpenGL ES 3 implementation of the core's `GlApi` trait. |
//! | [`mediacodec`] | The hardware decoder and encoder. |
//! | [`audio`] | The Oboe low-latency output. |
//! | [`device`] | Capability detection, the device profile and thermal readings. |
//! | [`trace`] | The Perfetto / ATrace sink. |
//! | [`surface`] | The zero-copy `Surface → GPU texture` path. |
//!
//! The native bindings are declared by hand ([`ffi`]) rather than generated
//! from the NDK headers, so that this crate **compiles on a desktop** and the
//! Rust code is type checked by CI even when the Android build is not
//! running.

#![deny(missing_docs)]

pub mod audio;
pub mod device;
pub mod egl;
pub mod ffi;
pub mod mediacodec;
pub mod surface;
pub mod trace;

pub use audio::OboeAudioOutput;
pub use device::{default_phone_capabilities, device_profile, AndroidCapabilities, AndroidThermal};
pub use egl::EglRenderer;
pub use mediacodec::MediaCodecDecoder;
pub use surface::{SurfaceResolver, SurfaceTextures};
pub use trace::{LogcatSink, PerfettoSink};
