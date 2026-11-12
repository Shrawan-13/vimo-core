//! Codec and format descriptions.

use vimo_foundation::error::{invalid_argument, VimoResult};
use vimo_foundation::time::{Time, Timebase};

/// A video codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CodecId {
    /// H.264 / AVC.
    H264,
    /// H.265 / HEVC.
    Hevc,
    /// VP8.
    Vp8,
    /// VP9.
    Vp9,
    /// AV1.
    Av1,
    /// MPEG-4 Part 2.
    Mpeg4,
    /// Motion JPEG.
    MotionJpeg,
    /// Apple ProRes.
    ProRes,
    /// A raw / uncompressed format.
    Raw,
}

impl CodecId {
    /// A stable short name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            CodecId::H264 => "h264",
            CodecId::Hevc => "hevc",
            CodecId::Vp8 => "vp8",
            CodecId::Vp9 => "vp9",
            CodecId::Av1 => "av1",
            CodecId::Mpeg4 => "mpeg4",
            CodecId::MotionJpeg => "mjpeg",
            CodecId::ProRes => "prores",
            CodecId::Raw => "raw",
        }
    }

    /// Decode from a stable short name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "h264" | "avc" => CodecId::H264,
            "hevc" | "h265" => CodecId::Hevc,
            "vp8" => CodecId::Vp8,
            "vp9" => CodecId::Vp9,
            "av1" => CodecId::Av1,
            "mpeg4" => CodecId::Mpeg4,
            "mjpeg" => CodecId::MotionJpeg,
            "prores" => CodecId::ProRes,
            "raw" => CodecId::Raw,
            _ => return None,
        })
    }
}

/// An audio codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AudioCodec {
    /// AAC.
    Aac,
    /// MP3.
    Mp3,
    /// Opus.
    Opus,
    /// Vorbis.
    Vorbis,
    /// PCM.
    Pcm,
    /// FLAC.
    Flac,
}

impl AudioCodec {
    /// A stable short name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            AudioCodec::Aac => "aac",
            AudioCodec::Mp3 => "mp3",
            AudioCodec::Opus => "opus",
            AudioCodec::Vorbis => "vorbis",
            AudioCodec::Pcm => "pcm",
            AudioCodec::Flac => "flac",
        }
    }
}

/// The pixel layout of a video frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PixelFormat {
    /// The default is opaque black RGBA.
    #[default]
    /// Planar YUV 4:2:0, 8 bit.
    Yuv420,
    /// Semi-planar YUV 4:2:0 (NV12), 8 bit.
    Nv12,
    /// Planar YUV 4:2:0, 10 bit.
    P010,
    /// RGBA, 8 bit per channel.
    Rgba8888,
    /// BGRA, 8 bit per channel.
    Bgra8888,
    /// Compressed surface texture (Android `HardwareBuffer` / `AHardwareBuffer`).
    Surface,
}

impl PixelFormat {
    /// Bytes per pixel (fractional for subsampled formats).
    #[must_use]
    pub const fn bytes_per_pixel(self) -> f32 {
        match self {
            PixelFormat::Yuv420 | PixelFormat::Nv12 => 1.5,
            PixelFormat::P010 => 3.0,
            PixelFormat::Rgba8888 | PixelFormat::Bgra8888 => 4.0,
            PixelFormat::Surface => 4.0,
        }
    }

    /// The size in bytes of a frame of this format.
    #[must_use]
    pub const fn frame_size(self, width: u32, height: u32) -> usize {
        match self {
            PixelFormat::Yuv420 => (width as usize * height as usize * 3) / 2,
            PixelFormat::Nv12 => (width as usize * height as usize * 3) / 2,
            PixelFormat::P010 => width as usize * height as usize * 3,
            PixelFormat::Rgba8888 | PixelFormat::Bgra8888 | PixelFormat::Surface => {
                width as usize * height as usize * 4
            }
        }
    }

    /// Whether the format can be sampled directly as RGBA on the GPU.
    #[must_use]
    pub const fn is_gpu_sampleable(self) -> bool {
        matches!(
            self,
            PixelFormat::Rgba8888 | PixelFormat::Bgra8888 | PixelFormat::Surface
        )
    }
}

/// The colour space of a video frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    /// BT.709 (HD).
    Bt709,
    /// BT.601 (SD).
    Bt601,
    /// Display P3.
    DisplayP3,
    /// Rec.2020 with PQ (HDR).
    Hlg,
    /// Rec.2020 with PQ (HDR).
    Pq,
    /// Linear.
    Linear,
}

/// Display rotation metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rotation {
    /// Rotation in degrees: 0, 90, 180 or 270.
    pub degrees: u32,
}

impl Rotation {
    /// No rotation.
    pub const NONE: Rotation = Rotation { degrees: 0 };
    /// 90° clockwise.
    pub const CW90: Rotation = Rotation { degrees: 90 };
    /// 180°.
    pub const CW180: Rotation = Rotation { degrees: 180 };
    /// 270° clockwise.
    pub const CW270: Rotation = Rotation { degrees: 270 };

