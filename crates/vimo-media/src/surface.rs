//! The zero-copy surface contract.
//!
//! On Android, a decoded frame can be handed to the GPU **without a copy**:
//! `MediaCodec` decodes into a `Surface`, and the GPU samples that surface
//! directly. This module defines the contract the platform layer implements;
//! the core only needs to know that a texture can be backed by a surface.
//!
//! ```text
//!   MediaCodec ──► Surface ──► EGLImage / HardwareBuffer ──► GL texture
//!        (no copy: the GPU reads the decoder's output directly)
//! ```
//!
//! The core never sees an `ANativeWindow`; it only sees a [`SurfaceSource`]
//! that can produce a texture handle for a given picture.

use vimo_foundation::error::{unsupported, VimoResult};

use crate::codec::PixelFormat;

/// A picture that can be uploaded to the GPU without copying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfacePicture {
    /// The source id the picture belongs to.
    pub source: u64,
    /// The position in the source media.
    pub time: vimo_foundation::time::Time,
}

/// How a texture is backed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureBacking {
    /// An ordinary buffer that must be uploaded (one copy).
    Buffer,
    /// A hardware buffer / `AHardwareBuffer` that the GPU can sample directly.
    HardwareBuffer,
    /// A decoder surface that the GPU samples directly (zero copy).
    DecoderSurface,
}

/// A source of GPU-backed textures.
pub trait SurfaceSource: Send {
    /// Acquire the texture for a picture.
    ///
    /// The implementation either returns a ready texture or asks the decoder
    /// for one. It must not copy the pixels into a CPU buffer.
    fn acquire(&mut self, picture: SurfacePicture) -> VimoResult<SurfaceTexture>;

    /// Release a texture that is no longer needed.
    fn release(&mut self, texture: SurfaceTexture);
}

/// A GPU texture backed by a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceTexture {
    /// The texture handle.
    pub id: u32,
    /// How the texture is backed.
    pub backing: TextureBacking,
    /// The width in pixels.
    pub width: u32,
    /// The height in pixels.
    pub height: u32,
    /// The pixel format.
    pub format: PixelFormat,
}

impl SurfaceTexture {
    /// Whether the texture can be sampled without a copy.
    #[must_use]
    pub const fn is_zero_copy(&self) -> bool {
        matches!(
            self.backing,
            TextureBacking::HardwareBuffer | TextureBacking::DecoderSurface
        )
    }
}

/// A surface source that has no zero-copy path.
///
/// The engine uses this to detect that it must fall back to the buffer path.
#[derive(Debug, Default)]
pub struct NoSurfaceSource;

impl SurfaceSource for NoSurfaceSource {
    fn acquire(&mut self, _picture: SurfacePicture) -> VimoResult<SurfaceTexture> {
        Err(unsupported("surface: no zero-copy path is available"))
    }

    fn release(&mut self, _texture: SurfaceTexture) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_copy_is_detected() {
        let t = SurfaceTexture {
            id: 1,
            backing: TextureBacking::DecoderSurface,
            width: 1920,
            height: 1080,
            format: PixelFormat::Surface,
        };
        assert!(t.is_zero_copy());
        let b = SurfaceTexture {
            backing: TextureBacking::Buffer,
            ..t
        };
        assert!(!b.is_zero_copy());
    }

    #[test]
    fn the_fallback_reports_unsupported() {
        let mut s = NoSurfaceSource;
        let picture = SurfacePicture {
            source: 1,
            time: vimo_foundation::time::Time::ZERO,
        };
        assert!(s.acquire(picture).is_err());
    }
}
