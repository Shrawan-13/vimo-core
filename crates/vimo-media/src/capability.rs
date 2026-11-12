//! Codec capability detection.
//!
//! The engine asks the platform what it can do, then picks the best path:
//! hardware decoder if it exists and fits, otherwise software, otherwise a
//! pre-generated proxy, otherwise give up gracefully.

use crate::codec::{AudioCodec, CodecId, PixelFormat};

/// What the platform can do with one codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodecSupport {
    /// The codec.
    pub codec: CodecId,
    /// Whether hardware acceleration is available.
    pub hardware: bool,
    /// The maximum width the codec handles.
    pub max_width: u32,
    /// The maximum height the codec handles.
    pub max_height: u32,
}

/// Everything the platform can decode and encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecCapabilities {
    /// Video decoders.
    pub video_decoders: Vec<CodecSupport>,
    /// Video encoders.
    pub video_encoders: Vec<CodecSupport>,
    /// Audio codecs.
    pub audio_codecs: Vec<AudioCodec>,
    /// How many hardware decoder instances may exist at once.
    pub max_decoder_instances: u32,
    /// The largest texture the GPU can allocate.
    pub max_texture_size: u32,
    /// Pixel formats that can be sampled directly by the GPU.
    pub sampleable_formats: Vec<PixelFormat>,
}

impl Default for CodecCapabilities {
    fn default() -> Self {
        CodecCapabilities {
            video_decoders: Vec::new(),
            video_encoders: Vec::new(),
            audio_codecs: Vec::new(),
            max_decoder_instances: 4,
            max_texture_size: 4096,
            sampleable_formats: vec![PixelFormat::Rgba8888, PixelFormat::Bgra8888],
        }
    }
}

impl CodecCapabilities {
    /// Whether the codec can be decoded at all.
    #[must_use]
    pub fn can_decode(&self, codec: CodecId) -> bool {
        self.video_decoders.iter().any(|d| d.codec == codec)
    }

    /// The best decoder for a codec: hardware first, then software.
    #[must_use]
    pub fn best_decoder(&self, codec: CodecId) -> Option<&CodecSupport> {
        self.video_decoders
            .iter()
            .filter(|d| d.codec == codec)
            .max_by_key(|d| d.hardware as u8)
    }

    /// Whether a codec can be decoded at the given resolution.
    #[must_use]
    pub fn can_decode_size(&self, codec: CodecId, width: u32, height: u32) -> bool {
        self.best_decoder(codec).is_some_and(|d| {
            width <= d.max_width && height <= d.max_height
        })
    }

    /// The best encoder for a codec.
    #[must_use]
    pub fn best_encoder(&self, codec: CodecId) -> Option<&CodecSupport> {
        self.video_encoders
            .iter()
            .filter(|e| e.codec == codec)
            .max_by_key(|e| e.hardware as u8)
    }

    /// Whether a pixel format can be sampled by the GPU without a conversion.
    #[must_use]
    pub fn is_sampleable(&self, format: PixelFormat) -> bool {
        self.sampleable_formats.contains(&format)
    }
}

/// A source of capability information.
pub trait CapabilityProbe {
    /// The current capabilities.
    fn capabilities(&self) -> CodecCapabilities;
}

/// A probe with fixed capabilities, for tests and for platforms that report a
/// static capability set.
#[derive(Debug, Clone, Default)]
pub struct StaticCapabilities {
    capabilities: CodecCapabilities,
}

impl StaticCapabilities {
    /// A probe reporting `capabilities`.
    #[must_use]
    pub fn new(capabilities: CodecCapabilities) -> Self {
        StaticCapabilities { capabilities }
    }
}

impl CapabilityProbe for StaticCapabilities {
    fn capabilities(&self) -> CodecCapabilities {
        self.capabilities.clone()
    }
}

/// Capabilities of a typical modern phone.
#[must_use]
pub fn modern_phone() -> CodecCapabilities {
    CodecCapabilities {
        video_decoders: vec![
            CodecSupport { codec: CodecId::H264, hardware: true, max_width: 4096, max_height: 2160 },
            CodecSupport { codec: CodecId::Hevc, hardware: true, max_width: 4096, max_height: 2160 },
            CodecSupport { codec: CodecId::Vp9, hardware: true, max_width: 3840, max_height: 2160 },
            CodecSupport { codec: CodecId::Av1, hardware: false, max_width: 1920, max_height: 1080 },
        ],
        video_encoders: vec![
            CodecSupport { codec: CodecId::H264, hardware: true, max_width: 1920, max_height: 1080 },
            CodecSupport { codec: CodecId::Hevc, hardware: true, max_width: 1920, max_height: 1080 },
        ],
        audio_codecs: vec![AudioCodec::Aac, AudioCodec::Opus, AudioCodec::Mp3, AudioCodec::Pcm],
        max_decoder_instances: 8,
        max_texture_size: 4096,
        sampleable_formats: vec![PixelFormat::Rgba8888, PixelFormat::Bgra8888, PixelFormat::Surface],
    }
}

/// Capabilities of a low-end device: one hardware decoder, software fallback.
#[must_use]
pub fn low_end_phone() -> CodecCapabilities {
    CodecCapabilities {
        video_decoders: vec![
            CodecSupport { codec: CodecId::H264, hardware: true, max_width: 1920, max_height: 1080 },
            CodecSupport { codec: CodecId::Hevc, hardware: false, max_width: 1920, max_height: 1080 },
            CodecSupport { codec: CodecId::Vp8, hardware: false, max_width: 1280, max_height: 720 },
        ],
        video_encoders: vec![CodecSupport {
            codec: CodecId::H264,
            hardware: true,
            max_width: 1920,
            max_height: 1080,
        }],
        audio_codecs: vec![AudioCodec::Aac, AudioCodec::Mp3, AudioCodec::Pcm],
        max_decoder_instances: 2,
        max_texture_size: 2048,
        sampleable_formats: vec![PixelFormat::Rgba8888, PixelFormat::Bgra8888],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_decoders_are_preferred() {
        let caps = modern_phone();
        let d = caps.best_decoder(CodecId::H264).unwrap();
        assert!(d.hardware);
    }

    #[test]
    fn size_limits_are_enforced() {
        let caps = modern_phone();
        assert!(caps.can_decode_size(CodecId::H264, 3840, 2160));
        assert!(!caps.can_decode_size(CodecId::Av1, 3840, 2160));
    }

    #[test]
    fn unknown_codecs_have_no_decoder() {
        let caps = low_end_phone();
        assert!(caps.best_decoder(CodecId::ProRes).is_none());
        assert!(!caps.can_decode(CodecId::ProRes));
    }
}
