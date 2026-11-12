//! # vimo-audio
//!
//! The Vimo audio engine: multi-track mixing, gain envelopes and fades,
//! sample-rate conversion, waveform generation and a low-latency output
//! abstraction.
//!
//! Everything in the audio path is allocation free once the buffers have been
//! allocated: the mixer writes into a caller-owned buffer and the resampler
//! keeps its state between calls.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod envelope;
pub mod mixer;
pub mod output;
pub mod resampler;
pub mod waveform;

pub use envelope::{AutomationCurve, FadeCurve, GainEnvelope};
pub use mixer::{peak_of, MixSource, MixSourceRef, Mixer, Pan, Peak};
pub use output::{AudioBlock, AudioOutput, NullAudioOutput};
pub use resampler::{Resampler, ResamplerQuality};
pub use waveform::{Waveform, WaveformCache, WaveformResolution, DEFAULT_RESOLUTION};
