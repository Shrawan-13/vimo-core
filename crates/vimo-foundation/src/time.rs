//! Exact time arithmetic.
//!
//! Vimo stores every timeline position as a signed count of **nanoseconds**
//! ([`Time`]). Rational frame rates are kept as normalized [`Rational`] values so
//! that 30000/1001 never becomes 29.97.
//!
//! Floating point is used only where the result is inherently fractional
//! (e.g. asking "where exactly is 12.5 frames?"); all conversions that must be
//! exact are computed in `i128` and rounded once.

use core::cmp::Ordering;
use core::fmt;
use core::ops::{Add, AddAssign, Div, Mul, Neg, Rem, Sub, SubAssign};

/// Nanoseconds per second.
pub const NANOS_PER_SEC: i64 = 1_000_000_000;
/// Microseconds per second.
pub const MICROS_PER_SEC: i64 = 1_000_000;
/// Milliseconds per second.
pub const MILLIS_PER_SEC: i64 = 1_000;

/// A signed duration/position in nanoseconds.
///
/// Range: roughly +/- 292 years. The type is `Copy`, `Eq` and `Ord`, so it can
/// be used as a map key and compared without float ambiguity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct Time(i64);

impl Time {
    /// The zero instant.
    pub const ZERO: Time = Time(0);
    /// Smallest representable duration.
    pub const MIN: Time = Time(i64::MIN);
    /// Largest representable duration.
    pub const MAX: Time = Time(i64::MAX);
    /// One second.
    pub const SECOND: Time = Time(NANOS_PER_SEC);

    /// Construct from raw nanoseconds.
    #[must_use]
    pub const fn from_nanos(ns: i64) -> Self {
        Time(ns)
    }

    /// Construct from microseconds.
    #[must_use]
    pub const fn from_micros(us: i64) -> Self {
        Time(us.saturating_mul(1_000))
    }

    /// Construct from milliseconds.
    #[must_use]
    pub const fn from_millis(ms: i64) -> Self {
        Time(ms.saturating_mul(1_000_000))
    }

    /// Construct from whole seconds.
    #[must_use]
    pub const fn from_secs(secs: i64) -> Self {
        Time(secs.saturating_mul(NANOS_PER_SEC))
    }

    /// Construct from a floating point number of seconds.
    ///
    /// Rounds to the nearest nanosecond. Values outside the representable range
    /// saturate rather than panic.
    #[must_use]
    pub fn from_secs_f64(secs: f64) -> Self {
        if !secs.is_finite() {
            return Time::ZERO;
        }
        let scaled = secs * NANOS_PER_SEC as f64;
        if scaled >= i64::MAX as f64 {
            return Time::MAX;
        }
        if scaled <= i64::MIN as f64 {
            return Time::MIN;
        }
        Time(scaled.round() as i64)
    }

    /// Raw nanoseconds.
    #[must_use]
    pub const fn as_nanos(self) -> i64 {
        self.0
    }

    /// Whole seconds (truncating toward zero).
    #[must_use]
    pub const fn as_secs(self) -> i64 {
        self.0 / NANOS_PER_SEC
    }

