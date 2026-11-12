//! Device tiers and the quality ladder.
//!
//! Vimo runs on everything from a $60 Android phone to a flagship. Rather than
//! probing every capability at runtime, the platform reports a **tier** and the
//! core maps the tier onto a concrete set of playback and export settings.
//!
//! The ladder is deliberately conservative at the low end: a low-end device
//! renders the preview at quarter resolution and edits against proxies, because
//! a smooth preview matters more than a sharp one.

use vimo_media::CodecCapabilities;

/// The performance class of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeviceTier {
    /// Entry level: 1-2 GB RAM, software decoding only.
    Low,
    /// Mid range: 3-4 GB RAM, one hardware decoder.
    Medium,
    /// Upper mid range: 6 GB RAM, several hardware decoders.
    High,
    /// Flagship: 8 GB+ RAM, many hardware decoders.
    Flagship,
}

impl DeviceTier {
    /// Derive a tier from the raw device facts.
    #[must_use]
    pub fn classify(ram_bytes: u64, hardware_decoders: u32, max_texture_size: u32) -> Self {
        let ram_gb = ram_bytes / (1024 * 1024 * 1024);
        if ram_gb >= 8 && hardware_decoders >= 4 && max_texture_size >= 4096 {
            DeviceTier::Flagship
        } else if ram_gb >= 6 && hardware_decoders >= 2 {
            DeviceTier::High
        } else if ram_gb >= 3 && hardware_decoders >= 1 {
            DeviceTier::Medium
        } else {
            DeviceTier::Low
        }
    }

    /// A short name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            DeviceTier::Low => "low",
            DeviceTier::Medium => "medium",
            DeviceTier::High => "high",
            DeviceTier::Flagship => "flagship",
        }
    }
}

/// The concrete playback and export settings for a tier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualitySettings {
    /// The preview resolution scale.
    pub preview_scale: f32,
    /// How many frames to decode ahead of the playhead.
    pub prefetch_frames: u32,
    /// How many decoders may be alive at once.
    pub decoder_limit: u32,
    /// Whether to edit against proxies.
    pub use_proxies: bool,
    /// The audio buffer size in frames (smaller is lower latency).
    pub audio_buffer_frames: usize,
    /// The maximum number of effect instances evaluated per frame.
    pub effect_budget: u32,
}

impl Default for QualitySettings {
    fn default() -> Self {
        QualitySettings {
            preview_scale: 1.0,
            prefetch_frames: 4,
            decoder_limit: 4,
            use_proxies: false,
            audio_buffer_frames: 256,
            effect_budget: 16,
        }
    }
}

/// The base quality settings for a tier.
#[must_use]
pub fn settings_for_tier(tier: DeviceTier) -> QualitySettings {
    match tier {
        DeviceTier::Low => QualitySettings {
            preview_scale: 0.5,
            prefetch_frames: 2,
            decoder_limit: 1,
            use_proxies: true,
            audio_buffer_frames: 512,
            effect_budget: 4,
        },
        DeviceTier::Medium => QualitySettings {
            preview_scale: 0.75,
            prefetch_frames: 3,
            decoder_limit: 2,
            use_proxies: true,
            audio_buffer_frames: 384,
            effect_budget: 8,
        },
        DeviceTier::High => QualitySettings {
            preview_scale: 1.0,
            prefetch_frames: 4,
            decoder_limit: 4,
            use_proxies: false,
            audio_buffer_frames: 256,
            effect_budget: 16,
        },
        DeviceTier::Flagship => QualitySettings {
            preview_scale: 1.0,
            prefetch_frames: 6,
            decoder_limit: 8,
            use_proxies: false,
            audio_buffer_frames: 192,
            effect_budget: 32,
        },
    }
}

/// A profile of the device the app runs on.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceProfile {
    /// The performance tier.
    pub tier: DeviceTier,
    /// Physical RAM in bytes.
    pub ram_bytes: u64,
    /// The largest texture the GPU can allocate.
    pub max_texture_size: u32,
    /// The codec capabilities of the device.
    pub capabilities: CodecCapabilities,
}

impl Default for DeviceProfile {
    fn default() -> Self {
        DeviceProfile {
            tier: DeviceTier::Medium,
            ram_bytes: 4 * 1024 * 1024 * 1024,
            max_texture_size: 4096,
            capabilities: vimo_media::modern_phone(),
        }
    }
}

impl DeviceProfile {
    /// Build a profile, deriving the tier from the raw facts.
    #[must_use]
    pub fn new(
        ram_bytes: u64,
        max_texture_size: u32,
        capabilities: CodecCapabilities,
    ) -> Self {
        let hardware_decoders = capabilities.video_decoders.iter().filter(|d| d.hardware).count() as u32;
        let tier = DeviceTier::classify(ram_bytes, hardware_decoders, max_texture_size);
        DeviceProfile { tier, ram_bytes, max_texture_size, capabilities }
    }

    /// The base quality settings for this device.
    #[must_use]
    pub fn quality(&self) -> QualitySettings {
        settings_for_tier(self.tier)
    }

    /// The fraction of RAM the app may use for its caches.
    #[must_use]
    pub fn cache_budget_fraction(&self) -> f64 {
        match self.tier {
            DeviceTier::Low => 0.10,
            DeviceTier::Medium => 0.15,
            DeviceTier::High => 0.20,
            DeviceTier::Flagship => 0.25,
        }
    }

    /// The memory budget for caches, in bytes.
    #[must_use]
    pub fn cache_budget_bytes(&self) -> usize {
        (self.ram_bytes as f64 * self.cache_budget_fraction()) as usize
    }
}

/// A source of device information.
pub trait DeviceProbe {
    /// The current profile.
    fn profile(&self) -> DeviceProfile;
}

/// A probe with a fixed profile, for tests and for platforms that report a
/// static profile.
#[derive(Debug, Clone, Default)]
pub struct StaticProfile {
    profile: DeviceProfile,
}

impl StaticProfile {
    /// A probe reporting `profile`.
    #[must_use]
    pub fn new(profile: DeviceProfile) -> Self {
        StaticProfile { profile }
    }
}

impl DeviceProbe for StaticProfile {
    fn profile(&self) -> DeviceProfile {
        self.profile.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_are_classified_from_the_facts() {
        assert_eq!(DeviceTier::classify(1 << 30, 0, 2048), DeviceTier::Low);
        assert_eq!(DeviceTier::classify(3 << 30, 1, 4096), DeviceTier::Medium);
        assert_eq!(DeviceTier::classify(6 << 30, 2, 4096), DeviceTier::High);
        assert_eq!(DeviceTier::classify(12 << 30, 6, 8192), DeviceTier::Flagship);
    }

    #[test]
    fn low_tier_uses_a_small_preview() {
        let s = settings_for_tier(DeviceTier::Low);
        assert!(s.preview_scale <= 0.5);
        assert!(s.use_proxies);
        assert_eq!(s.decoder_limit, 1);
    }

    #[test]
    fn cache_budget_grows_with_ram() {
        let low = DeviceProfile::new(2 << 30, 2048, vimo_media::low_end_phone());
        let high = DeviceProfile::new(12 << 30, 8192, vimo_media::modern_phone());
        assert!(high.cache_budget_bytes() > low.cache_budget_bytes());
    }
}
