//! Gain envelopes (fades).
//!
//! A fade is a gain that changes over time: it ramps up over the fade-in
//! duration and ramps down over the fade-out duration. The envelope is
//! evaluated per sample, so it must be cheap and allocation free.

use vimo_foundation::time::Time;

/// The shape of a fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeCurve {
    /// Linear ramp.
    Linear,
    /// Quadratic ramp (slow start).
    Quadratic,
    /// Smooth (smoothstep) ramp.
    Smooth,
    /// Constant power (equal-power cross-fade friendly).
    ConstantPower,
}

impl FadeCurve {
    /// Evaluate the curve at `t` in `0..=1`.
    #[must_use]
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            FadeCurve::Linear => t,
            FadeCurve::Quadratic => t * t,
            FadeCurve::Smooth => t * t * (3.0 - 2.0 * t),
            FadeCurve::ConstantPower => t.sqrt(),
        }
    }
}

/// A gain envelope over a clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GainEnvelope {
    /// How long the fade in lasts.
    pub fade_in: Time,
    /// How long the fade out lasts.
    pub fade_out: Time,
    /// The curve used for both fades.
    pub curve: FadeCurve,
    /// The base gain.
    pub gain: f32,
}

impl Default for GainEnvelope {
    fn default() -> Self {
        GainEnvelope {
            fade_in: Time::ZERO,
            fade_out: Time::ZERO,
            curve: FadeCurve::Linear,
            gain: 1.0,
        }
    }
}

impl GainEnvelope {
    /// A linear envelope with the given fades.
    #[must_use]
    pub fn linear(fade_in: Time, fade_out: Time) -> Self {
        GainEnvelope {
            fade_in,
            fade_out,
            curve: FadeCurve::Linear,
            gain: 1.0,
        }
    }

    /// The gain at `position` within a clip of duration `duration`.
    #[must_use]
    pub fn gain_at(&self, position: Time, duration: Time) -> f32 {
        let mut g = self.gain;
        if self.fade_in > Time::ZERO && position < self.fade_in {
            let t = (position.as_secs_f64() / self.fade_in.as_secs_f64()) as f32;
            g *= self.curve.apply(t);
        }
        if self.fade_out > Time::ZERO && duration > Time::ZERO {
            let fade_start = duration - self.fade_out;
            if position > fade_start {
                let remaining = duration - position;
                let t = (remaining.as_secs_f64() / self.fade_out.as_secs_f64()) as f32;
                g *= self.curve.apply(t);
            }
        }
        g.clamp(0.0, 4.0)
    }

    /// The gain for a sample index inside a buffer of `frames` samples.
    #[must_use]
    pub fn gain_for_frame(&self, frame: usize, frames: usize, sample_rate: u32) -> f32 {
        if sample_rate == 0 || frames == 0 {
            return self.gain;
        }
        let position = Time::from_secs_f64(frame as f64 / sample_rate as f64);
        let duration = Time::from_secs_f64(frames as f64 / sample_rate as f64);
        self.gain_at(position, duration)
    }
}

/// A keyframed gain curve over the timeline.
///
/// Used for volume automation lanes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutomationCurve {
    /// `(time, gain)` pairs, sorted by time.
    pub points: Vec<(Time, f32)>,
}

impl AutomationCurve {
    /// An empty curve (unity gain).
    #[must_use]
    pub fn new() -> Self {
        AutomationCurve::default()
    }

    /// Add a point, keeping the list sorted.
    pub fn add_point(&mut self, time: Time, gain: f32) {
        let gain = gain.clamp(0.0, 4.0);
        match self.points.binary_search_by_key(&time, |p| p.0) {
            Ok(i) => self.points[i] = (time, gain),
            Err(i) => self.points.insert(i, (time, gain)),
        }
    }

    /// Remove the point at `time`.
    pub fn remove_point(&mut self, time: Time) -> bool {
        if let Ok(i) = self.points.binary_search_by_key(&time, |p| p.0) {
            self.points.remove(i);
            true
        } else {
            false
        }
    }

    /// The gain at `time`.
    #[must_use]
    pub fn gain_at(&self, time: Time) -> f32 {
        match self.points.len() {
            0 => 1.0,
            1 => self.points[0].1,
            _ => {
                if time <= self.points[0].0 {
                    return self.points[0].1;
                }
                let last = self.points[self.points.len() - 1];
                if time >= last.0 {
                    return last.1;
                }
                let idx = self.points.partition_point(|(t, _)| *t <= time).max(1);
                let (t0, g0) = self.points[idx - 1];
                let (t1, g1) = self.points[idx];
                let span = (t1 - t0).as_secs_f64();
                if span <= 0.0 {
                    return g1;
                }
                let t = ((time - t0).as_secs_f64() / span) as f32;
                g0 + (g1 - g0) * t.clamp(0.0, 1.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_in_ramps_from_zero() {
        let e = GainEnvelope::linear(Time::from_secs(1), Time::ZERO);
        assert_eq!(e.gain_at(Time::ZERO, Time::from_secs(2)), 0.0);
        assert!((e.gain_at(Time::from_millis(500), Time::from_secs(2)) - 0.5).abs() < 1e-6);
        assert_eq!(e.gain_at(Time::from_secs(1), Time::from_secs(2)), 1.0);
    }

    #[test]
    fn fade_out_ramps_to_zero() {
        let e = GainEnvelope::linear(Time::ZERO, Time::from_secs(1));
        assert_eq!(e.gain_at(Time::from_secs(2), Time::from_secs(2)), 0.0);
        assert!((e.gain_at(Time::from_millis(1500), Time::from_secs(2)) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn automation_curve_interpolates() {
        let mut c = AutomationCurve::new();
        c.add_point(Time::ZERO, 0.0);
        c.add_point(Time::from_secs(2), 1.0);
        assert!((c.gain_at(Time::from_secs(1)) - 0.5).abs() < 1e-6);
        assert_eq!(c.gain_at(Time::from_secs(3)), 1.0);
        assert_eq!(c.gain_at(Time::from_secs(-1)), 0.0);
    }

    #[test]
    fn automation_points_are_sorted() {
        let mut c = AutomationCurve::new();
        c.add_point(Time::from_secs(2), 1.0);
        c.add_point(Time::from_secs(1), 0.5);
        assert_eq!(c.points[0].0, Time::from_secs(1));
    }
}
