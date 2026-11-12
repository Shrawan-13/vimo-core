//! The C shim for Oboe.
//!
//! Oboe is C++, so the audio output is reached through a tiny C interface.
//! The shim is compiled by the NDK's CMake as part of the Android build.

use vimo_foundation::error::{ErrorKind, VimoError};
use vimo_media::capability::{
    CapabilityProbe, CodecCapabilities, CodecSupport,
};
use vimo_media::codec::{AudioCodec, CodecId, PixelFormat};

/// A probe that reports a fixed capability set.
///
/// The Android layer fills this in from `MediaCodecList` and the device's
/// memory info; the core only needs the result.
#[derive(Debug, Clone)]
pub struct AndroidCapabilities {
    capabilities: CodecCapabilities,
    ram_bytes: u64,
    max_texture_size: u32,
}

impl AndroidCapabilities {
    /// Build a probe.
    #[must_use]
    pub fn new(
        capabilities: CodecCapabilities,
        ram_bytes: u64,
        max_texture_size: u32,
    ) -> Self {
        AndroidCapabilities { capabilities, ram_bytes, max_texture_size }
    }

    /// The device's physical memory.
    #[must_use]
    pub fn ram_bytes(&self) -> u64 {
        self.ram_bytes
    }

    /// The largest texture the GPU can allocate.
    #[must_use]
    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }
}

impl CapabilityProbe for AndroidCapabilities {
    fn capabilities(&self) -> CodecCapabilities {
        self.capabilities.clone()
    }
}

/// Build the capability set of a typical phone.
///
/// The Android layer replaces this with the real `MediaCodecList` query; this
/// is the fallback used when the query fails.
#[must_use]
pub fn default_phone_capabilities() -> CodecCapabilities {
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
        sampleable_formats: vec![
            PixelFormat::Rgba8888,
            PixelFormat::Bgra8888,
            PixelFormat::Surface,
        ],
    }
}

/// The device profile for a phone with the given memory and capabilities.
pub fn device_profile(
    capabilities: CodecCapabilities,
    ram_bytes: u64,
    max_texture_size: u32,
) -> vimo_device::DeviceProfile {
    vimo_device::DeviceProfile::new(ram_bytes, max_texture_size, capabilities)
}

/// A thermal reading from the platform.
#[derive(Debug, Clone, Copy)]
pub struct AndroidThermal {
    /// The `THERMAL_STATUS_*` constant.
    pub status: i32,
    /// The temperature in degrees Celsius, when known.
    pub temperature_c: Option<f32>,
}

impl AndroidThermal {
    /// Convert to the core's thermal reading.
    #[must_use]
    pub fn to_core(self) -> vimo_device::ThermalReading {
        vimo_device::ThermalReading {
            state: vimo_device::ThermalState::from_android(self.status),
            temperature_c: self.temperature_c,
        }
    }
}

/// An error from the platform layer.
#[must_use]
pub fn platform_error(context: &'static str, detail: &str) -> VimoError {
    VimoError::detailed(ErrorKind::Graphics, context, detail.to_string())
}