    /// Fractional seconds as `f64`.
    #[must_use]
    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / NANOS_PER_SEC as f64
    }

    /// Milliseconds (truncating toward zero).
    #[must_use]
    pub const fn as_millis(self) -> i64 {
        self.0 / 1_000_000
    }

    /// Sub-second remainder in nanoseconds, always in `[0, 1e9)`.
    #[must_use]
    pub const fn subsec_nanos(self) -> i64 {
        self.0.rem_euclid(NANOS_PER_SEC)
    }

    /// Checked addition.
    #[must_use]
    pub const fn checked_add(self, rhs: Time) -> Option<Time> {
        match self.0.checked_add(rhs.0) {
            Some(v) => Some(Time(v)),
            None => None,
        }
    }

    /// Checked subtraction.
    #[must_use]
    pub const fn checked_sub(self, rhs: Time) -> Option<Time> {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Some(Time(v)),
            None => None,
        }
    }

    /// Saturating addition: clamps at the representable bounds instead of wrapping.
    #[must_use]
    pub const fn saturating_add(self, rhs: Time) -> Time {
        Time(self.0.saturating_add(rhs.0))
    }

    /// Saturating subtraction.
    #[must_use]
    pub const fn saturating_sub(self, rhs: Time) -> Time {
        Time(self.0.saturating_sub(rhs.0))
    }

    /// Minimum of two times.
    #[must_use]
    pub fn min(self, other: Time) -> Time {
        if self <= other {
            self
        } else {
            other
        }
    }

    /// Maximum of two times.
    #[must_use]
    pub fn max(self, other: Time) -> Time {
        if self >= other {
            self
        } else {
            other
        }
    }

    /// Clamp into `[lo, hi]`.
    #[must_use]
    pub fn clamp(self, lo: Time, hi: Time) -> Time {
        debug_assert!(lo <= hi, "clamp bounds inverted");
        self.max(lo).min(hi)
    }

    /// Absolute value.
    #[must_use]
    pub const fn abs(self) -> Time {
        Time(self.0.abs())
    }

    /// Frame index (floor) at the given timebase.
    ///
    /// Negative times floor toward negative infinity, matching the behaviour of
    /// "frame 0 starts at 0", so negative pre-roll time maps to frame `-1`.
    #[must_use]
    pub fn frame_at(self, tb: Timebase) -> i64 {
        tb.time_to_frame_floor(self)
    }

    /// Fractional frame position at the given timebase.
    #[must_use]
    pub fn frames_at(self, tb: Timebase) -> f64 {
        tb.time_to_frame_exact(self)
    }

    /// Convert to the timestamp of a frame index under `tb`.
    #[must_use]
    pub fn frame_time(tb: Timebase, frame: i64) -> Time {
        tb.frame_to_time(frame)
    }
}

impl Add for Time {
    type Output = Time;
    fn add(self, rhs: Time) -> Time {
        Time(self.0 + rhs.0)
    }
}

impl AddAssign for Time {
    fn add_assign(&mut self, rhs: Time) {
        self.0 += rhs.0;
    }
}

impl Sub for Time {
    type Output = Time;
    fn sub(self, rhs: Time) -> Time {
        Time(self.0 - rhs.0)
    }
}

impl SubAssign for Time {
    fn sub_assign(&mut self, rhs: Time) {
        self.0 -= rhs.0;
    }
}

impl Neg for Time {
    type Output = Time;
    fn neg(self) -> Time {
        Time(-self.0)
    }
}

impl Mul<i64> for Time {
    type Output = Time;
    fn mul(self, rhs: i64) -> Time {
        Time(self.0 * rhs)
    }
}

impl Div<i64> for Time {
    type Output = Time;
    /// Integer division of the nanosecond count.
    fn div(self, rhs: i64) -> Time {
        Time(self.0 / rhs)
    }
}

impl Div<Time> for Time {
    type Output = i64;
    /// The number of whole `rhs` in `self`.
    fn div(self, rhs: Time) -> i64 {
        if rhs.0 == 0 {
            0
        } else {
            self.0 / rhs.0
        }
    }
}

impl Rem<Time> for Time {
    type Output = Time;
    /// The remainder of the nanosecond counts.
    fn rem(self, rhs: Time) -> Time {
        if rhs.0 == 0 {
            Time::ZERO
        } else {
            Time(self.0 % rhs.0)
        }
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let neg = self.0 < 0;
        let abs = self.0.unsigned_abs();
        let total_secs = abs / NANOS_PER_SEC as u64;
        let nanos = abs % NANOS_PER_SEC as u64;
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let secs = total_secs % 60;
        let millis = nanos / 1_000_000;
        if neg {
            write!(f, "-")?;
        }
        write!(f, "{hours:02}:{minutes:02}:{secs:02}.{millis:03}")?;
        if nanos % 1_000 != 0 {
            write!(f, ".{:03}", nanos % 1_000)?;
        }
        Ok(())
    }
}

