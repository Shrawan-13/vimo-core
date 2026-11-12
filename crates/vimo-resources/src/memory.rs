//! Memory accounting and pressure handling.
//!
//! Android gives an app a soft memory limit and then kills it. The core
//! therefore tracks every large allocation it makes and reacts to pressure
//! *before* the system starts killing processes: caches are trimmed, the
//! preview resolution is lowered, and decoders are released.

use vimo_foundation::time::Time;

/// How much memory the system is under pressure for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PressureLevel {
    /// Plenty of headroom.
    Normal,
    /// Approaching the limit; trim caches.
    Warning,
    /// Close to the limit; drop everything that can be dropped.
    Critical,
}

impl PressureLevel {
    /// Parse the Android `ComponentCallbacks2` levels.
    #[must_use]
    pub const fn from_android(level: i32) -> Self {
        match level {
            10 => PressureLevel::Critical,
            5 => PressureLevel::Warning,
            _ => PressureLevel::Normal,
        }
    }

    /// The fraction of the budget that may still be used.
    #[must_use]
    pub const fn budget_fraction(self) -> f64 {
        match self {
            PressureLevel::Normal => 1.0,
            PressureLevel::Warning => 0.75,
            PressureLevel::Critical => 0.5,
        }
    }
}

/// A tracker for the memory the core is using.
///
/// The tracker is advisory: it never *fails* an allocation, it only tells the
/// caller how much is left.
#[derive(Debug, Clone)]
pub struct MemoryTracker {
    used: usize,
    peak: usize,
    budget: usize,
}

impl MemoryTracker {
    /// A tracker with the given budget in bytes.
    #[must_use]
    pub fn new(budget: usize) -> Self {
        MemoryTracker { used: 0, peak: 0, budget: budget.max(1) }
    }

    /// The bytes currently in use.
    #[must_use]
    pub fn used(&self) -> usize {
        self.used
    }

    /// The highest usage ever reached.
    #[must_use]
    pub fn peak(&self) -> usize {
        self.peak
    }

    /// The budget in bytes.
    #[must_use]
    pub fn budget(&self) -> usize {
        self.budget
    }

    /// Change the budget.
    pub fn set_budget(&mut self, budget: usize) {
        self.budget = budget.max(1);
    }

    /// The fraction of the budget in use.
    #[must_use]
    pub fn usage(&self) -> f64 {
        self.used as f64 / self.budget as f64
    }

    /// The current pressure level.
    #[must_use]
    pub fn pressure(&self) -> PressureLevel {
        let u = self.usage();
        if u >= 0.9 {
            PressureLevel::Critical
        } else if u >= 0.75 {
            PressureLevel::Warning
        } else {
            PressureLevel::Normal
        }
    }

    /// Record an allocation.
    pub fn allocate(&mut self, bytes: usize) {
        self.used = self.used.saturating_add(bytes);
        if self.used > self.peak {
            self.peak = self.used;
        }
    }

    /// Record a release.
    pub fn release(&mut self, bytes: usize) {
        self.used = self.used.saturating_sub(bytes);
    }

    /// How many bytes may still be allocated.
    #[must_use]
    pub fn headroom(&self) -> usize {
        self.budget.saturating_sub(self.used)
    }
}

/// A suggestion for how much to free, produced under memory pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrimRequest {
    /// The level of pressure.
    pub level: PressureLevel,
    /// How many bytes should be freed.
    pub target_bytes: usize,
    /// Whether playback should continue (only the caches are trimmed) or the
    /// engine should pause.
    pub may_pause: bool,
}

/// Decides how aggressively to trim caches.
///
/// The policy is intentionally simple and deterministic so that it can be unit
/// tested:
///
/// * `Normal` — do nothing.
/// * `Warning` — drop the least recently used half of the frame cache.
/// * `Critical` — empty the frame cache, release idle decoders and textures.
pub fn trim_policy(level: PressureLevel, tracker: &MemoryTracker) -> TrimRequest {
    match level {
        PressureLevel::Normal => TrimRequest { level, target_bytes: 0, may_pause: false },
        PressureLevel::Warning => {
            let cache_bytes = (tracker.used() as f64 * 0.5) as usize;
            TrimRequest { level, target_bytes: cache_bytes, may_pause: false }
        }
        PressureLevel::Critical => TrimRequest {
            level,
            target_bytes: tracker.used(),
            may_pause: true,
        },
    }
}

/// A monotonic timer used for LRU accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tick(pub u64);

impl Tick {
    /// The next tick.
    #[must_use]
    pub fn next(self) -> Tick {
        Tick(self.0.wrapping_add(1))
    }
}

/// A duration helper used when reporting cache statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duration(Time);

impl Duration {
    /// A duration.
    #[must_use]
    pub fn new(t: Time) -> Self {
        Duration(t)
    }

    /// The duration.
    #[must_use]
    pub fn get(self) -> Time {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_levels_map_to_android() {
        assert_eq!(PressureLevel::from_android(10), PressureLevel::Critical);
        assert_eq!(PressureLevel::from_android(5), PressureLevel::Warning);
        assert_eq!(PressureLevel::from_android(0), PressureLevel::Normal);
    }

    #[test]
    fn pressure_rises_with_usage() {
        let mut t = MemoryTracker::new(1000);
        assert_eq!(t.pressure(), PressureLevel::Normal);
        t.allocate(800);
        assert_eq!(t.pressure(), PressureLevel::Warning);
        t.allocate(150);
        assert_eq!(t.pressure(), PressureLevel::Critical);
        t.release(950);
        assert_eq!(t.pressure(), PressureLevel::Normal);
    }

    #[test]
    fn critical_policy_asks_for_everything() {
        let mut t = MemoryTracker::new(1000);
        t.allocate(900);
        let req = trim_policy(PressureLevel::Critical, &t);
        assert_eq!(req.target_bytes, 900);
        assert!(req.may_pause);
    }
}
