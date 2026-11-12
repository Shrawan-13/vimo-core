//! # vimo-media
//!
//! Media I/O abstractions for Vimo Core.
//!
//! The core defines *what* it needs from the platform (decoders, encoders,
//! capability information) without knowing how it is provided: `MediaCodec` on
//! Android, FFmpeg on the desktop, or a synthetic decoder in tests.
//!
//! ```
//! use vimo_media::{Decoder, DecoderConfig, FakeDecoder, MediaFormat, VideoFormat, PixelFormat};
//! use vimo_foundation::time::Time;
//!
//! let mut format = MediaFormat::new();
//! format.video = Some(VideoFormat::new(64, 64, PixelFormat::Rgba8888));
//!
//! let mut decoder = FakeDecoder::new();
//! decoder.configure(&DecoderConfig::new(format)).unwrap();
//! let frame = decoder.decode_at(Time::from_secs(1)).unwrap();
//! assert_eq!(frame.size(), (64, 64));
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod capability;
pub mod codec;
pub mod decoder;
pub mod frame;
pub mod proxy;
pub mod surface;
pub mod vfr;

pub use capability::{low_end_phone, modern_phone, CapabilityProbe, CodecCapabilities, CodecSupport, StaticCapabilities};
pub use codec::{
    AudioCodec, AudioFormat, CodecId, ColorSpace, MediaFormat, PixelFormat, Rotation, VideoFormat,
};
pub use decoder::{
    Decoder, DecoderConfig, Encoder, EncoderConfig, EncodedChunk, FakeDecoder, FakeEncoder, SeekMode,
};
pub use frame::{AudioBuffer, FrameBuffer, VideoFrame};
pub use surface::{NoSurfaceSource, SurfacePicture, SurfaceSource, SurfaceTexture, TextureBacking};
pub use proxy::{
    ProxyJob, ProxyPolicy, ProxyRunner, ProxyScheduler, ProxyState, ProxyUpdate,
};
pub use vfr::VfrTable;