/// A normalized rational number `num / den` with `den > 0` and `gcd(num, den) == 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rational {
    num: i64,
    den: i64,
}

impl Rational {
    /// Zero.
    pub const ZERO: Rational = Rational { num: 0, den: 1 };
    /// One.
    pub const ONE: Rational = Rational { num: 1, den: 1 };

    /// Construct and normalize. Returns `None` when `den == 0`.
    #[must_use]
    pub fn new(num: i64, den: i64) -> Option<Self> {
        if den == 0 {
            return None;
        }
        let (mut num, mut den) = (num, den);
        if den < 0 {
            num = -num;
            den = -den;
        }
        let g = gcd(num.unsigned_abs(), den as u64).max(1);
        Some(Rational {
            num: num / g as i64,
            den: den / g as i64,
        })
    }

    /// Construct, panicking on a zero denominator. Use in `const` contexts.
    ///
    /// # Panics
    /// Panics if `den == 0`.
    #[must_use]
    pub const fn new_const(num: i64, den: i64) -> Self {
        assert!(den != 0, "rational denominator must be non-zero");
        let (mut n, mut d) = (num, den);
        if d < 0 {
            n = -n;
            d = -d;
        }
        // const-friendly gcd
        let mut a = if n < 0 { (-n) as u64 } else { n as u64 };
        let mut b = d as u64;
        while b != 0 {
            let t = a % b;
            a = b;
            b = t;
        }
        let g = if a == 0 { 1 } else { a };
        Rational {
            num: n / g as i64,
            den: d / g as i64,
        }
    }

    /// Numerator (sign carries the value).
    #[must_use]
    pub const fn num(self) -> i64 {
        self.num
    }

    /// Denominator, always positive.
    #[must_use]
    pub const fn den(self) -> i64 {
        self.den
    }

    /// Floating point value.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Reciprocal, or `None` if this is zero.
    #[must_use]
    pub fn recip(self) -> Option<Rational> {
        if self.num == 0 {
            None
        } else {
            Rational::new(self.den, self.num)
        }
    }

    /// Checked addition.
    #[must_use]
    pub fn checked_add(self, rhs: Rational) -> Option<Rational> {
        let (n, d) = (
            self.num as i128 * rhs.den as i128 + rhs.num as i128 * self.den as i128,
            self.den as i128 * rhs.den as i128,
        );
        if n > i64::MAX as i128 || n < i64::MIN as i128 || d > i64::MAX as i128 {
            return None;
        }
        Rational::new(n as i64, d as i64)
    }

    /// Checked multiplication.
    #[must_use]
    pub fn checked_mul(self, rhs: Rational) -> Option<Rational> {
        let n = self.num as i128 * rhs.num as i128;
        let d = self.den as i128 * rhs.den as i128;
        if n > i64::MAX as i128 || n < i64::MIN as i128 || d > i64::MAX as i128 {
            return None;
        }
        Rational::new(n as i64, d as i64)
    }

    /// Approximate equality with a relative tolerance.
    #[must_use]
    pub fn approx_eq(self, rhs: Rational, rel_eps: f64) -> bool {
        let a = self.as_f64();
        let b = rhs.as_f64();
        let diff = (a - b).abs();
        diff <= rel_eps * a.abs().max(b.abs()).max(1.0)
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// A frame rate (rational) plus drop-frame signalling.
///
/// Drop-frame (29.97DF / 59.94DF) is modelled explicitly because the *frame
/// numbering* differs from the *time* even though the rate is identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timebase {
    fps: Rational,
    drop_frame: bool,
}

impl Timebase {
    /// A non-drop-frame timebase.
    #[must_use]
    pub const fn from_fps(num: i64, den: i64) -> Self {
        Timebase {
            fps: Rational::new_const(num, den),
            drop_frame: false,
        }
    }