    /// Whether the rotation swaps width and height.
    #[must_use]
    pub const fn swaps_axes(self) -> bool {
        self.degrees == 90 || self.degrees == 270
    }

    /// The display size for a coded size.
    #[must_use]
    pub const fn display_size(self, width: u32, height: u32) -> (u32, u32) {
        if self.swaps_axes() {
            (height, width)
        } else {
            (width, height)
        }
    }
}

/// The description of a video stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoFormat {
    /// Coded width in pixels.
    pub width: u32,
    /// Coded height in pixels.
    pub height: u32,
    /// Pixel layout.
    pub pixel_format: PixelFormat,
    /// Colour space.
    pub color_space: ColorSpace,
    /// Bits per colour component.
    pub bit_depth: u8,
    /// Display rotation.
    pub rotation: Rotation,
    /// Nominal frame rate.
    pub timebase: Timebase,
}

impl VideoFormat {
    /// A format description.
    #[must_use]
    pub const fn new(width: u32, height: u32, pixel_format: PixelFormat) -> Self {
        VideoFormat {
            width,
            height,
            pixel_format,
            color_space: ColorSpace::Bt709,
            bit_depth: 8,
            rotation: Rotation::NONE,
            timebase: Timebase::from_fps(30, 1),
        }
    }

    /// The size of one frame in bytes.
    #[must_use]
    pub fn frame_bytes(&self) -> usize {
        self.pixel_format.frame_size(self.width, self.height)
    }

    /// The display size, taking the rotation metadata into account.
    #[must_use]
    pub fn display_size(&self) -> (u32, u32) {
        self.rotation.display_size(self.width, self.height)
    }
}

/// The description of an audio stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of interleaved channels.
    pub channels: u16,
    /// The codec.
    pub codec: AudioCodec,
}

impl AudioFormat {
    /// A format description.
    #[must_use]
    pub const fn new(sample_rate: u32, channels: u16) -> Self {
        AudioFormat {
            sample_rate,
            channels,
            codec: AudioCodec::Pcm,
        }
    }

    /// Bytes per second of audio.
    #[must_use]
    pub fn bytes_per_second(&self) -> usize {
        self.sample_rate as usize * self.channels as usize * 2
    }

    /// The number of samples in `duration` for one channel.
    #[must_use]
    pub fn samples_for(&self, duration: Time) -> u64 {
        (duration.as_secs_f64() * self.sample_rate as f64).round() as u64
    }
}

/// The description of a media source.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaFormat {
    /// The video stream, if any.
    pub video: Option<VideoFormat>,
    /// The audio stream, if any.
    pub audio: Option<AudioFormat>,
    /// The video codec.
    pub video_codec: Option<CodecId>,
    /// The container duration.
    pub duration: Time,
    /// Whether the source has a variable frame rate.
    pub variable_frame_rate: bool,
}

impl MediaFormat {
    /// An empty format.
    #[must_use]
    pub fn new() -> Self {
        MediaFormat {
            video: None,
            audio: None,
            video_codec: None,
            duration: Time::ZERO,
            variable_frame_rate: false,
        }
    }

    /// Validate the format, returning an error when it is inconsistent.
    pub fn validate(&self) -> VimoResult<()> {
        if let Some(v) = &self.video {
            if v.width == 0 || v.height == 0 {
                return Err(invalid_argument("media format: zero video dimensions"));
            }
            if v.width % 2 != 0 || v.height % 2 != 0 {
                return Err(invalid_argument("media format: odd video dimensions"));
            }
        }
        if let Some(a) = &self.audio {
            if a.sample_rate == 0 {
                return Err(invalid_argument("media format: zero sample rate"));
            }
            if a.channels == 0 {
                return Err(invalid_argument("media format: zero channels"));
            }
        }
        Ok(())
    }
}

impl Default for MediaFormat {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_sizes_are_correct() {
        assert_eq!(
            PixelFormat::Yuv420.frame_size(1920, 1080),
            1920 * 1080 * 3 / 2
        );
        assert_eq!(
            PixelFormat::Rgba8888.frame_size(1920, 1080),
            1920 * 1080 * 4
        );
        assert_eq!(PixelFormat::P010.frame_size(1920, 1080), 1920 * 1080 * 3);
    }

    #[test]
    fn rotation_swaps_axes() {
        assert!(Rotation::CW90.swaps_axes());
        assert_eq!(Rotation::CW90.display_size(1920, 1080), (1080, 1920));
        assert_eq!(Rotation::NONE.display_size(1920, 1080), (1920, 1080));
    }

    #[test]
    fn format_validation_catches_bad_input() {
        let mut f = MediaFormat::new();
        f.video = Some(VideoFormat::new(0, 1080, PixelFormat::Yuv420));
        assert!(f.validate().is_err());
        f.video = Some(VideoFormat::new(1921, 1080, PixelFormat::Yuv420));
        assert!(f.validate().is_err());
    }

    #[test]
    fn codec_names_roundtrip() {
        for c in [CodecId::H264, CodecId::Hevc, CodecId::Vp9, CodecId::Av1] {
            assert_eq!(CodecId::from_name(c.name()), Some(c));
        }
    }
}
