//! The master clock.
//!
//! Playback is driven by the **audio** device: the audio hardware consumes
//! samples at a fixed rate, and the video side follows it. The clock keeps the
//! timeline position derived from a monotonic time source so that the position
//! never goes backwards and never depends on the frame rate.
//!
//! The time source is injectable so that tests can drive the clock
//! deterministically.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use vimo_foundation::time::Time;

/// The playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    /// Not playing and positioned at the start.
    Stopped,
    /// Playing.
    Playing,
    /// Paused at a position.
    Paused,
}

/// A source of monotonic time.
pub trait TimeSource: Send + Sync {
    /// The time elapsed since the source was created.
    fn elapsed(&self) -> Duration;
}

/// A time source backed by [`Instant`].
#[derive(Debug)]
pub struct MonotonicClock {
    start: Instant,
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicClock {
    /// A new monotonic clock.
    #[must_use]
    pub fn new() -> Self {
        MonotonicClock { start: Instant::now() }
    }
}

impl TimeSource for MonotonicClock {
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

/// A time source that only moves when it is told to.
///
/// Used by tests to make playback deterministic.
#[derive(Debug, Default)]
pub struct ManualClock {
    nanos: AtomicU64,
}

impl ManualClock {
    /// A clock at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Move the clock forward.
    pub fn advance(&self, d: Duration) {
        self.nanos.fetch_add(d.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Set the clock to an absolute value.
    pub fn set(&self, d: Duration) {
        self.nanos.store(d.as_nanos() as u64, Ordering::Relaxed);
    }
}

impl TimeSource for ManualClock {
    fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.nanos.load(Ordering::Relaxed))
    }
}

/// The master playback clock.
pub struct MasterClock {
    state: PlaybackState,
    /// The timeline position at `anchor`.
    position: Time,
    /// The time source reading that corresponds to `position`.
    anchor: Duration,
    /// The playback rate (1.0 = normal, 0.5 = half speed, -1 = reverse).
    rate: f32,
    /// The end of the timeline.
    duration: Time,
    /// An optional loop range.
    loop_range: Option<(Time, Time)>,
    source: Arc<dyn TimeSource>,
}

impl std::fmt::Debug for MasterClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasterClock")
            .field("state", &self.state)
            .field("position", &self.position)
            .field("rate", &self.rate)
            .field("duration", &self.duration)
            .field("loop_range", &self.loop_range)
            .finish()
    }
}

impl Default for MasterClock {
    fn default() -> Self {
        MasterClock::new(Arc::new(MonotonicClock::new()))
    }
}

impl MasterClock {
    /// A clock driven by `source`.
    #[must_use]
    pub fn new(source: Arc<dyn TimeSource>) -> Self {
        MasterClock {
            state: PlaybackState::Stopped,
            position: Time::ZERO,
            anchor: Duration::ZERO,
            rate: 1.0,
            duration: Time::ZERO,
            loop_range: None,
            source,
        }
    }

    /// The playback state.
    #[must_use]
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// The playback rate.
    #[must_use]
    pub fn rate(&self) -> f32 {
        self.rate
    }

    /// The timeline duration.
    #[must_use]
    pub fn duration(&self) -> Time {
        self.duration
    }

    /// Set the timeline duration.
    pub fn set_duration(&mut self, duration: Time) {
        self.duration = duration;
    }

    /// Start or resume playback.
    pub fn play(&mut self) {
        if self.state == PlaybackState::Playing {
            return;
        }
        self.anchor = self.source.elapsed();
        self.state = PlaybackState::Playing;
    }

    /// Pause playback, keeping the position.
    pub fn pause(&mut self) {
        if self.state != PlaybackState::Playing {
            return;
        }
        self.position = self.raw_position();
        self.state = PlaybackState::Paused;
    }

    /// Stop playback and rewind.
    pub fn stop(&mut self) {
        self.position = Time::ZERO;
        self.anchor = self.source.elapsed();
        self.state = PlaybackState::Stopped;
    }

    /// Jump to a position.
    pub fn seek(&mut self, time: Time) {
        let clamped = time.clamp(Time::ZERO, self.duration);
        self.position = clamped;
        self.anchor = self.source.elapsed();
    }

    /// Change the playback rate.
    ///
    /// Negative rates play backwards. The position is preserved.
    pub fn set_rate(&mut self, rate: f32) {
        if self.state == PlaybackState::Playing {
            self.position = self.raw_position();
            self.anchor = self.source.elapsed();
        }
        self.rate = rate.clamp(-8.0, 8.0);
    }

    /// Set the loop range (`None` disables looping).
    pub fn set_loop(&mut self, range: Option<(Time, Time)>) {
        self.loop_range = range.filter(|(a, b)| b > a);
    }

    /// The loop range.
    #[must_use]
    pub fn loop_range(&self) -> Option<(Time, Time)> {
        self.loop_range
    }

