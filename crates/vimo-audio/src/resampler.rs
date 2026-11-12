//! Sample-rate conversion.
//!
//! Media is often 44.1 kHz while the device's audio output runs at 48 kHz.
//! The resampler converts between them without the audible artefacts of naive
//! nearest-neighbour sampling.
//!
//! A 4-point cubic (Catmull-Rom) interpolator is used: it is cheap enough to
//! run in the audio callback (a handful of operations per sample) and
//! noticeably better than linear interpolation.
//!
//! The resampler is **streaming**: it keeps the fractional read position and
//! a short history between calls, so converting block by block produces the
//! same output as converting everything at once.

/// The interpolation quality of a resampler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResamplerQuality {
    /// Nearest neighbour: cheap, aliases.
    Nearest,
    /// Linear interpolation.
    Linear,
    /// 4-point cubic (Catmull-Rom).
    Cubic,
}

/// Converts PCM between sample rates.
#[derive(Debug, Clone)]
pub struct Resampler {
    input_rate: u32,
    output_rate: u32,
    quality: ResamplerQuality,
    /// The absolute fractional read position, in input frames.
    in_pos: f64,
    /// The absolute index of the first frame of the next block.
    consumed: u64,
    /// The channel count the resampler is configured for.
    channels: u16,
    /// The last four frames of the previous block.
    prev: Vec<f32>,
}

impl Resampler {
    /// A resampler converting from `input_rate` to `output_rate`.
    #[must_use]
    pub fn new(input_rate: u32, output_rate: u32, quality: ResamplerQuality) -> Self {
        Resampler {
            input_rate,
            output_rate,
            quality,
            in_pos: 0.0,
            consumed: 0,
            channels: 0,
            prev: Vec::new(),
        }
    }

    /// The input sample rate.
    #[must_use]
    pub fn input_rate(&self) -> u32 {
        self.input_rate
    }

    /// The output sample rate.
    #[must_use]
    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }

    /// The resampling ratio.
    #[must_use]
    pub fn ratio(&self) -> f64 {
        if self.input_rate == 0 {
            return 1.0;
        }
        self.output_rate as f64 / self.input_rate as f64
    }

    /// Reset the internal state.
    pub fn reset(&mut self) {
        self.in_pos = 0.0;
        self.consumed = 0;
        self.prev.clear();
    }

    /// Convert `input` into `output`, appending to the existing content.
    pub fn process(&mut self, input: &[f32], channels: u16, output: &mut Vec<f32>) {
        let ch = channels as usize;
        if self.input_rate == self.output_rate
            || self.input_rate == 0
            || self.output_rate == 0
            || ch == 0
        {
            output.extend_from_slice(input);
            return;
        }
        if self.channels != channels {
            self.channels = channels;
            self.prev = vec![0.0; 4 * ch];
            self.in_pos = 0.0;
            self.consumed = 0;
        }
        let frames = input.len() / ch;
        if frames == 0 {
            return;
        }

        let block_start = self.consumed;
        let block_end = block_start + frames as u64;
        let step = self.input_rate as f64 / self.output_rate as f64;

        // We can only read a frame when its two-frame lookahead is available.
        while self.in_pos + 2.0 <= (block_end as f64 - 1.0) {
            let f = self.in_pos.floor() as i64;
            let frac = (self.in_pos - f as f64) as f32;
            for c in 0..ch {
                let x0 = sample(input, &self.prev, ch, block_start, block_end, f - 1, c);
                let x1 = sample(input, &self.prev, ch, block_start, block_end, f, c);
                let x2 = sample(input, &self.prev, ch, block_start, block_end, f + 1, c);
                let x3 = sample(input, &self.prev, ch, block_start, block_end, f + 2, c);
                output.push(match self.quality {
                    ResamplerQuality::Nearest => x1,
                    ResamplerQuality::Linear => x1 + (x2 - x1) * frac,
                    ResamplerQuality::Cubic => cubic_interpolate(x0, x1, x2, x3, frac),
                });
            }
            self.in_pos += step;
        }

        // Keep the last four frames so the next block continues seamlessly.
        let mut new_prev = Vec::with_capacity(4 * ch);
        for i in 0..4i64 {
            let f = block_end as i64 - 4 + i;
            for c in 0..ch {
                new_prev.push(sample(input, &self.prev, ch, block_start, block_end, f, c));
            }
        }
        self.prev = new_prev;
        self.consumed = block_end;
    }
}

