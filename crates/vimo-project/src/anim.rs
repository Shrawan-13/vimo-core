//! Keyframes and interpolation.
//!
//! Animation is stored per parameter. A track is a sorted list of keyframes;
//! sampling it is a binary search plus a segment evaluation, which is cheap
//! enough to run every frame for every animated parameter.
//!
//! No part of this module allocates: sampling is a pure function of the track
//! and a time.

use vimo_foundation::color::Rgba8;
use vimo_foundation::math::{Vec2, Vec3};
use vimo_foundation::time::Time;
use vimo_manifest::ParamValue;

/// How the value between two keyframes is interpolated.
#[derive(Debug, Clone, Copy, PartialEq)]
#[derive(Default)]
pub enum Interpolation {
    /// Straight line between the two values.
    #[default]
    Linear,
    /// Cubic bezier easing (`x1, y1, x2, y2`), CSS-style.
    Bezier {
        /// First control point x (0..=1).
        x1: f32,
        /// First control point y.
        y1: f32,
        /// Second control point x (0..=1).
        x2: f32,
        /// Second control point y.
        y2: f32,
    },
    /// The value holds until the next keyframe.
    Hold,
}


impl Interpolation {
    /// Build a bezier interpolation, clamping the x control points to 0..=1.
    #[must_use]
    pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Interpolation::Bezier {
            x1: x1.clamp(0.0, 1.0),
            y1,
            x2: x2.clamp(0.0, 1.0),
            y2,
        }
    }

    /// Ease a normalised `t` in `0..=1`.
    #[must_use]
    pub fn ease(&self, t: f32) -> f32 {
        match *self {
            Interpolation::Linear => t,
            Interpolation::Hold => 0.0,
            Interpolation::Bezier { x1, y1, x2, y2 } => bezier_ease(x1, y1, x2, y2, t),
        }
    }
}

/// A single keyframe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keyframe {
    /// Time relative to the owner of the animation track.
    pub time: Time,
    /// Value at this time.
    pub value: ParamValue,
    /// How to leave this keyframe towards the next one.
    pub interpolation: Interpolation,
}

impl Keyframe {
    /// A keyframe with linear interpolation.
    #[must_use]
    pub fn new(time: Time, value: ParamValue) -> Self {
        Keyframe { time, value, interpolation: Interpolation::Linear }
    }
}

/// A list of keyframes for one parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimTrack {
    /// The parameter index this track drives.
    pub param: u16,
    /// Keyframes, sorted by time, strictly increasing.
    pub keys: Vec<Keyframe>,
}

impl AnimTrack {
    /// An empty track for `param`.
    #[must_use]
    pub fn new(param: u16) -> Self {
        AnimTrack { param, keys: Vec::new() }
    }

    /// Whether the track has no keyframes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Insert or replace a keyframe, keeping the list sorted.
    ///
    /// A keyframe within one nanosecond of an existing one replaces it.
    pub fn insert(&mut self, key: Keyframe) -> usize {
        match self.keys.binary_search_by_key(&key.time, |k| k.time) {
            Ok(idx) => {
                self.keys[idx] = key;
                idx
            }
            Err(idx) => {
                self.keys.insert(idx, key);
                idx
            }
        }
    }

    /// Remove the keyframe at `index`.
    pub fn remove(&mut self, index: usize) -> Option<Keyframe> {
        if index < self.keys.len() {
            Some(self.keys.remove(index))
        } else {
            None
        }
    }

