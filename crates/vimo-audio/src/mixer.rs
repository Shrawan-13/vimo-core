//! Multi-track mixing.
//!
//! The mixer sums the audio of every clip that is active at a given time,
//! applying per-clip gain, fades and pan, then applies the track and master
//! gain. Mixing is done into a caller-owned buffer so that the audio callback
//! never allocates.

use vimo_foundation::time::Time;

use crate::envelope::{AutomationCurve, GainEnvelope};

/// The stereo position of a source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pan {
    /// -1 = hard left, 0 = centre, 1 = hard right.
    pub value: f32,
}

impl Default for Pan {
    fn default() -> Self {
        Pan { value: 0.0 }
    }
}

impl Pan {
    /// The left gain.
    #[must_use]
    pub fn left_gain(&self) -> f32 {
        let p = self.value.clamp(-1.0, 1.0);
        if p <= 0.0 {
            1.0
        } else {
            1.0 - p
        }
    }

    /// The right gain.
    #[must_use]
    pub fn right_gain(&self) -> f32 {
        let p = self.value.clamp(-1.0, 1.0);
        if p >= 0.0 {
            1.0
        } else {
            1.0 + p
        }
    }
}

/// One source to be mixed.
#[derive(Debug, Clone)]
pub struct MixSource {
    /// The samples, interleaved.
    pub samples: Vec<f32>,
    /// The number of channels in `samples`.
    pub channels: u16,
    /// The position in the timeline of the first sample.
    pub start: Time,
    /// The gain applied to the source.
    pub gain: f32,
    /// The pan applied to the source.
    pub pan: Pan,
    /// The fade envelope.
    pub envelope: GainEnvelope,
    /// An optional automation curve.
    pub automation: Option<AutomationCurve>,
    /// Whether the source is muted.
    pub muted: bool,
}

impl MixSource {
    /// A source with default settings.
    #[must_use]
    pub fn new(samples: Vec<f32>, channels: u16, start: Time) -> Self {
        MixSource {
            samples,
            channels,
            start,
            gain: 1.0,
            pan: Pan::default(),
            envelope: GainEnvelope::default(),
            automation: None,
            muted: false,
        }
    }
}

/// A borrowed view of a mix source.
///
/// The engine mixes straight out of the decoder's scratch buffer, so the
/// samples are never copied.
#[derive(Debug, Clone, Copy)]
pub struct MixSourceRef<'a> {
    /// The interleaved samples.
    pub samples: &'a [f32],
    /// The number of channels in `samples`.
    pub channels: u16,
    /// The position in the timeline of the first sample.
    pub start: Time,
    /// The gain applied to the source.
    pub gain: f32,
    /// The pan applied to the source.
    pub pan: Pan,
    /// The fade envelope.
    pub envelope: GainEnvelope,
    /// An optional automation curve.
    pub automation: Option<&'a AutomationCurve>,
    /// Whether the source is muted.
    pub muted: bool,
}

impl MixSourceRef<'_> {
    /// Borrow an owned source.
    #[must_use]
    pub fn of(source: &MixSource) -> MixSourceRef<'_> {
        MixSourceRef {
            samples: &source.samples,
            channels: source.channels,
            start: source.start,
            gain: source.gain,
            pan: source.pan,
            envelope: source.envelope,
            automation: source.automation.as_ref(),
            muted: source.muted,
        }
    }
}

/// Mixes sources into an output buffer.
#[derive(Debug, Default)]
pub struct Mixer {
    /// The master gain.
    pub master_gain: f32,
    /// The number of output channels.
    pub output_channels: u16,
}

impl Mixer {
    /// A mixer with the given output channel count.
    #[must_use]
    pub fn new(output_channels: u16) -> Self {
        Mixer { master_gain: 1.0, output_channels: output_channels.max(1) }
    }

    /// Mix `sources` into `output`.
    ///
    /// `output` is *added to*, not overwritten, so the caller can accumulate
    /// several mixers. `timeline_time` is the position of the first output
    /// sample.
    pub fn mix(&self, sources: &[MixSource], timeline_time: Time, output: &mut [f32]) {
        for source in sources {
            self.mix_one(MixSourceRef::of(source), timeline_time, output);
        }
    }

    /// Mix borrowed sources into an output buffer.
    ///
    /// Takes an iterator so that the caller does not have to build a
    /// temporary `Vec` of views.
    pub fn mix_refs<'a, I>(&self, sources: I, timeline_time: Time, output: &mut [f32])
    where
        I: IntoIterator<Item = MixSourceRef<'a>>,
    {
        for source in sources {
            self.mix_one(source, timeline_time, output);
        }
    }

