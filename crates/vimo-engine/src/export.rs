//! The export pipeline.
//!
//! Export uses **the same render graph** as the preview, with three
//! differences: the resolution is the preset's, the scale is 1.0, and the
//! frames are produced at exact frame indices rather than at the playhead.
//! That makes the export deterministic: the same project and preset always
//! produce the same frames.
//!
//! The pipeline is *step driven* rather than thread driven, so the Android
//! layer can run it inside a foreground service and obey the platform's
//! lifecycle rules.

use vimo_foundation::error::{cancelled, invalid_argument, VimoResult};
use vimo_foundation::time::{Time, Timebase};
use vimo_media::{CodecId, Encoder};
use vimo_render::RenderGraph;

/// An export preset.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportPreset {
    /// The preset name.
    pub name: String,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Video bitrate in bits per second.
    pub bitrate: u32,
    /// Frame rate.
    pub fps: u32,
    /// The video codec.
    pub codec: CodecId,
    /// Audio bitrate in bits per second (0 disables audio).
    pub audio_bitrate: u32,
    /// Audio sample rate.
    pub sample_rate: u32,
}

impl ExportPreset {
    /// A 1080p preset.
    #[must_use]
    pub fn hd_1080p() -> Self {
        ExportPreset {
            name: "1080p".to_string(),
            width: 1920,
            height: 1080,
            bitrate: 12_000_000,
            fps: 30,
            codec: CodecId::H264,
            audio_bitrate: 192_000,
            sample_rate: 48_000,
        }
    }

    /// A 4K preset.
    #[must_use]
    pub fn uhd_2160p() -> Self {
        ExportPreset {
            name: "2160p".to_string(),
            bitrate: 45_000_000,
            ..ExportPreset::hd_1080p()
        }
    }

    /// A 720p preset for social media.
    #[must_use]
    pub fn hd_720p() -> Self {
        ExportPreset {
            name: "720p".to_string(),
            width: 1280,
            height: 720,
            bitrate: 6_000_000,
            fps: 30,
            codec: CodecId::H264,
            audio_bitrate: 128_000,
            sample_rate: 48_000,
        }
    }

    /// Validate the preset.
    pub fn validate(&self) -> VimoResult<()> {
        if self.width == 0 || self.height == 0 {
            return Err(invalid_argument("export preset: zero dimensions"));
        }
        if self.fps == 0 {
            return Err(invalid_argument("export preset: zero frame rate"));
        }
        if self.bitrate == 0 {
            return Err(invalid_argument("export preset: zero bitrate"));
        }
        Ok(())
    }

    /// The timebase the preset implies.
    #[must_use]
    pub fn timebase(&self) -> Timebase {
        Timebase::from_fps(self.fps as i64, 1)
    }
}

/// The state of an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportState {
    /// Not started.
    Idle,
    /// Running.
    Running,
    /// Paused; the export can be resumed.
    Paused,
    /// Finished.
    Completed,
    /// Cancelled by the user.
    Cancelled,
    /// Failed.
    Failed,
}

/// The progress of an export.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportProgress {
    /// The number of frames encoded so far.
    pub frame: u64,
    /// The total number of frames.
    pub total_frames: u64,
    /// The state.
    pub state: ExportState,
}

impl ExportProgress {
    /// The completion fraction in `0..=1`.
    #[must_use]
    pub fn fraction(&self) -> f32 {
        if self.total_frames == 0 {
            return 0.0;
        }
        (self.frame as f32 / self.total_frames as f32).clamp(0.0, 1.0)
    }
}

/// Drives an export.
pub struct ExportPipeline {
    preset: ExportPreset,
    state: ExportState,
    frame: u64,
    total_frames: u64,
    duration: Time,
    timebase: Timebase,
    encoder: Option<Box<dyn Encoder>>,
    /// The number of chunks written.
    chunks: u64,
}

impl std::fmt::Debug for ExportPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportPipeline")
            .field("preset", &self.preset)
            .field("state", &self.state)
            .field("frame", &self.frame)
            .field("total_frames", &self.total_frames)
            .finish()
    }
}

impl ExportPipeline {
    /// A pipeline for `duration` using `preset`.
    pub fn new(preset: ExportPreset, duration: Time) -> VimoResult<Self> {
        preset.validate()?;
        let timebase = preset.timebase();
        let total_frames = duration
            .as_secs_f64()
            .mul_add(timebase.fps().as_f64(), 0.5)
            .floor()
            .max(0.0) as u64;
        Ok(ExportPipeline {
            preset,
            state: ExportState::Idle,
            frame: 0,
            total_frames,
            duration,
            timebase,
            encoder: None,
            chunks: 0,
        })
    }

    /// The preset.
    #[must_use]
    pub fn preset(&self) -> &ExportPreset {
        &self.preset
    }

