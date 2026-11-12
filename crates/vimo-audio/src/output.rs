//! Audio output.
//!
//! The core fills a buffer; the platform (Oboe on Android, ALSA/CPAL on the
//! desktop) plays it. The callback must never allocate or block, so the
//! mixing buffer is owned by the engine and reused.

use vimo_foundation::time::Time;

/// A block of audio to play.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBlock {
    /// Interleaved samples in `[-1, 1]`.
    pub samples: Vec<f32>,
    /// The number of channels.
    pub channels: u16,
    /// The sample rate.
    pub sample_rate: u32,
    /// The timeline position of the first sample.
    pub start: Time,
}

impl AudioBlock {
    /// An empty block.
    #[must_use]
    pub fn new(channels: u16, sample_rate: u32) -> Self {
        AudioBlock {
            samples: Vec::new(),
            channels,
            sample_rate,
            start: Time::ZERO,
        }
    }

    /// The number of sample frames.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / self.channels as usize
        }
    }

    /// The duration of the block.
    #[must_use]
    pub fn duration(&self) -> Time {
        if self.sample_rate == 0 {
            return Time::ZERO;
        }
        Time::from_secs_f64(self.frame_count() as f64 / self.sample_rate as f64)
    }
}

/// A low-latency audio sink.
pub trait AudioOutput: Send {
    /// The sample rate the sink runs at.
    fn sample_rate(&self) -> u32;
    /// The number of channels the sink expects.
    fn channels(&self) -> u16;
    /// The number of frames the sink wants per callback.
    fn preferred_frames(&self) -> usize;
    /// Start the sink.
    fn start(&mut self) -> vimo_foundation::VimoResult<()>;
    /// Stop the sink.
    fn stop(&mut self);
    /// Push a block of audio.
    fn push(&mut self, block: &AudioBlock) -> vimo_foundation::VimoResult<()>;
}

/// An output that discards everything, for tests and for muted sessions.
#[derive(Debug, Default)]
pub struct NullAudioOutput {
    sample_rate: u32,
    channels: u16,
    frames: usize,
    pushed: usize,
}

impl NullAudioOutput {
    /// A sink with the given format.
    #[must_use]
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        NullAudioOutput {
            sample_rate,
            channels,
            frames: 256,
            pushed: 0,
        }
    }

    /// The number of blocks that were pushed.
    #[must_use]
    pub fn pushed(&self) -> usize {
        self.pushed
    }
}

impl AudioOutput for NullAudioOutput {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn preferred_frames(&self) -> usize {
        self.frames
    }
    fn start(&mut self) -> vimo_foundation::VimoResult<()> {
        Ok(())
    }
    fn stop(&mut self) {}
    fn push(&mut self, _block: &AudioBlock) -> vimo_foundation::VimoResult<()> {
        self.pushed += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_duration() {
        let mut b = AudioBlock::new(2, 48000);
        b.samples = vec![0.0; 960];
        assert_eq!(b.frame_count(), 480);
        assert!((b.duration().as_secs_f64() - 0.01).abs() < 1e-9);
    }

    #[test]
    fn null_output_counts_pushes() {
        let mut o = NullAudioOutput::new(48000, 2);
        o.start().unwrap();
        o.push(&AudioBlock::new(2, 48000)).unwrap();
        assert_eq!(o.pushed(), 1);
    }
}
