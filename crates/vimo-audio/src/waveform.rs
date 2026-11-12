//! Waveform generation and caching.
//!
//! The timeline draws waveforms for every visible clip. Computing them from
//! the decoded audio is expensive, so the peaks are computed once and cached.
//! The cache is keyed by source and resolution.

use vimo_foundation::map::FxHashMap;
use vimo_foundation::time::Time;

use crate::mixer::Peak;

/// The resolution of a waveform (samples per bucket).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WaveformResolution {
    /// Samples per peak bucket.
    pub samples_per_bucket: u32,
}

impl WaveformResolution {
    /// A resolution.
    #[must_use]
    pub fn new(samples_per_bucket: u32) -> Self {
        WaveformResolution { samples_per_bucket: if samples_per_bucket == 0 { 1 } else { samples_per_bucket } }
    }
}

/// The default resolution used for the timeline overview.
pub const DEFAULT_RESOLUTION: WaveformResolution = WaveformResolution { samples_per_bucket: 512 };

/// A cached waveform.
#[derive(Debug, Clone, PartialEq)]
pub struct Waveform {
    /// The resolution it was computed at.
    pub resolution: WaveformResolution,
    /// The peaks, in order.
    pub peaks: Vec<Peak>,
    /// The duration the waveform covers.
    pub duration: Time,
}

impl Waveform {
    /// Compute the waveform of a block of interleaved samples.
    #[must_use]
    pub fn compute(
        samples: &[f32],
        channels: u16,
        resolution: WaveformResolution,
        duration: Time,
    ) -> Self {
        let ch = channels.max(1) as usize;
        let frames = samples.len() / ch;
        let mut peaks = Vec::with_capacity(frames / resolution.samples_per_bucket as usize + 1);
        let mut i = 0usize;
        while i < frames {
            let end = (i + resolution.samples_per_bucket as usize).min(frames);
            let mut peak = Peak { min: 0.0, max: 0.0 };
            let mut first = true;
            for f in i..end {
                for c in 0..ch {
                    let v = samples[f * ch + c];
                    if first {
                        peak = Peak { min: v, max: v };
                        first = false;
                    } else {
                        peak = peak.merge(Peak { min: v, max: v });
                    }
                }
            }
            if first {
                peak = Peak { min: 0.0, max: 0.0 };
            }
            peaks.push(peak);
            i = end;
        }
        Waveform { resolution, peaks, duration }
    }

    /// The number of buckets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.peaks.len()
    }

    /// Whether the waveform is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.peaks.is_empty()
    }
}

/// A cache of computed waveforms.
#[derive(Debug, Default)]
pub struct WaveformCache {
    entries: FxHashMap<(u64, u32), Waveform>,
}

impl WaveformCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        WaveformCache::default()
    }

    /// Look up a waveform.
    #[must_use]
    pub fn get(&self, source: u64, resolution: WaveformResolution) -> Option<&Waveform> {
        self.entries.get(&(source, resolution.samples_per_bucket))
    }

    /// Insert a waveform.
    pub fn insert(&mut self, source: u64, waveform: Waveform) {
        self.entries.insert((source, waveform.resolution.samples_per_bucket), waveform);
    }

    /// Remove every waveform of a source.
    pub fn remove_source(&mut self, source: u64) {
        self.entries.retain(|(s, _), _| *s != source);
    }

    /// Drop every waveform.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The number of cached waveforms.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The approximate memory used by the cache, in bytes.
    #[must_use]
    pub fn memory_bytes(&self) -> usize {
        self.entries
            .values()
            .map(|w| w.peaks.len() * core::mem::size_of::<Peak>())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_buckets_the_samples() {
        let samples: Vec<f32> = (0..1000).map(|i| (i % 100) as f32 / 100.0).collect();
        let w = Waveform::compute(&samples, 1, WaveformResolution::new(100), Time::from_secs(1));
        assert_eq!(w.len(), 10);
        assert!(w.peaks[0].max > 0.0);
    }

    #[test]
    fn stereo_is_mixed_into_one_peak() {
        let samples = vec![1.0, -1.0, 0.5, -0.5];
        let w = Waveform::compute(&samples, 2, WaveformResolution::new(1), Time::ZERO);
        assert_eq!(w.len(), 2);
        assert_eq!(w.peaks[0].min, -1.0);
        assert_eq!(w.peaks[0].max, 1.0);
    }

    #[test]
    fn cache_roundtrips() {
        let mut c = WaveformCache::new();
        let w = Waveform::compute(&[0.0, 1.0], 1, DEFAULT_RESOLUTION, Time::ZERO);
        c.insert(7, w);
        assert!(c.get(7, DEFAULT_RESOLUTION).is_some());
        assert!(c.get(8, DEFAULT_RESOLUTION).is_none());
        c.remove_source(7);
        assert!(c.get(7, DEFAULT_RESOLUTION).is_none());
    }
}
