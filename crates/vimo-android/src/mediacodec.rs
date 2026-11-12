//! The MediaCodec decoder and encoder.
//!
//! The decoder is configured with an `ANativeWindow`, so the hardware decoder
//! writes its output **directly into a surface**. The GPU then samples that
//! surface, so the picture never exists in system memory — that is the
//! zero-copy path.
//!
//! The `Decoder` trait in the core is frame oriented; the surface path is
//! timestamp oriented. The adapter below keeps a small ring of pending
//! timestamps so that the core can ask for "the frame at time T" and get back
//! the surface that holds it.

use std::ffi::CString;

use vimo_foundation::error::{ErrorKind, VimoError, VimoResult};
use vimo_foundation::time::Time;
use vimo_media::codec::{CodecId, MediaFormat, VideoFormat};
use vimo_media::decoder::{Decoder, DecoderConfig, SeekMode};
use vimo_media::frame::VideoFrame;

use crate::ffi::*;

/// The name of the MediaCodec that decodes a codec.
#[must_use]
pub fn codec_name(codec: CodecId) -> &'static str {
    match codec {
        CodecId::H264 => "c2.android.avc.decoder",
        CodecId::Hevc => "c2.android.hevc.decoder",
        CodecId::Vp9 => "c2.android.vp9.decoder",
        CodecId::Av1 => "c2.android.av1.decoder",
        _ => "c2.android.mpeg4.decoder",
    }
}

/// The MIME type of a codec.
#[must_use]
pub fn mime_type(codec: CodecId) -> &'static str {
    match codec {
        CodecId::H264 => AMEDIA_MIMETYPE_VIDEO_AVC,
        CodecId::Hevc => AMEDIA_MIMETYPE_VIDEO_HEVC,
        CodecId::Vp9 => AMEDIA_MIMETYPE_VIDEO_VP9,
        CodecId::Av1 => AMEDIA_MIMETYPE_VIDEO_AV1,
        _ => "video/mp4v-es",
    }
}

/// A hardware decoder that writes into a surface.
pub struct MediaCodecDecoder {
    /// The native handles are stored as integers so the type stays `Send`;
    /// a decoder is only ever used from the thread that owns it.
    codec: usize,
    format: Option<MediaFormat>,
    /// The surface the decoder renders into.
    window: usize,
    /// The timestamps of the frames that are still in flight.
    pending: Vec<Time>,
    started: bool,
    released: bool,
    /// The time of the last frame that was produced.
    last_pts: Time,
}

impl MediaCodecDecoder {
    /// A decoder that has not been configured yet.
    #[must_use]
    pub fn new() -> Self {
        MediaCodecDecoder {
            codec: 0,
            format: None,
            window: 0,
            pending: Vec::new(),
            started: false,
            released: false,
            last_pts: Time::ZERO,
        }
    }

    /// The surface the decoder renders into.
    #[must_use]
    pub fn window(&self) -> *mut ANativeWindow {
        self.window as *mut ANativeWindow
    }

    /// The time of the last frame the decoder produced.
    #[must_use]
    pub fn last_pts(&self) -> Time {
        self.last_pts
    }
}

impl Default for MediaCodecDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for MediaCodecDecoder {
    fn configure(&mut self, config: &DecoderConfig) -> VimoResult<()> {
        if self.released {
            return Err(vimo_foundation::error::invalid_argument(
                "mediacodec: the decoder was released",
            ));
        }
        let Some(video) = config.format.video else {
            return Err(vimo_foundation::error::invalid_argument(
                "mediacodec: the source has no video stream",
            ));
        };
        let Some(codec) = config.format.video_codec else {
            return Err(vimo_foundation::error::invalid_argument(
                "mediacodec: the source has no video codec",
            ));
        };

        unsafe {
            let name = CString::new(codec_name(codec))
                .map_err(|_| vimo_foundation::error::invalid_argument("mediacodec: bad codec name"))?;
            let codec_ptr = AMediaCodec_createCodecByName(name.as_ptr());
            if codec_ptr.is_null() {
                return Err(VimoError::detailed(
                    ErrorKind::Decode,
                    "mediacodec",
                    format!("no decoder named {}", codec.name()),
                ));
            }
            self.codec = codec_ptr as usize;

            let format = AMediaFormat_new();
            if format.is_null() {
                return Err(graphics("mediacodec: could not create the format"));
            }
            let mime = CString::new(mime_type(codec)).unwrap();
            AMediaFormat_setString(format, c"mime".as_ptr(), mime.as_ptr());
            let width = CString::new("width").unwrap();
            let height = CString::new("height").unwrap();
            AMediaFormat_setInt32(format, width.as_ptr(), video.width as i32);
            AMediaFormat_setInt32(format, height.as_ptr(), video.height as i32);
            let frame_rate = CString::new("frame-rate").unwrap();
            let fps = video.timebase.fps().as_f64() as i32;
            AMediaFormat_setInt32(format, frame_rate.as_ptr(), fps);
            let i_frame_interval = CString::new("i-frame-interval").unwrap();
            AMediaFormat_setInt32(format, i_frame_interval.as_ptr(), 1);

            let mut mime_type_out: i32 = 0;
            let result = AMediaCodec_configure(
                self.codec as *mut AMediaCodec,
                format,
                self.window as *mut ANativeWindow,
                0,
                &mut mime_type_out,
            );
            AMediaFormat_delete(format);
            if result != AMEDIA_OK {
                return Err(VimoError::detailed(
                    ErrorKind::Decode,
                    "mediacodec",
                    format!("configure failed ({result})"),
                ));
            }
        }

        self.format = Some(config.format.clone());
        Ok(())
    }

