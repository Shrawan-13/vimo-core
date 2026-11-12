//! Frame buffers.
//!
//! A [`FrameBuffer`] owns its pixel storage but can hand it back to a pool
//! without copying: the underlying `Vec<u8>` is moved, not cloned. That is what
//! makes the decode → GPU upload path allocation free in steady state.

use vimo_foundation::time::Time;

use crate::codec::{PixelFormat, Rotation, VideoFormat};

/// A block of pixel storage.
///
/// The buffer remembers its **logical size** separately from the allocation,
/// so a recycled buffer can be handed back with the right dimensions even
/// after its storage has been moved out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameBuffer {
    data: Vec<u8>,
    size: usize,
    /// The format of the pixels in this buffer.
    pub format: PixelFormat,
    /// Coded width.
    pub width: u32,
    /// Coded height.
    pub height: u32,
    /// Stride in bytes (0 means tightly packed).
    pub stride: u32,
}

impl FrameBuffer {
    /// An empty buffer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A buffer with `len` bytes of storage, zeroed.
    #[must_use]
    pub fn with_len(len: usize) -> Self {
        FrameBuffer {
            data: vec![0u8; len],
            size: len,
            ..Default::default()
        }
    }

    /// A buffer sized for a video format.
    #[must_use]
    pub fn for_format(format: &VideoFormat) -> Self {
        let mut buf = FrameBuffer::with_len(format.frame_bytes());
        buf.format = format.pixel_format;
        buf.width = format.width;
        buf.height = format.height;
        buf
    }

    /// The pixel data.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.size.min(self.data.len())]
    }

    /// The pixel data, mutably.
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        if self.data.len() < self.size {
            self.data.resize(self.size, 0);
        }
        &mut self.data[..self.size]
    }

    /// The number of bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.size
    }

    /// Whether the buffer holds no data.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Move the storage out, leaving the buffer logically empty.
    ///
    /// Used by pools to recycle the allocation without copying.
    pub fn take_data(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.data)
    }

    /// Replace the storage, keeping the logical size.
    ///
    /// The allocation is reused: the incoming vector is resized to the logical
    /// size and the old allocation is dropped.
    pub fn set_data(&mut self, mut data: Vec<u8>) {
        data.clear();
        data.resize(self.size, 0);
        self.data = data;
    }
}

/// A decoded video frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrame {
    /// The pixels.
    pub buffer: FrameBuffer,
    /// Presentation timestamp.
    pub pts: Time,
    /// Duration until the next frame.
    pub duration: Time,
    /// Display rotation carried by the container.
    pub rotation: Rotation,
    /// Whether the frame is a key frame.
    pub keyframe: bool,
    /// The frame's index in the source, when known.
    pub index: u64,
}

impl VideoFrame {
    /// A frame with the given pixels.
    #[must_use]
    pub fn new(buffer: FrameBuffer, pts: Time) -> Self {
        VideoFrame {
            buffer,
            pts,
            duration: Time::ZERO,
            rotation: Rotation::NONE,
            keyframe: false,
            index: 0,
        }
    }

    /// The coded size.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.buffer.width, self.buffer.height)
    }
}

/// A block of interleaved PCM samples.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioBuffer {
    /// Interleaved 16-bit samples.
    pub samples: Vec<i16>,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of channels.
    pub channels: u16,
    /// Presentation timestamp.
    pub pts: Time,
}

impl AudioBuffer {
    /// An empty buffer.
    #[must_use]
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        AudioBuffer {
            samples: Vec::new(),
            sample_rate,
            channels,
            pts: Time::ZERO,
        }
    }

    /// The number of sample frames (per channel).
    #[must_use]
    pub fn frame_count(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / self.channels as usize
        }
    }

    /// The duration of the buffer.
    #[must_use]
    pub fn duration(&self) -> Time {
        if self.sample_rate == 0 {
            return Time::ZERO;
        }
        Time::from_secs_f64(self.frame_count() as f64 / self.sample_rate as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::VideoFormat;

    #[test]
    fn buffer_for_format_has_the_right_size() {
        let f = VideoFormat::new(64, 32, PixelFormat::Yuv420);
        let b = FrameBuffer::for_format(&f);
        assert_eq!(b.len(), 64 * 32 * 3 / 2);
    }

    #[test]
    fn set_data_reuses_the_allocation() {
        let mut b = FrameBuffer::with_len(64);
        let cap = b.as_slice().len();
        let recycled = b.take_data();
        assert!(recycled.capacity() >= cap);
        b.set_data(recycled);
        assert_eq!(b.len(), 64);
    }

    #[test]
    fn audio_duration() {
        let mut a = AudioBuffer::new(48000, 2);
        a.samples = vec![0; 9600];
        assert_eq!(a.frame_count(), 4800);
        assert!((a.duration().as_secs_f64() - 0.1).abs() < 1e-9);
    }
}