    /// Index of the keyframe at or immediately before `time`.
    fn segment(&self, time: Time) -> Option<usize> {
        if self.keys.is_empty() {
            return None;
        }
        if time < self.keys[0].time {
            return None;
        }
        // Last keyframe at or before `time`.
        let mut lo = 0usize;
        let mut hi = self.keys.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.keys[mid].time <= time {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Some(lo - 1)
    }

    /// Sample the animated value at `time` (relative to the track owner).
    ///
    /// Returns `None` when the track has no keyframes.
    #[must_use]
    pub fn sample(&self, time: Time) -> Option<ParamValue> {
        let n = self.keys.len();
        if n == 0 {
            return None;
        }
        let Some(idx) = self.segment(time) else {
            // Before the first keyframe: hold the first value.
            return Some(self.keys[0].value);
        };
        if idx + 1 >= n {
            return Some(self.keys[n - 1].value);
        }
        let a = self.keys[idx];
        let b = self.keys[idx + 1];
        let span = b.time - a.time;
        if span <= Time::ZERO {
            return Some(b.value);
        }
        let raw = ((time - a.time).as_nanos() as f64) / (span.as_nanos() as f64);
        let t = raw.clamp(0.0, 1.0) as f32;
        let eased = a.interpolation.ease(t);
        Some(lerp_value(a.value, b.value, eased))
    }
}

/// Interpolate between two parameter values.
///
/// Numeric types interpolate; discrete types (bool, enum) switch at the end of
/// the segment.
#[must_use]
pub fn lerp_value(a: ParamValue, b: ParamValue, t: f32) -> ParamValue {
    let t = t.clamp(0.0, 1.0);
    match (a, b) {
        (ParamValue::Float(x), ParamValue::Float(y)) => ParamValue::Float(x + (y - x) * t),
        (ParamValue::Int(x), ParamValue::Int(y)) => ParamValue::Int((x as f32 + (y - x) as f32 * t).round() as i32),
        (ParamValue::Bool(x), ParamValue::Bool(y)) => ParamValue::Bool(if t >= 1.0 { y } else { x }),
        (ParamValue::Enum(x), ParamValue::Enum(y)) => ParamValue::Enum(if t >= 1.0 { y } else { x }),
        (ParamValue::Color(x), ParamValue::Color(y)) => {
            // Interpolate the raw 8-bit channels; `to_f32` returns 0..1 values.
            ParamValue::Color(Rgba8::new(
                lerp_u8(x.r as f32, y.r as f32, t),
                lerp_u8(x.g as f32, y.g as f32, t),
                lerp_u8(x.b as f32, y.b as f32, t),
                lerp_u8(x.a as f32, y.a as f32, t),
            ))
        }
        (ParamValue::Vec2(x), ParamValue::Vec2(y)) => ParamValue::Vec2(Vec2::new(
            x.x + (y.x - x.x) * t,
            x.y + (y.y - x.y) * t,
        )),
        (ParamValue::Vec3(x), ParamValue::Vec3(y)) => ParamValue::Vec3(Vec3::new(
            x.x + (y.x - x.x) * t,
            x.y + (y.y - x.y) * t,
            x.z + (y.z - x.z) * t,
        )),
        // Mixed types: step at the end of the segment.
        _ => {
            if t >= 1.0 {
                b
            } else {
                a
            }
        }
    }
}

#[inline]
fn lerp_u8(a: f32, b: f32, t: f32) -> u8 {
    (a + (b - a) * t).round().clamp(0.0, 255.0) as u8
}

/// Evaluate a CSS-style cubic bezier easing function.
///
/// The x control points are clamped to `0..=1` so the curve is monotonic in x
/// and therefore invertible. The inverse is found with Newton-Raphson and falls
/// back to bisection when the derivative is too small.
#[must_use]
pub fn bezier_ease(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let x1 = x1.clamp(0.0, 1.0);
    let x2 = x2.clamp(0.0, 1.0);

    let sample_x = |u: f32| {
        let v = 1.0 - u;
        3.0 * v * v * u * x1 + 3.0 * v * u * u * x2 + u * u * u
    };
    let sample_dx = |u: f32| {
        let v = 1.0 - u;
        3.0 * v * v * x1 + 6.0 * v * u * (x2 - x1) + 3.0 * u * u * (1.0 - x2)
    };
    let sample_y = |u: f32| {
        let v = 1.0 - u;
        3.0 * v * v * u * y1 + 3.0 * v * u * u * y2 + u * u * u
    };

    let mut u = x;
    for _ in 0..8 {
        let err = sample_x(u) - x;
        if err.abs() < 1e-6 {
            return sample_y(u);
        }
        let d = sample_dx(u);
        if d.abs() < 1e-6 {
            break;
        }
        u -= err / d;
        if !(0.0..=1.0).contains(&u) {
            break;
        }
    }

    // Bisection fallback: robust, still fast (20 iterations of a cubic).
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    u = x;
    for _ in 0..24 {
        let err = sample_x(u) - x;
        if err.abs() < 1e-6 {
            break;
        }
        if err > 0.0 {
            hi = u;
        } else {
            lo = u;
        }
        u = 0.5 * (lo + hi);
    }
    sample_y(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_interpolation() {
        let mut t = AnimTrack::new(0);
        t.insert(Keyframe::new(Time::from_secs(0), ParamValue::Float(0.0)));
        t.insert(Keyframe::new(Time::from_secs(1), ParamValue::Float(10.0)));
        assert_eq!(t.sample(Time::from_secs(0)), Some(ParamValue::Float(0.0)));
        assert_eq!(t.sample(Time::from_millis(500)), Some(ParamValue::Float(5.0)));
        assert_eq!(t.sample(Time::from_secs(1)), Some(ParamValue::Float(10.0)));
    }

    #[test]
    fn holds_before_the_first_keyframe() {
        let mut t = AnimTrack::new(0);
        t.insert(Keyframe::new(Time::from_secs(1), ParamValue::Float(3.0)));
        assert_eq!(t.sample(Time::ZERO), Some(ParamValue::Float(3.0)));
    }

    #[test]
    fn hold_interpolation_snaps() {
        let mut t = AnimTrack::new(0);
        let mut k0 = Keyframe::new(Time::from_secs(0), ParamValue::Float(0.0));
        k0.interpolation = Interpolation::Hold;
        t.insert(k0);
        t.insert(Keyframe::new(Time::from_secs(1), ParamValue::Float(10.0)));
        assert_eq!(t.sample(Time::from_millis(500)), Some(ParamValue::Float(0.0)));
        assert_eq!(t.sample(Time::from_secs(1)), Some(ParamValue::Float(10.0)));
    }

    #[test]
    fn bezier_easing_endpoints_and_midpoint() {
        assert_eq!(bezier_ease(0.25, 0.1, 0.25, 1.0, 0.0), 0.0);
        assert_eq!(bezier_ease(0.25, 0.1, 0.25, 1.0, 1.0), 1.0);
        let mid = bezier_ease(0.42, 0.0, 0.58, 1.0, 0.5);
        assert!((mid - 0.5).abs() < 0.01, "mid={mid}");
    }

    #[test]
    fn bezier_ease_is_monotonic() {
        let mut prev = -1.0;
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            let y = bezier_ease(0.25, 0.1, 0.25, 1.0, x);
            assert!(y >= prev - 1e-4, "not monotonic at {x}: {y} < {prev}");
            prev = y;
        }
    }

    #[test]
    fn inserting_a_keyframe_keeps_order() {
        let mut t = AnimTrack::new(0);
        t.insert(Keyframe::new(Time::from_secs(2), ParamValue::Float(2.0)));
        t.insert(Keyframe::new(Time::from_secs(1), ParamValue::Float(1.0)));
        t.insert(Keyframe::new(Time::from_secs(3), ParamValue::Float(3.0)));
        assert_eq!(t.keys.len(), 3);
        assert_eq!(t.keys[0].time, Time::from_secs(1));
        assert_eq!(t.keys[2].time, Time::from_secs(3));
    }

    #[test]
    fn replacing_a_keyframe_at_the_same_time() {
        let mut t = AnimTrack::new(0);
        t.insert(Keyframe::new(Time::from_secs(1), ParamValue::Float(1.0)));
        t.insert(Keyframe::new(Time::from_secs(1), ParamValue::Float(9.0)));
        assert_eq!(t.keys.len(), 1);
        assert_eq!(t.keys[0].value, ParamValue::Float(9.0));
    }

    #[test]
    fn colour_interpolation() {
        let a = ParamValue::Color(Rgba8::new(0, 0, 0, 255));
        let b = ParamValue::Color(Rgba8::new(255, 255, 255, 255));
        let m = lerp_value(a, b, 0.5);
        assert_eq!(m, ParamValue::Color(Rgba8::new(128, 128, 128, 255)));
    }
}