    /// A timebase with explicit drop-frame flag.
    #[must_use]
    pub const fn with_drop_frame(fps: Rational, drop_frame: bool) -> Self {
        Timebase { fps, drop_frame }
    }

    /// The frame rate.
    #[must_use]
    pub const fn fps(self) -> Rational {
        self.fps
    }

    /// Whether drop-frame numbering is used.
    #[must_use]
    pub const fn is_drop_frame(self) -> bool {
        self.drop_frame
    }

    /// True if this timebase can express drop-frame numbering (29.97 or 59.94).
    #[must_use]
    pub const fn supports_drop_frame(self) -> bool {
        self.fps.num == 30000 && self.fps.den == 1001
            || self.fps.num == 60000 && self.fps.den == 1001
    }

    /// Nanoseconds per frame as an exact rational (`1e9 / fps`).
    #[must_use]
    pub fn frame_duration(self) -> Rational {
        Rational::new(NANOS_PER_SEC * self.fps.den, self.fps.num).unwrap_or(Rational::ONE)
    }

    /// Exact timestamp of a frame index (linear numbering).
    ///
    /// The result is rounded **up** to the next whole nanosecond. Rounding up
    /// guarantees that [`Timebase::time_to_frame_floor`] maps the returned
    /// timestamp back to the same frame index, whereas rounding to nearest can
    /// land 1 ns below the true boundary and shift the frame by one.
    #[must_use]
    pub fn frame_to_time(self, frame: i64) -> Time {
        let num = (frame as i128) * NANOS_PER_SEC as i128 * self.fps.den as i128;
        let den = self.fps.num as i128;
        Time(clamp_i128(div_ceil(num, den)))
    }

    /// Number of frame *labels* skipped at each minute boundary for this rate.
    #[must_use]
    pub const fn drop_frames_per_minute(self) -> i64 {
        if self.fps.num == 60000 {
            4
        } else {
            2
        }
    }

    /// Continuous (linear) frames elapsed in one minute of wall-clock time.
    #[must_use]
    pub fn continuous_frames_per_minute(self) -> i64 {
        // fps * 60, truncated: 29.97 -> 1798, 59.94 -> 3596.
        (self.fps.num as i128 * 60 / self.fps.den as i128) as i64
    }

    /// Continuous frames elapsed in ten minutes of wall-clock time.
    #[must_use]
    pub fn continuous_frames_per_10min(self) -> i64 {
        (self.fps.num as i128 * 600 / self.fps.den as i128) as i64
    }

    /// Convert a *continuous* frame index into the drop-frame **label**.
    ///
    /// For 29.97DF the labels `00:01:00;00` and `00:01:00;01` never appear:
    /// they are skipped at the start of every minute except every tenth.
    #[must_use]
    pub fn continuous_to_drop_label(self, continuous: i64) -> i64 {
        if !self.drop_frame || !self.supports_drop_frame() {
            return continuous;
        }
        let drop = self.drop_frames_per_minute();
        let per10 = self.continuous_frames_per_10min();
        let per_min = self.continuous_frames_per_minute();
        let d = continuous.div_euclid(per10);
        let m = continuous.rem_euclid(per10);
        let mut label = continuous + 9 * drop * d;
        if m >= drop {
            label += drop * ((m - drop) / per_min);
        }
        label
    }

    /// Convert a drop-frame **label** back into the continuous frame index.
    ///
    /// Labels that drop-frame never produces are mapped to the nearest valid
    /// continuous index.
    #[must_use]
    pub fn drop_label_to_continuous(self, label: i64) -> i64 {
        if !self.drop_frame || !self.supports_drop_frame() {
            return label;
        }
        let drop = self.drop_frames_per_minute();
        let per10 = self.continuous_frames_per_10min();
        let per_min = self.continuous_frames_per_minute();
        let d = label.div_euclid(per10);
        let m = label.rem_euclid(per10);
        let mut cont = label - 9 * drop * d;
        if m >= drop {
            cont -= drop * ((m - drop) / per_min);
        }
        cont
    }

