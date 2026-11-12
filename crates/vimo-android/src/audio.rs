//! The low-latency audio output.
//!
//! Oboe is a C++ library, so the platform layer talks to it through a small C
//! shim (`vimo_oboe.h` / `vimo_oboe.cpp`, built by the NDK's CMake). The shim
//! keeps the JNI and Rust sides free of C++ and gives the audio callback a
//! plain C entry point.

use std::ffi::c_void;

use vimo_audio::output::{AudioBlock, AudioOutput};
use vimo_foundation::error::{ErrorKind, VimoError, VimoResult};
use vimo_foundation::time::Time;

/// The C shim's opaque handle.
pub type VimoAudio = c_void;

extern "C" {
    /// `vimo_audio_create`
    fn vimo_audio_create(sample_rate: i32, channels: i32, frames_per_callback: i32)
        -> *mut VimoAudio;
    /// `vimo_audio_destroy`
    fn vimo_audio_destroy(audio: *mut VimoAudio);
    /// `vimo_audio_start`
    fn vimo_audio_start(audio: *mut VimoAudio) -> i32;
    /// `vimo_audio_stop`
    fn vimo_audio_stop(audio: *mut VimoAudio);
    /// `vimo_audio_write`
    fn vimo_audio_write(audio: *mut VimoAudio, data: *const f32, frames: i32) -> i32;
    /// `vimo_audio_latency_frames`
    fn vimo_audio_latency_frames(audio: *mut VimoAudio) -> i32;
    /// `vimo_audio_position_frames`
    fn vimo_audio_position_frames(audio: *mut VimoAudio) -> i64;
}

/// An Oboe-backed audio output.
pub struct OboeAudioOutput {
    /// The shim handle, stored as an integer so the type stays `Send`.
    audio: usize,
    sample_rate: u32,
    channels: u16,
    frames: usize,
    running: bool,
    /// The number of frames that were written.
    written: u64,
}

impl OboeAudioOutput {
    /// Create the output.
    ///
    /// `frames_per_callback` should be small (128-256) for low latency.
    pub fn new(sample_rate: u32, channels: u16, frames_per_callback: usize) -> VimoResult<Self> {
        let audio = unsafe {
            vimo_audio_create(
                sample_rate as i32,
                channels as i32,
                frames_per_callback as i32,
            )
        };
        if audio.is_null() {
            return Err(VimoError::detailed(
                ErrorKind::Graphics,
                "oboe",
                "could not create the audio stream",
            ));
        }
        Ok(OboeAudioOutput {
            audio: audio as usize,
            sample_rate,
            channels: channels.max(1),
            frames: frames_per_callback.max(32),
            running: false,
            written: 0,
        })
    }

    /// How far the device has played, in frames.
    #[must_use]
    pub fn position_frames(&self) -> i64 {
        unsafe { vimo_audio_position_frames(self.audio as *mut VimoAudio) }
    }

    /// The position the device has played, as a timeline time.
    #[must_use]
    pub fn position(&self) -> Time {
        Time::from_secs_f64(self.position_frames() as f64 / self.sample_rate as f64)
    }

    /// The output latency, in frames.
    #[must_use]
    pub fn latency_frames(&self) -> i32 {
        unsafe { vimo_audio_latency_frames(self.audio as *mut VimoAudio) }
    }
}

impl Drop for OboeAudioOutput {
    fn drop(&mut self) {
        unsafe {
            if self.running {
                vimo_audio_stop(self.audio as *mut VimoAudio);
            }
            vimo_audio_destroy(self.audio as *mut VimoAudio);
        }
    }
}

impl AudioOutput for OboeAudioOutput {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn preferred_frames(&self) -> usize {
        self.frames
    }

    fn start(&mut self) -> VimoResult<()> {
        if self.running {
            return Ok(());
        }
        let result = unsafe { vimo_audio_start(self.audio as *mut VimoAudio) };
        if result != 0 {
            return Err(VimoError::detailed(
                ErrorKind::Graphics,
                "oboe",
                format!("could not start the audio stream ({result})"),
            ));
        }
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) {
        if !self.running {
            return;
        }
        unsafe { vimo_audio_stop(self.audio as *mut VimoAudio) };
        self.running = false;
    }

    fn push(&mut self, block: &AudioBlock) -> VimoResult<()> {
        if !self.running {
            return Err(vimo_foundation::error::invalid_state("oboe: the stream is not running"));
        }
        let frames = block.frame_count() as i32;
        if frames <= 0 {
            return Ok(());
        }
        let written = unsafe {
            vimo_audio_write(self.audio as *mut VimoAudio, block.samples.as_ptr(), frames)
        };
        if written < 0 {
            return Err(VimoError::detailed(
                ErrorKind::Graphics,
                "oboe",
                format!("the audio callback failed ({written})"),
            ));
        }
        self.written += written as u64;
        Ok(())
    }
}
