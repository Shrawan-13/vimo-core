//! Decoder and encoder traits.
//!
//! The core never talks to `MediaCodec` or FFmpeg directly. It talks to these
//! traits, which keeps the core testable on a desktop and lets the Android
//! layer swap implementations (hardware decoder → software decoder) at
//! runtime without the core noticing.

use vimo_foundation::error::{invalid_argument, VimoResult};
use vimo_foundation::time::Time;

use crate::codec::{AudioFormat, CodecId, MediaFormat, PixelFormat, VideoFormat};
use crate::frame::{AudioBuffer, FrameBuffer, VideoFrame};

/// How a decoder should seek.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekMode {
    /// Seek to the closest key frame at or before the target.
    PreviousKeyFrame,
    /// Seek to the closest key frame at or after the target.
    NextKeyFrame,
    /// Seek precisely, decoding and discarding frames.
    Exact,
}

/// Decoder configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct DecoderConfig {
    /// The source to decode.
    pub format: MediaFormat,
    /// The starting position.
    pub start: Time,
    /// Seek behaviour.
    pub seek_mode: SeekMode,
    /// Whether the decoder should drop frames it cannot keep up with.
    pub allow_frame_drop: bool,
    /// The number of frames the decoder may buffer internally.
    pub output_buffer_size: u32,
}

impl DecoderConfig {
    /// A configuration for a format.
    #[must_use]
    pub fn new(format: MediaFormat) -> Self {
        DecoderConfig {
            format,
            start: Time::ZERO,
            seek_mode: SeekMode::PreviousKeyFrame,
            allow_frame_drop: true,
            output_buffer_size: 4,
        }
    }
}

/// A decoder for one media source.
pub trait Decoder: Send {
    /// Prepare the decoder for a source.
    fn configure(&mut self, config: &DecoderConfig) -> VimoResult<()>;

    /// Decode the frame at `time`.
    ///
    /// Implementations may seek internally. The returned frame's `pts` is the
    /// *exact* presentation time of the frame, which may differ slightly from
    /// the requested time for variable frame rate sources.
    fn decode_at(&mut self, time: Time) -> VimoResult<VideoFrame>;

    /// Decode the next frame in presentation order.
    fn decode_next(&mut self) -> VimoResult<Option<VideoFrame>>;

    /// Flush any internal state and return the decoder to the start.
    fn reset(&mut self) -> VimoResult<()>;

    /// Release the decoder's resources. The decoder must not be used afterwards.
    fn release(&mut self);

    /// A human readable name, for diagnostics.
    fn name(&self) -> &str;
}

/// Encoder configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct EncoderConfig {
    /// The output format.
    pub format: VideoFormat,
    /// The target bitrate in bits per second.
    pub bitrate: u32,
    /// The target frame rate (frames per second).
    pub frame_rate: u32,
    /// The key frame interval in frames.
    pub key_frame_interval: u32,
    /// The audio format, if any.
    pub audio: Option<AudioFormat>,
}

impl EncoderConfig {
    /// A configuration for a video format and bitrate.
    #[must_use]
    pub fn new(format: VideoFormat, bitrate: u32, frame_rate: u32) -> Self {
        EncoderConfig {
            format,
            bitrate,
            frame_rate,
            key_frame_interval: frame_rate * 2,
            audio: None,
        }
    }
}

/// A chunk of encoded output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedChunk {
    /// The encoded bytes.
    pub data: Vec<u8>,
    /// Presentation timestamp.
    pub pts: Time,
    /// Whether the chunk is a key frame.
    pub keyframe: bool,
}

/// An encoder.
pub trait Encoder: Send {
    /// Prepare the encoder.
    fn configure(&mut self, config: &EncoderConfig) -> VimoResult<()>;

    /// Encode one video frame.
    fn encode_video(&mut self, frame: &VideoFrame) -> VimoResult<Vec<EncodedChunk>>;

    /// Encode one audio buffer.
    fn encode_audio(&mut self, buffer: &AudioBuffer) -> VimoResult<Vec<EncodedChunk>>;

    /// Flush the encoder and return the remaining chunks.
    fn finish(&mut self) -> VimoResult<Vec<EncodedChunk>>;

    /// Release the encoder's resources.
    fn release(&mut self);

    /// A human readable name, for diagnostics.
    fn name(&self) -> &str;
}

/// A software decoder used on the desktop and in tests.
///
/// It "decodes" by producing solid-colour frames derived from the frame index,
/// which is enough to exercise the whole pipeline (timing, caching, prefetch,
/// export) without shipping media files.
#[derive(Debug)]
pub struct FakeDecoder {
    format: MediaFormat,
    position: Time,
    frame_index: u64,
    released: bool,
}

impl Default for FakeDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeDecoder {
    /// A new fake decoder.
    #[must_use]
    pub fn new() -> Self {
        FakeDecoder {
            format: MediaFormat::new(),
            position: Time::ZERO,
            frame_index: 0,
            released: false,
        }
    }

    /// The number of frames produced so far.
    #[must_use]
    pub fn frames_produced(&self) -> u64 {
        self.frame_index
    }
}