    /// Exact timestamp of a drop-frame label.
    #[must_use]
    pub fn frame_to_time_drop(self, label: i64) -> Time {
        self.frame_to_time(self.drop_label_to_continuous(label))
    }

    /// Floor a time to a frame index (linear numbering).
    #[must_use]
    pub fn time_to_frame_floor(self, t: Time) -> i64 {
        let scaled = (t.as_nanos() as i128) * self.fps.num as i128;
        let denom = self.fps.den as i128 * NANOS_PER_SEC as i128;
        div_floor(scaled, denom) as i64
    }

    /// Exact fractional frame position.
    #[must_use]
    pub fn time_to_frame_exact(self, t: Time) -> f64 {
        (t.as_nanos() as f64) * self.fps.num as f64 / (self.fps.den as f64 * NANOS_PER_SEC as f64)
    }

    /// Frame index whose timestamp is closest to `t`.
    #[must_use]
    pub fn time_to_frame_nearest(self, t: Time) -> i64 {
        let scaled = (t.as_nanos() as i128) * self.fps.num as i128;
        let denom = self.fps.den as i128 * NANOS_PER_SEC as i128;
        div_round_nearest(scaled, denom) as i64
    }

    /// Number of frames in a duration (may be fractional-aware rounding down).
    #[must_use]
    pub fn duration_to_frames(self, d: Time) -> i64 {
        self.time_to_frame_floor(d)
    }
}

impl Default for Timebase {
    fn default() -> Self {
        Timebase::from_fps(30, 1)
    }
}

#[inline]
fn clamp_i128(v: i128) -> i64 {
    if v > i64::MAX as i128 {
        i64::MAX
    } else if v < i64::MIN as i128 {
        i64::MIN
    } else {
        v as i64
    }
}

#[inline]
fn div_floor(a: i128, b: i128) -> i128 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

#[inline]
fn div_ceil(a: i128, b: i128) -> i128 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) == (b < 0)) {
        q + 1
    } else {
        q
    }
}

/// Round `a / b` to the nearest integer, ties away from zero.
#[inline]
fn div_round_nearest(a: i128, b: i128) -> i128 {
    let q = div_floor(a, b);
    let rem = a - q * b; // 0 <= rem < b for b > 0
    let twice = rem * 2;
    if twice > b {
        q + 1
    } else if twice < b {
        q
    } else if a >= 0 {
        q + 1
    } else {
        q
    }
}

/// A half-open time interval `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TimeRange {
    /// Inclusive start.
    pub start: Time,
    /// Exclusive end.
    pub end: Time,
}

impl TimeRange {
    /// Construct a range; `end` is clamped to be `>= start`.
    #[must_use]
    pub fn new(start: Time, end: Time) -> Self {
        TimeRange {
            start,
            end: end.max(start),
        }
    }

    /// A zero-length range at `at`.
    #[must_use]
    pub fn point(at: Time) -> Self {
        TimeRange { start: at, end: at }
    }

    /// Length of the range.
    #[must_use]
    pub fn duration(self) -> Time {
        self.end - self.start
    }

    /// Whether the range is empty.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.end <= self.start
    }

    /// Whether `t` lies inside the half-open interval.
    #[must_use]
    pub fn contains(self, t: Time) -> bool {
        t >= self.start && t < self.end
    }

    /// Whether the range overlaps `other`.
    #[must_use]
    pub fn overlaps(self, other: TimeRange) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Overlap of two ranges, if any.
    #[must_use]
    pub fn intersect(self, other: TimeRange) -> Option<TimeRange> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        if end > start {
            Some(TimeRange { start, end })
        } else {
            None
        }
    }

    /// Shift the range by `delta`.
    #[must_use]
    pub fn shifted(self, delta: Time) -> TimeRange {
        TimeRange {
            start: self.start + delta,
            end: self.end + delta,
        }
    }
}

