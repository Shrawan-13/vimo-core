//! Playback audio.
//!
//! The engine mixes the audio of every clip that is active at the playhead
//! and hands the result to the audio device. Mixing happens into a
//! caller-owned buffer, and the samples come from the platform through the
//! [`AudioSource`] trait, which fills a buffer rather than allocating one.
//!
//! The master clock is driven by the audio device: the audio callback reports
//! how far it has played, and the engine nudges the clock to follow it. That
//! is what keeps the video in sync with the sound.

use vimo_audio::{AudioBlock, MixSourceRef, Mixer, Pan, Resampler, ResamplerQuality};
use vimo_foundation::time::Time;
use vimo_project::model::{ClipId, TrackId};
use vimo_project::Project;

/// Supplies the audio of a clip.
pub trait AudioSource: Send + Sync {
    /// Fill `out` with the interleaved samples of `clip` starting at `time`.
    ///
    /// Returns the number of sample frames written, or `None` when the clip
    /// has no audio at that position. The implementation must not allocate.
    fn fill(&self, clip: ClipId, time: Time, out: &mut [f32]) -> Option<usize>;
}

/// An audio source that has no audio at all.
#[derive(Debug, Default)]
pub struct SilentAudioSource;

impl AudioSource for SilentAudioSource {
    fn fill(&self, _clip: ClipId, _time: Time, _out: &mut [f32]) -> Option<usize> {
        None
    }
}

/// The per-clip state the mixer needs.
struct ClipSlot {
    /// The decoded samples of this clip.
    samples: Vec<f32>,
    /// The gain of this clip.
    gain: f32,
}

/// Mixes the audio of the timeline.
pub struct PlaybackAudio {
    mixer: Mixer,
    resampler: Resampler,
    /// The scratch buffer a clip's samples are decoded into.
    scratch: Vec<f32>,
    /// The per-clip slots, reused between frames.
    slots: Vec<ClipSlot>,
    /// The sample rate of the output device.
    sample_rate: u32,
    /// The number of output channels.
    channels: u16,
    /// How many of the slots are in use this frame.
    slot_count: usize,
}

impl std::fmt::Debug for PlaybackAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlaybackAudio")
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .finish()
    }
}

impl PlaybackAudio {
    /// A mixer for an output device.
    #[must_use]
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        let channels = channels.max(1);
        PlaybackAudio {
            mixer: Mixer::new(channels),
            resampler: Resampler::new(sample_rate, sample_rate, ResamplerQuality::Cubic),
            scratch: Vec::new(),
            slots: Vec::new(),
            sample_rate,
            channels,
            slot_count: 0,
        }
    }

    /// The output sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The number of output channels.
    #[must_use]
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Render `frames` sample frames starting at `time` into `out`.
    ///
    /// `out.samples` is resized to `frames * channels`; the allocation is
    /// reused between calls.
    pub fn render(
        &mut self,
        project: &Project,
        source: &dyn AudioSource,
        time: Time,
        frames: usize,
        out: &mut AudioBlock,
    ) {
        let channels = self.channels as usize;
        out.samples.clear();
        out.samples.resize(frames * channels, 0.0);
        out.channels = self.channels;
        out.sample_rate = self.sample_rate;
        out.start = time;

        let soloed = project.tracks.iter().any(|t| t.alive && t.solo);
        let mut used = 0usize;

        for (track_index, track) in project.tracks.iter().enumerate() {
            if !track.alive || !track.enabled || track.muted {
                continue;
            }
            if soloed && !track.solo {
                continue;
            }
            let Some(clip_id) = project.active_clip(track_index as TrackId, time) else {
                continue;
            };
            let Some(clip) = project.clips.get(clip_id as usize) else {
                continue;
            };
            if clip.muted {
                continue;
            }
            let scratch_frames = frames * channels;
            if self.scratch.len() < scratch_frames {
                self.scratch.resize(scratch_frames, 0.0);
            }
            let source_time = source_time_for(clip, time);
            let Some(written) = source.fill(clip_id, source_time, &mut self.scratch) else {
                continue;
            };
            if written == 0 {
                continue;
            }
            // Reuse the slot's buffer so that steady-state mixing does not
            // allocate.
            let needed = written * channels;
            let slot_index = self.slots.len();
            if self.slots.len() <= slot_index {
                self.slots.push(ClipSlot {
                    samples: Vec::new(),
                    gain: 1.0,
                });
            }
            let slot = &mut self.slots[slot_index];
            if slot.samples.len() < needed {
                slot.samples.resize(needed, 0.0);
            }
            slot.samples[..needed].copy_from_slice(&self.scratch[..needed]);
            slot.gain = clip.volume * track.volume;
            used += 1;
        }
        self.slot_count = used;

        let channels = self.channels;
        let slots = &self.slots[..self.slot_count];
        self.mixer.mix_refs(
            slots.iter().map(|slot| MixSourceRef {
                samples: &slot.samples,
                channels,
                start: Time::ZERO,
                gain: slot.gain,
                pan: Pan::default(),
                envelope: Default::default(),
                automation: None,
                muted: false,
            }),
            Time::ZERO,
            &mut out.samples,
        );

        // Clamp so that a hot mix cannot wrap around.
        for s in &mut out.samples {
            *s = s.clamp(-1.0, 1.0);
        }
    }

    /// The resampler used when the media rate differs from the device rate.
    #[must_use]
    pub fn resampler(&mut self) -> &mut Resampler {
        &mut self.resampler
    }
}

