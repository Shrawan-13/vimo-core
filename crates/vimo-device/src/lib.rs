//! # vimo-device
//!
//! Device adaptation for Vimo Core: capability tiers, the quality ladder and
//! thermal monitoring.
//!
//! ```
//! use vimo_device::{settings_for_tier, DeviceTier};
//!
//! let quality = settings_for_tier(DeviceTier::Low);
//! assert!(quality.preview_scale <= 0.5);
//! assert!(quality.use_proxies);
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod thermal;
pub mod tier;

pub use thermal::{ThermalMonitor, ThermalReading, ThermalState};
pub use tier::{
    settings_for_tier, DeviceProbe, DeviceProfile, DeviceTier, QualitySettings, StaticProfile,
};