    /// Mix a single source into the output.
    fn mix_one(&self, source: MixSourceRef<'_>, timeline_time: Time, output: &mut [f32]) {
        let out_channels = self.output_channels as usize;
        let master = self.master_gain;
        if source.muted || source.channels == 0 {
            return;
        }
        let in_channels = source.channels as usize;
        let frames = source.samples.len() / in_channels;
        if frames == 0 {
            return;
        }
        // Where the source starts relative to the output window.
        let offset = (source.start - timeline_time).as_nanos();
        let start_frame = if offset >= 0 {
            (offset / 1_000_000_000) * in_channels as i64
        } else {
            -((-offset) / 1_000_000_000) * in_channels as i64
        };
        // Skip the part of the source that lies before the window.
        let skip = if start_frame < 0 { (-start_frame) as usize } else { 0 };
        let base = start_frame.max(0) as usize;
        let span = Time::from_nanos((frames as i64) * 1_000_000_000 / 44_100);

        for f in skip..frames {
            let out_index = base + (f - skip) * in_channels;
            if out_index + in_channels > output.len() {
                break;
            }
            let pos = Time::from_nanos((f as i64) * 1_000_000_000 / 44_100);
            let mut g = source.gain * source.envelope.gain_at(pos, span);
            if let Some(curve) = &source.automation {
                g *= curve.gain_at(source.start + pos);
            }
            if g == 0.0 {
                continue;
            }
            let in_base = f * in_channels;
            let left = source.samples[in_base];
            let right = if in_channels > 1 {
                source.samples[in_base + 1]
            } else {
                left
            };
            let l = left * g * source.pan.left_gain();
            let r = right * g * source.pan.right_gain();
            output[out_index] += l * master;
            if out_channels > 1 {
                output[out_index + 1] += r * master;
            }
        }
    }
}

/// Peak detection over a block of samples.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Peak {
    /// The minimum sample value in the block.
    pub min: f32,
    /// The maximum sample value in the block.
    pub max: f32,
}

impl Peak {
    /// Combine two peaks.
    #[must_use]
    pub fn merge(self, other: Peak) -> Peak {
        Peak { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    /// The peak-to-peak amplitude.
    #[must_use]
    pub fn amplitude(&self) -> f32 {
        self.max - self.min
    }
}

/// Computes the peak of a block of samples.
#[must_use]
pub fn peak_of(samples: &[f32]) -> Peak {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for s in samples {
        if *s < min {
            min = *s;
        }
        if *s > max {
            max = *s;
        }
    }
    if samples.is_empty() {
        Peak { min: 0.0, max: 0.0 }
    } else {
        Peak { min, max }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixing_sums_sources() {
        let mixer = Mixer::new(2);
        let a = MixSource::new(vec![1.0, 1.0], 2, Time::ZERO);
        let b = MixSource::new(vec![0.5, 0.5], 2, Time::ZERO);
        let mut out = vec![0.0; 4];
        mixer.mix(&[a, b], Time::ZERO, &mut out);
        assert_eq!(out, vec![1.5, 1.5, 0.0, 0.0]);
    }

    #[test]
    fn muted_sources_are_skipped() {
        let mixer = Mixer::new(2);
        let mut a = MixSource::new(vec![1.0, 1.0], 2, Time::ZERO);
        a.muted = true;
        let mut out = vec![0.0; 2];
        mixer.mix(&[a], Time::ZERO, &mut out);
        assert_eq!(out, vec![0.0, 0.0]);
    }

    #[test]
    fn pan_attenuates_the_opposite_channel() {
        let mixer = Mixer::new(2);
        let mut a = MixSource::new(vec![1.0, 1.0], 2, Time::ZERO);
        a.pan = Pan { value: 1.0 };
        let mut out = vec![0.0; 2];
        mixer.mix(&[a], Time::ZERO, &mut out);
        assert!(out[0] < 0.01);
        assert!(out[1] > 0.99);
    }

    #[test]
    fn peak_finds_extremes() {
        let p = peak_of(&[-1.0, 0.5, -0.25, 1.0]);
        assert_eq!(p.min, -1.0);
        assert_eq!(p.max, 1.0);
        assert_eq!(p.amplitude(), 2.0);
    }
}