    /// The current position, derived from the time source.
    ///
    /// Does not mutate the clock.
    #[must_use]
    pub fn position(&self) -> Time {
        match self.state {
            PlaybackState::Playing => self.raw_position(),
            _ => self.position,
        }
    }

    /// The position without looping.
    fn raw_position(&self) -> Time {
        let elapsed = self.source.elapsed().saturating_sub(self.anchor);
        let delta = Time::from_secs_f64(elapsed.as_secs_f64() * self.rate as f64);
        (self.position + delta).clamp(Time::ZERO, self.duration)
    }

    /// Advance the clock, handling looping and the end of the timeline.
    ///
    /// Returns the current position. Call this once per frame.
    pub fn tick(&mut self) -> Time {
        if self.state != PlaybackState::Playing {
            return self.position;
        }
        let mut pos = self.raw_position();
        let mut wrapped = false;

        if let Some((start, end)) = self.loop_range {
            if pos >= end {
                // Wrap around, keeping the overshoot so the loop is seamless.
                let span = end - start;
                let overshoot = pos - end;
                pos = start + overshoot % span;
                self.anchor = self.source.elapsed();
                wrapped = true;
            }
        }

        if !wrapped && pos >= self.duration {
            pos = self.duration;
            self.state = PlaybackState::Paused;
            self.anchor = self.source.elapsed();
        }
        self.position = pos;
        self.position
    }

    /// Nudge the clock to follow an external (audio) position.
    ///
    /// Small differences are ignored to avoid jitter; larger ones pull the
    /// clock into sync.
    pub fn sync_to(&mut self, external: Time, tolerance: Time) {
        if self.state != PlaybackState::Playing {
            return;
        }
        let current = self.raw_position();
        let diff = external - current;
        if diff.abs() <= tolerance {
            return;
        }
        self.position = external;
        self.anchor = self.source.elapsed();
    }

    /// The distance between the clock and an external position.
    #[must_use]
    pub fn drift(&self, external: Time) -> Time {
        external - self.position()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock() -> (MasterClock, Arc<ManualClock>) {
        let src = Arc::new(ManualClock::new());
        (MasterClock::new(src.clone()), src)
    }

    #[test]
    fn playback_advances_with_the_source() {
        let (mut c, src) = clock();
        c.set_duration(Time::from_secs(10));
        c.play();
        src.advance(Duration::from_millis(500));
        assert_eq!(c.tick(), Time::from_millis(500));
    }

    #[test]
    fn pause_freezes_the_position() {
        let (mut c, src) = clock();
        c.set_duration(Time::from_secs(10));
        c.play();
        src.advance(Duration::from_millis(500));
        c.pause();
        src.advance(Duration::from_secs(5));
        assert_eq!(c.position(), Time::from_millis(500));
    }

    #[test]
    fn seeking_moves_the_position() {
        let (mut c, _src) = clock();
        c.set_duration(Time::from_secs(10));
        c.seek(Time::from_secs(3));
        assert_eq!(c.position(), Time::from_secs(3));
    }

    #[test]
    fn rate_scales_the_speed() {
        let (mut c, src) = clock();
        c.set_duration(Time::from_secs(10));
        c.set_rate(2.0);
        c.play();
        src.advance(Duration::from_millis(500));
        assert_eq!(c.tick(), Time::from_secs(1));
    }

    #[test]
    fn reverse_plays_backwards() {
        let (mut c, src) = clock();
        c.set_duration(Time::from_secs(10));
        c.seek(Time::from_secs(5));
        c.set_rate(-1.0);
        c.play();
        src.advance(Duration::from_millis(500));
        assert_eq!(c.tick(), Time::from_millis(4500));
    }

    #[test]
    fn looping_wraps_around() {
        let (mut c, src) = clock();
        c.set_duration(Time::from_secs(10));
        c.set_loop(Some((Time::from_secs(2), Time::from_secs(4))));
        c.play();
        src.advance(Duration::from_millis(2500));
        assert_eq!(c.tick(), Time::from_millis(2500));
        src.advance(Duration::from_millis(1000));
        let pos = c.tick();
        assert!(pos >= Time::from_secs(2) && pos < Time::from_secs(3), "pos={pos:?}");
    }

    #[test]
    fn reaching_the_end_pauses() {
        let (mut c, src) = clock();
        c.set_duration(Time::from_secs(1));
        c.play();
        src.advance(Duration::from_secs(2));
        c.tick();
        assert_eq!(c.state(), PlaybackState::Paused);
        assert_eq!(c.position(), Time::from_secs(1));
    }

    #[test]
    fn sync_corrects_drift() {
        let (mut c, src) = clock();
        c.set_duration(Time::from_secs(10));
        c.play();
        src.advance(Duration::from_millis(100));
        c.sync_to(Time::from_millis(500), Time::from_millis(10));
        assert_eq!(c.position(), Time::from_millis(500));
    }
}
