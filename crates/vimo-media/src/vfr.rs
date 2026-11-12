//! Variable frame rate handling.
//!
//! A VFR source does not have a fixed frame duration, so the core cannot
//! derive a frame index from a time with a rational timebase. Instead it keeps
//! the *actual* presentation timestamps and looks frames up in them.

use vimo_foundation::time::{Time, Timebase};

/// The presentation timestamps of a variable frame rate source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VfrTable {
    /// Sorted presentation timestamps.
    times: Vec<Time>,
    /// The nominal timebase, used for reporting.
    nominal: Timebase,
}

impl VfrTable {
    /// Build a table from sorted timestamps.
    #[must_use]
    pub fn from_times(times: Vec<Time>) -> Self {
        VfrTable { times, nominal: Timebase::default() }
    }

    /// Build a table, sorting the timestamps.
    #[must_use]
    pub fn from_unsorted(mut times: Vec<Time>) -> Self {
        times.sort_unstable();
        VfrTable { times, nominal: Timebase::default() }
    }

    /// The nominal timebase.
    #[must_use]
    pub fn nominal(&self) -> Timebase {
        self.nominal
    }

    /// Override the nominal timebase.
    pub fn set_nominal(&mut self, timebase: Timebase) {
        self.nominal = timebase;
    }

    /// The number of frames.
    #[must_use]
    pub fn len(&self) -> usize {
        self.times.len()
    }

    /// Whether the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.times.is_empty()
    }

    /// The presentation time of a frame.
    #[must_use]
    pub fn time_of(&self, frame: usize) -> Option<Time> {
        self.times.get(frame).copied()
    }

    /// The duration of a frame (the gap to the next frame).
    #[must_use]
    pub fn duration_of(&self, frame: usize) -> Time {
        let Some(t) = self.times.get(frame) else {
            return Time::ZERO;
        };
        match self.times.get(frame + 1) {
            Some(next) => next.saturating_sub(*t),
            None => Time::ZERO,
        }
    }

    /// The index of the frame that covers `time`.
    ///
    /// Returns `None` when `time` is before the first frame. A time at or
    /// after the last frame returns the last index.
    #[must_use]
    pub fn frame_at(&self, time: Time) -> Option<usize> {
        if self.times.is_empty() || time < self.times[0] {
            return None;
        }
        // Largest index whose time is <= `time`.
        let mut lo = 0usize;
        let mut hi = self.times.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.times[mid] <= time {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Some((lo - 1).min(self.times.len() - 1))
    }

    /// The average frame rate over the whole table.
    #[must_use]
    pub fn average_fps(&self) -> f64 {
        if self.times.len() < 2 {
            return 0.0;
        }
        let span = self.times[self.times.len() - 1] - self.times[0];
        if span <= Time::ZERO {
            return 0.0;
        }
        (self.times.len() - 1) as f64 / span.as_secs_f64()
    }

    /// Detect whether the timestamps are evenly spaced (within `tolerance`).
    ///
    /// Evenly spaced sources can use the fast rational timebase path.
    #[must_use]
    pub fn is_constant_rate(&self, tolerance: Time) -> bool {
        if self.times.len() < 3 {
            return true;
        }
        let first = self.times[1] - self.times[0];
        for w in self.times.windows(2) {
            let d = w[1] - w[0];
            if (d - first).abs() > tolerance {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_is_a_binary_search() {
        let t = VfrTable::from_times(vec![
            Time::from_secs(0),
            Time::from_millis(100),
            Time::from_millis(250),
        ]);
        assert_eq!(t.frame_at(Time::from_secs(0)), Some(0));
        assert_eq!(t.frame_at(Time::from_millis(150)), Some(1));
        assert_eq!(t.frame_at(Time::from_millis(300)), Some(2));
        assert_eq!(t.frame_at(Time::from_millis(-1)), None);
    }

    #[test]
    fn duration_is_the_gap_to_the_next_frame() {
        let t = VfrTable::from_times(vec![Time::ZERO, Time::from_millis(100)]);
        assert_eq!(t.duration_of(0), Time::from_millis(100));
        assert_eq!(t.duration_of(1), Time::ZERO);
    }

    #[test]
    fn constant_rate_detection() {
        let even: Vec<Time> = (0..10).map(|i| Time::from_millis(i * 33)).collect();
        assert!(VfrTable::from_times(even).is_constant_rate(Time::from_millis(1)));
        let uneven = vec![Time::ZERO, Time::from_millis(33), Time::from_millis(100)];
        assert!(!VfrTable::from_times(uneven).is_constant_rate(Time::from_millis(1)));
    }

    #[test]
    fn average_fps() {
        let t = VfrTable::from_times(vec![Time::ZERO, Time::from_secs(1)]);
        assert!((t.average_fps() - 1.0).abs() < 1e-9);
    }
}