/// Ordering helper for sorting by time then stable id.
#[inline]
pub fn cmp_time_then<T, F: Fn(&T) -> Time>(a: &T, b: &T, f: F) -> Ordering {
    f(a).cmp(&f(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_is_normalized() {
        let r = Rational::new(60000, 1001).unwrap();
        assert_eq!((r.num(), r.den()), (60000, 1001));
        let r = Rational::new(30, 0);
        assert!(r.is_none());
        let r = Rational::new(-30, 60).unwrap();
        assert_eq!((r.num(), r.den()), (-1, 2));
    }

    #[test]
    fn frame_roundtrip_2997() {
        let tb = Timebase::from_fps(30000, 1001);
        for f in [0i64, 1, 29, 30, 1000, 17981, 17982, 100_000] {
            let t = tb.frame_to_time(f);
            assert_eq!(tb.time_to_frame_floor(t), f, "frame {f}");
        }
    }

    #[test]
    fn frame_roundtrip_24() {
        let tb = Timebase::from_fps(24, 1);
        let t = tb.frame_to_time(48);
        assert_eq!(t.as_secs_f64(), 2.0);
        assert_eq!(tb.time_to_frame_nearest(t), 48);
    }

    #[test]
    fn time_display() {
        assert_eq!(Time::from_secs(1).to_string(), "00:00:01.000");
        assert_eq!(Time::from_millis(1500).to_string(), "00:00:01.500");
        assert_eq!(Time::from_secs(-2).to_string(), "-00:00:02.000");
        assert_eq!(Time::from_secs(3661).to_string(), "01:01:01.000");
    }

    #[test]
    fn ranges_intersect() {
        let a = TimeRange::new(Time::from_secs(0), Time::from_secs(10));
        let b = TimeRange::new(Time::from_secs(5), Time::from_secs(15));
        let i = a.intersect(b).unwrap();
        assert_eq!(i.start, Time::from_secs(5));
        assert_eq!(i.end, Time::from_secs(10));
        assert!(!a.overlaps(TimeRange::new(Time::from_secs(10), Time::from_secs(20))));
    }

    #[test]
    fn drop_frame_skips_two_labels_per_minute() {
        let tb = Timebase::with_drop_frame(Rational::new(30000, 1001).unwrap(), true);
        // 17982 continuous frames == 10 minutes of wall clock == 17982 labels.
        assert_eq!(tb.continuous_frames_per_10min(), 17982);
        assert_eq!(tb.continuous_frames_per_minute(), 1798);
        // Labels 00:01:00;00 (1800) and 00:01:00;01 (1801) are skipped.
        assert_eq!(tb.continuous_to_drop_label(1797), 1797);
        assert_eq!(tb.continuous_to_drop_label(1798), 1798);
        assert_eq!(tb.continuous_to_drop_label(1799), 1799);
        assert_eq!(tb.continuous_to_drop_label(1800), 1802);
        // Round trip through the label space is stable for valid labels.
        for c in [
            0i64, 1, 1797, 1798, 1799, 1800, 1801, 1802, 17981, 17982, 35964,
        ] {
            let label = tb.continuous_to_drop_label(c);
            assert_eq!(
                tb.drop_label_to_continuous(label),
                c,
                "label {label} for c {c}"
            );
        }
    }

    #[test]
    fn drop_frame_time_matches_wall_clock_over_ten_minutes() {
        let tb = Timebase::with_drop_frame(Rational::new(30000, 1001).unwrap(), true);
        let c = tb.continuous_frames_per_10min();
        let t = tb.frame_to_time(c);
        // 17982 frames at 29.97 fps is 599.9994 s of wall clock.
        let err = (t.as_nanos() as f64 - 600.0 * NANOS_PER_SEC as f64).abs();
        assert!(err < 1_000_000.0, "off by {err} ns");
    }

    #[test]
    fn invalid_rational_rejected() {
        assert!(Rational::new(1, 0).is_none());
        assert!(Rational::new_const(1, 1).num() == 1);
    }
}