    /// The state.
    #[must_use]
    pub fn state(&self) -> ExportState {
        self.state
    }

    /// The number of frames encoded.
    #[must_use]
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// The total number of frames.
    #[must_use]
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// The number of chunks written to the encoder.
    #[must_use]
    pub fn chunks(&self) -> u64 {
        self.chunks
    }

    /// The progress.
    #[must_use]
    pub fn progress(&self) -> ExportProgress {
        ExportProgress { frame: self.frame, total_frames: self.total_frames, state: self.state }
    }

    /// Start the export with an encoder.
    pub fn start(&mut self, encoder: Box<dyn Encoder>) -> VimoResult<()> {
        self.preset.validate()?;
        self.encoder = Some(encoder);
        self.frame = 0;
        self.chunks = 0;
        self.state = ExportState::Running;
        Ok(())
    }

    /// Resume an export from a checkpoint.
    ///
    /// The encoder must already hold the data up to `frame`.
    pub fn resume(&mut self, encoder: Box<dyn Encoder>, frame: u64) -> VimoResult<()> {
        self.encoder = Some(encoder);
        self.frame = frame.min(self.total_frames);
        self.state = ExportState::Running;
        Ok(())
    }

    /// Pause the export.
    pub fn pause(&mut self) {
        if self.state == ExportState::Running {
            self.state = ExportState::Paused;
        }
    }

    /// Resume a paused export.
    pub fn resume_paused(&mut self) {
        if self.state == ExportState::Paused {
            self.state = ExportState::Running;
        }
    }

    /// Cancel the export.
    pub fn cancel(&mut self) {
        if self.state == ExportState::Running || self.state == ExportState::Paused {
            self.state = ExportState::Cancelled;
        }
        if let Some(encoder) = &mut self.encoder {
            encoder.release();
        }
        self.encoder = None;
    }

    /// The time of a frame index.
    #[must_use]
    pub fn time_of_frame(&self, frame: u64) -> Time {
        self.timebase.frame_to_time(frame as i64)
    }

    /// Render and encode the next frame.
    ///
    /// Returns the progress. When the export is finished, the state becomes
    /// [`ExportState::Completed`].
    pub fn encode_next(
        &mut self,
        graph: &mut RenderGraph,
        frame: &vimo_media::VideoFrame,
    ) -> VimoResult<ExportProgress> {
        match self.state {
            ExportState::Running => {}
            ExportState::Idle => return Err(invalid_argument("export: not started")),
            ExportState::Paused => return Ok(self.progress()),
            ExportState::Cancelled => return Err(cancelled("export: cancelled")),
            ExportState::Completed => return Ok(self.progress()),
            ExportState::Failed => return Err(invalid_argument("export: failed")),
        }

        // The export renders at the preset's resolution, full scale.
        graph.set_resolution(self.preset.width, self.preset.height);
        graph.set_preview_scale(1.0);
        graph.set_preview_mode(false);

        let time = self.time_of_frame(self.frame);
        let _ = graph.frame_at(time)?;

        if let Some(encoder) = &mut self.encoder {
            let chunks = encoder.encode_video(frame)?;
            self.chunks += chunks.len() as u64;
        }

        self.frame += 1;
        if self.frame >= self.total_frames {
            self.finish()?;
        }
        Ok(self.progress())
    }

    /// Flush the encoder and finish the export.
    pub fn finish(&mut self) -> VimoResult<()> {
        if let Some(encoder) = &mut self.encoder {
            let chunks = encoder.finish()?;
            self.chunks += chunks.len() as u64;
        }
        self.state = ExportState::Completed;
        Ok(())
    }

    /// The total duration being exported.
    #[must_use]
    pub fn duration(&self) -> Time {
        self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_validate() {
        let mut p = ExportPreset::hd_1080p();
        assert!(p.validate().is_ok());
        p.fps = 0;
        assert!(p.validate().is_err());
    }

    #[test]
    fn total_frames_follow_the_preset() {
        let pipeline = ExportPipeline::new(ExportPreset::hd_1080p(), Time::from_secs(10)).unwrap();
        assert_eq!(pipeline.total_frames(), 300);
    }

    #[test]
    fn cancelling_stops_the_pipeline() {
        let mut pipeline = ExportPipeline::new(ExportPreset::hd_1080p(), Time::from_secs(1)).unwrap();
        pipeline.start(Box::new(vimo_media::FakeEncoder::default())).unwrap();
        pipeline.cancel();
        assert_eq!(pipeline.state(), ExportState::Cancelled);
    }

    #[test]
    fn frame_times_are_exact() {
        let pipeline = ExportPipeline::new(ExportPreset::hd_1080p(), Time::from_secs(10)).unwrap();
        assert_eq!(pipeline.time_of_frame(30), Time::from_secs(1));
    }
}