/// The source-media time a clip shows at `time`.
fn source_time_for(clip: &vimo_project::model::Clip, time: Time) -> Time {
    let elapsed = time - clip.timeline.start;
    let speed = if clip.speed.abs() < f32::EPSILON {
        1.0
    } else {
        clip.speed
    };
    let delta = Time::from_secs_f64(elapsed.as_secs_f64() * speed as f64);
    if clip.forward {
        clip.source_in + delta
    } else {
        clip.source_in - delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vimo_foundation::time::TimeRange;
    use vimo_project::model::{Asset, Clip, Track, TrackKind};

    struct ConstantSource {
        value: f32,
    }

    impl AudioSource for ConstantSource {
        fn fill(&self, _clip: ClipId, _time: Time, out: &mut [f32]) -> Option<usize> {
            let frames = out.len() / 2;
            for f in 0..frames {
                out[f * 2] = self.value;
                out[f * 2 + 1] = self.value;
            }
            Some(frames)
        }
    }

    fn project() -> Project {
        let mut p = Project::new();
        p.assets.push(Asset::video("a", Time::from_secs(30)));
        let mut t = Track::new("V1", TrackKind::Video);
        t.clips.push(0);
        p.tracks.push(t);
        p.clips.push(Clip::new(
            0,
            TimeRange::new(Time::ZERO, Time::from_secs(5)),
            Time::ZERO,
        ));
        p
    }

    #[test]
    fn mixing_produces_the_expected_level() {
        let mut audio = PlaybackAudio::new(48_000, 2);
        let mut block = AudioBlock::new(2, 48_000);
        audio.render(
            &project(),
            &ConstantSource { value: 0.5 },
            Time::ZERO,
            256,
            &mut block,
        );
        assert_eq!(block.frame_count(), 256);
        assert!((block.samples[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn muted_clips_are_silent() {
        let mut p = project();
        p.clips[0].muted = true;
        let mut audio = PlaybackAudio::new(48_000, 2);
        let mut block = AudioBlock::new(2, 48_000);
        audio.render(
            &p,
            &ConstantSource { value: 0.5 },
            Time::ZERO,
            64,
            &mut block,
        );
        assert_eq!(block.samples[0], 0.0);
    }

    #[test]
    fn a_silent_source_produces_silence() {
        let mut audio = PlaybackAudio::new(48_000, 2);
        let mut block = AudioBlock::new(2, 48_000);
        audio.render(&project(), &SilentAudioSource, Time::ZERO, 64, &mut block);
        assert!(block.samples.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn solo_mutes_the_other_tracks() {
        let mut p = project();
        let mut other = Track::new("V2", TrackKind::Video);
        other.clips.push(0);
        p.tracks.push(other);
        p.tracks[0].solo = true;
        let mut audio = PlaybackAudio::new(48_000, 2);
        let mut block = AudioBlock::new(2, 48_000);
        audio.render(
            &p,
            &ConstantSource { value: 0.5 },
            Time::ZERO,
            64,
            &mut block,
        );
        // Two clips would sum to 1.0; solo leaves only one.
        assert!(
            (block.samples[0] - 0.5).abs() < 1e-6,
            "got {}",
            block.samples[0]
        );
    }
}