impl Decoder for FakeDecoder {
    fn configure(&mut self, config: &DecoderConfig) -> VimoResult<()> {
        if self.released {
            return Err(invalid_argument("fake decoder: already released"));
        }
        config.format.validate()?;
        self.format = config.format.clone();
        self.position = config.start;
        self.frame_index = 0;
        Ok(())
    }

    fn decode_at(&mut self, time: Time) -> VimoResult<VideoFrame> {
        if self.released {
            return Err(invalid_argument("fake decoder: already released"));
        }
        let video = self
            .format
            .video
            .unwrap_or_else(|| VideoFormat::new(64, 64, PixelFormat::Rgba8888));
        let fps = video.timebase.fps().as_f64().max(1.0);
        let index = (time.as_secs_f64() * fps).round() as u64;
        self.frame_index = index;
        let mut buffer = FrameBuffer::for_format(&video);
        let data = buffer.as_mut_slice();
        let seed = (index % 251) as u8;
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(31).wrapping_add(seed);
        }
        let mut frame = VideoFrame::new(buffer, time);
        frame.index = index;
        frame.keyframe = index % 30 == 0;
        frame.duration = Time::from_secs_f64(1.0 / fps);
        frame.rotation = video.rotation;
        self.position = time;
        Ok(frame)
    }

    fn decode_next(&mut self) -> VimoResult<Option<VideoFrame>> {
        if self.released {
            return Err(invalid_argument("fake decoder: already released"));
        }
        let video = self
            .format
            .video
            .unwrap_or_else(|| VideoFormat::new(64, 64, PixelFormat::Rgba8888));
        let fps = video.timebase.fps().as_f64().max(1.0);
        let frame = self.decode_at(self.position)?;
        self.position += Time::from_secs_f64(1.0 / fps);
        Ok(Some(frame))
    }

    fn reset(&mut self) -> VimoResult<()> {
        self.position = Time::ZERO;
        self.frame_index = 0;
        Ok(())
    }

    fn release(&mut self) {
        self.released = true;
    }

    fn name(&self) -> &str {
        "fake"
    }
}

/// A software encoder that simply stores the raw frames.
#[derive(Debug, Default)]
pub struct FakeEncoder {
    config: Option<EncoderConfig>,
    released: bool,
}

impl Encoder for FakeEncoder {
    fn configure(&mut self, config: &EncoderConfig) -> VimoResult<()> {
        if self.released {
            return Err(invalid_argument("fake encoder: already released"));
        }
        if config.bitrate == 0 {
            return Err(invalid_argument("fake encoder: bitrate must be positive"));
        }
        self.config = Some(config.clone());
        Ok(())
    }

    fn encode_video(&mut self, frame: &VideoFrame) -> VimoResult<Vec<EncodedChunk>> {
        if self.released {
            return Err(invalid_argument("fake encoder: already released"));
        }
        Ok(vec![EncodedChunk {
            data: frame.buffer.as_slice().to_vec(),
            pts: frame.pts,
            keyframe: frame.keyframe,
        }])
    }

    fn encode_audio(&mut self, buffer: &AudioBuffer) -> VimoResult<Vec<EncodedChunk>> {
        if self.released {
            return Err(invalid_argument("fake encoder: already released"));
        }
        let mut data = Vec::with_capacity(buffer.samples.len() * 2);
        for s in &buffer.samples {
            data.extend_from_slice(&s.to_le_bytes());
        }
        Ok(vec![EncodedChunk { data, pts: buffer.pts, keyframe: true }])
    }

    fn finish(&mut self) -> VimoResult<Vec<EncodedChunk>> {
        Ok(Vec::new())
    }

    fn release(&mut self) {
        self.released = true;
    }

    fn name(&self) -> &str {
        "fake"
    }
}

/// The codec a platform supports for a given purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodecSupport {
    /// The codec.
    pub codec: CodecId,
    /// Whether hardware acceleration is available.
    pub hardware: bool,
    /// The maximum width the codec can handle.
    pub max_width: u32,
    /// The maximum height the codec can handle.
    pub max_height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use vimo_foundation::time::Timebase;

    #[test]
    fn fake_decoder_produces_frames() {
        let mut d = FakeDecoder::new();
        let mut format = MediaFormat::new();
        let mut v = VideoFormat::new(32, 16, PixelFormat::Rgba8888);
        v.timebase = Timebase::from_fps(30, 1);
        format.video = Some(v);
        d.configure(&DecoderConfig::new(format)).unwrap();
        let f = d.decode_at(Time::from_secs(1)).unwrap();
        assert_eq!(f.size(), (32, 16));
        assert_eq!(f.index, 30);
    }

    #[test]
    fn released_decoder_errors() {
        let mut d = FakeDecoder::new();
        d.release();
        assert!(d.configure(&DecoderConfig::new(MediaFormat::new())).is_err());
    }

    #[test]
    fn encoder_rejects_zero_bitrate() {
        let mut e = FakeEncoder::default();
        let cfg = EncoderConfig::new(VideoFormat::new(16, 16, PixelFormat::Rgba8888), 0, 30);
        assert!(e.configure(&cfg).is_err());
    }
}
