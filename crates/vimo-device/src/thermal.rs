//! Thermal monitoring.
//!
//! Phones throttle — or worse, shut down — when they get hot. A video editor
//! decodes, composites and encodes, which is exactly the workload that heats a
//! device up. The core therefore watches the thermal status and *proactively*
//! lowers the preview quality instead of waiting for the system to kill the
//! process.
//!
//! The monitor uses **hysteresis**: the quality is lowered quickly when the
//! device heats up, but restored only after the device has been cool for a
//! while. That prevents the quality from oscillating.

use std::collections::VecDeque;

use crate::tier::{QualitySettings, DeviceTier};

/// The thermal status reported by the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThermalState {
    /// Normal operation.
    Nominal,
    /// Slightly warm.
    Light,
    /// Warm: reduce sustained load.
    Moderate,
    /// Hot: reduce the preview quality aggressively.
    Severe,
    /// Critically hot: stop decoding ahead and drop to the minimum.
    Critical,
    /// Imminent shutdown: release everything and stop.
    Emergency,
}

impl ThermalState {
    /// Parse the Android `THERMAL_STATUS_*` constants.
    #[must_use]
    pub const fn from_android(status: i32) -> Self {
        match status {
            6 => ThermalState::Emergency,
            5 => ThermalState::Critical,
            4 => ThermalState::Severe,
            3 => ThermalState::Moderate,
            2 => ThermalState::Light,
            _ => ThermalState::Nominal,
        }
    }

    /// How many steps the quality should be reduced by (0 = none).
    #[must_use]
    pub const fn reduction_steps(self) -> u32 {
        match self {
            ThermalState::Nominal => 0,
            ThermalState::Light => 0,
            ThermalState::Moderate => 1,
            ThermalState::Severe => 2,
            ThermalState::Critical => 3,
            ThermalState::Emergency => 4,
        }
    }
}

/// A thermal reading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThermalReading {
    /// The platform's thermal status.
    pub state: ThermalState,
    /// The temperature in degrees Celsius, when known.
    pub temperature_c: Option<f32>,
}

/// Watches the thermal status and decides how much quality to give back.
#[derive(Debug, Clone)]
pub struct ThermalMonitor {
    /// The last state the platform reported.
    pub current: ThermalState,
    /// The state the quality settings are currently based on.
    pub effective: ThermalState,
    /// Consecutive readings at the current level.
    consecutive: u32,
    /// The readings seen recently.
    history: VecDeque<ThermalReading>,
    /// How many consecutive hot readings are needed before reducing quality.
    pub escalate_after: u32,
    /// How many consecutive cool readings are needed before restoring quality.
    pub recover_after: u32,
    /// The maximum number of history entries kept.
    pub history_len: usize,
}

impl Default for ThermalMonitor {
    fn default() -> Self {
        ThermalMonitor {
            current: ThermalState::Nominal,
            effective: ThermalState::Nominal,
            consecutive: 0,
            history: VecDeque::new(),
            escalate_after: 2,
            recover_after: 8,
            history_len: 32,
        }
    }
}

impl ThermalMonitor {
    /// A monitor with the default hysteresis.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a reading into the monitor.
    ///
    /// Returns the *effective* state — the one the quality settings should be
    /// based on.
    pub fn update(&mut self, reading: ThermalReading) -> ThermalState {
        self.current = reading.state;
        self.history.push_back(reading);
        while self.history.len() > self.history_len {
            self.history.pop_front();
        }

        if reading.state > self.effective {
            self.consecutive += 1;
            if self.consecutive >= self.escalate_after {
                self.effective = reading.state;
                self.consecutive = 0;
            }
        } else if reading.state < self.effective {
            self.consecutive += 1;
            if self.consecutive >= self.recover_after {
                self.effective = reading.state;
                self.consecutive = 0;
            }
        } else {
            self.consecutive = 0;
        }
        self.effective
    }

    /// The effective thermal state.
    #[must_use]
    pub fn effective_state(&self) -> ThermalState {
        self.effective
    }

    /// The most recent reading.
    #[must_use]
    pub fn latest(&self) -> Option<ThermalReading> {
        self.history.back().copied()
    }

    /// Apply the thermal state to a tier's quality settings.
    #[must_use]
    pub fn apply(&self, base: QualitySettings, tier: DeviceTier) -> QualitySettings {
        let steps = self.effective.reduction_steps();
        if steps == 0 {
            return base;
        }
        let mut q = base;
        // Each step halves the preview resolution, down to 1/16.
        let divisor = 2f32.powi(steps.min(4) as i32);
        q.preview_scale = (q.preview_scale / divisor).clamp(0.0625, 1.0);
        q.prefetch_frames = q.prefetch_frames.saturating_sub(steps);
        q.decoder_limit = q.decoder_limit.saturating_sub(steps.min(q.decoder_limit.saturating_sub(1)));
        q.effect_budget = q.effect_budget.saturating_sub(steps * 2);
        if steps >= 3 {
            q.use_proxies = true;
            q.audio_buffer_frames *= 2;
        }
        let _ = tier;
        q
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heating_up_reduces_quality() {
        let mut m = ThermalMonitor::new();
        let base = crate::tier::settings_for_tier(DeviceTier::High);
        for _ in 0..3 {
            m.update(ThermalReading { state: ThermalState::Severe, temperature_c: Some(45.0) });
        }
        assert_eq!(m.effective_state(), ThermalState::Severe);
        let q = m.apply(base, DeviceTier::High);
        assert!(q.preview_scale < base.preview_scale);
    }

    #[test]
    fn quality_is_restored_only_after_sustained_cooling() {
        let mut m = ThermalMonitor::new();
        for _ in 0..3 {
            m.update(ThermalReading { state: ThermalState::Severe, temperature_c: None });
        }
        assert_eq!(m.effective_state(), ThermalState::Severe);
        for _ in 0..3 {
            m.update(ThermalReading { state: ThermalState::Nominal, temperature_c: None });
        }
        assert_eq!(m.effective_state(), ThermalState::Severe, "must not recover too early");
        for _ in 0..10 {
            m.update(ThermalReading { state: ThermalState::Nominal, temperature_c: None });
        }
        assert_eq!(m.effective_state(), ThermalState::Nominal);
    }

    #[test]
    fn android_statuses_map_to_states() {
        assert_eq!(ThermalState::from_android(0), ThermalState::Nominal);
        assert_eq!(ThermalState::from_android(4), ThermalState::Severe);
        assert_eq!(ThermalState::from_android(6), ThermalState::Emergency);
    }
}