/// Read the sample at absolute frame `f`, channel `c`.
fn sample(
    input: &[f32],
    prev: &[f32],
    ch: usize,
    block_start: u64,
    block_end: u64,
    f: i64,
    c: usize,
) -> f32 {
    if f < 0 {
        return 0.0;
    }
    let abs = f as u64;
    if abs < block_start {
        // In the previous block's history (the last four frames).
        let idx = abs + 4 - block_start;
        return prev.get((idx as usize) * ch + c).copied().unwrap_or(0.0);
    }
    if abs < block_end {
        let idx = (abs - block_start) as usize;
        return input.get(idx * ch + c).copied().unwrap_or(0.0);
    }
    // Past the end of the block: clamp to the last frame.
    let idx = (block_end - block_start) as usize - 1;
    input.get(idx * ch + c).copied().unwrap_or(0.0)
}

/// Catmull-Rom interpolation between `x1` and `x2`.
#[inline]
fn cubic_interpolate(x0: f32, x1: f32, x2: f32, x3: f32, t: f32) -> f32 {
    let a0 = -0.5 * x0 + 1.5 * x1 - 1.5 * x2 + 0.5 * x3;
    let a1 = x0 - 2.5 * x1 + 2.0 * x2 - 0.5 * x3;
    let a2 = -0.5 * x0 + 0.5 * x2;
    let a3 = x1;
    ((a0 * t + a1) * t + a2) * t + a3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_when_rates_match() {
        let mut r = Resampler::new(48000, 48000, ResamplerQuality::Cubic);
        let mut out = Vec::new();
        r.process(&[1.0, 2.0, 3.0, 4.0], 1, &mut out);
        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn upsampling_produces_more_samples() {
        let mut r = Resampler::new(44100, 48000, ResamplerQuality::Linear);
        let input: Vec<f32> = (0..480).map(|i| (i as f32 / 480.0).sin()).collect();
        let mut out = Vec::new();
        r.process(&input, 1, &mut out);
        // 480 frames at 44.1 kHz become ~522 frames at 48 kHz.
        assert!((500..545).contains(&out.len()), "got {}", out.len());
    }

    #[test]
    fn downsampling_produces_fewer_samples() {
        let mut r = Resampler::new(48000, 24000, ResamplerQuality::Linear);
        let input: Vec<f32> = (0..480).map(|i| (i as f32 / 480.0).sin()).collect();
        let mut out = Vec::new();
        r.process(&input, 1, &mut out);
        assert!((220..260).contains(&out.len()), "got {}", out.len());
    }

    #[test]
    fn block_by_block_matches_one_shot() {
        let input: Vec<f32> = (0..1000).map(|i| (i as f32 / 100.0).sin()).collect();

        let mut one = Resampler::new(44100, 48000, ResamplerQuality::Cubic);
        let mut whole = Vec::new();
        one.process(&input, 1, &mut whole);

        let mut many = Resampler::new(44100, 48000, ResamplerQuality::Cubic);
        let mut chunked = Vec::new();
        for block in input.chunks(128) {
            many.process(block, 1, &mut chunked);
        }

        assert_eq!(whole.len(), chunked.len(), "lengths differ");
        for (a, b) in whole.iter().zip(chunked.iter()) {
            assert!((a - b).abs() < 1e-3, "values differ: {a} vs {b}");
        }
    }

    #[test]
    fn stereo_channels_stay_separated() {
        let mut r = Resampler::new(48000, 96000, ResamplerQuality::Linear);
        let input = vec![1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
        let mut out = Vec::new();
        r.process(&input, 2, &mut out);
        for chunk in out.chunks(2) {
            assert!((chunk[0] - 1.0).abs() < 1e-5, "left={}", chunk[0]);
            assert!((chunk[1] + 1.0).abs() < 1e-5, "right={}", chunk[1]);
        }
    }
}