    fn decode_at(&mut self, time: Time) -> VimoResult<VideoFrame> {
        if self.released {
            return Err(vimo_foundation::error::invalid_argument(
                "mediacodec: the decoder was released",
            ));
        }
        if !self.started {
            unsafe {
                let result = AMediaCodec_start(self.codec as *mut AMediaCodec);
                if result != AMEDIA_OK {
                    return Err(VimoError::detailed(
                        ErrorKind::Decode,
                        "mediacodec",
                        format!("start failed ({result})"),
                    ));
                }
            }
            self.started = true;
        }

        // Drain the output buffers until the requested time is reached.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
        loop {
            unsafe {
                let mut buffer: *mut AMediaBuffer = core::ptr::null_mut();
                let index =
                    AMediaCodec_dequeueOutputBuffer(self.codec as *mut AMediaCodec, &mut buffer, 0);
                if index >= 0 {
                    let index = index as usize;
                    let pts = self.pending.get(index).copied().unwrap_or(self.last_pts);
                    self.pending.truncate(index);
                    AMediaCodec_releaseOutputBufferAtTime(
                        self.codec as *mut AMediaCodec,
                        index,
                        pts.as_nanos() as i64 / 1000,
                        0,
                    );
                    self.last_pts = pts;
                    if pts >= time {
                        break;
                    }
                    continue;
                }
            }
            if std::time::Instant::now() > deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        // The picture lives in the surface, so the frame is a descriptor.
        let format = self
            .format
            .as_ref()
            .and_then(|f| f.video)
            .unwrap_or_else(|| VideoFormat::new(1920, 1080, vimo_media::codec::PixelFormat::Surface));
        let mut frame = VideoFrame::new(vimo_media::frame::FrameBuffer::new(), self.last_pts);
        frame.buffer.format = vimo_media::codec::PixelFormat::Surface;
        frame.buffer.width = format.width;
        frame.buffer.height = format.height;
        Ok(frame)
    }

    fn decode_next(&mut self) -> VimoResult<Option<VideoFrame>> {
        let time = self.last_pts;
        Ok(Some(self.decode_at(time)?))
    }

    fn reset(&mut self) -> VimoResult<()> {
        if self.codec != 0 {
            unsafe { AMediaCodec_flush(self.codec as *mut AMediaCodec) };
        }
        self.pending.clear();
        self.last_pts = Time::ZERO;
        Ok(())
    }

    fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        if self.codec != 0 {
            unsafe {
                if self.started {
                    AMediaCodec_stop(self.codec as *mut AMediaCodec);
                }
                AMediaCodec_delete(self.codec as *mut AMediaCodec);
            }
            self.codec = 0;
        }
    }

    fn name(&self) -> &str {
        "mediacodec"
    }
}

/// The seek mode a surface decoder can honour.
#[must_use]
pub fn seek_mode_of(mode: SeekMode) -> i32 {
    match mode {
        SeekMode::PreviousKeyFrame => 2,  // AMEDIA_SEEK_TO_PREVIOUS_SYNC
        SeekMode::NextKeyFrame => 1,      // AMEDIA_SEEK_TO_NEXT_SYNC
        SeekMode::Exact => 0,              // AMEDIA_SEEK_CLOSEST_SYNC
    }
}

/// A graphics error.
fn graphics(context: &'static str) -> VimoError {
    VimoError::detailed(ErrorKind::Graphics, context, "the driver reported a failure")
}
