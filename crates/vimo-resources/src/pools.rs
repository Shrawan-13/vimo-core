//! Pools for decoders, textures and frame buffers.
//!
//! Pools exist to make the steady state allocation free:
//!
//! * The **decoder pool** enforces the hardware limit — a phone may only have
//!   a handful of `MediaCodec` instances alive at once.
//! * The **texture pool** recycles GPU textures, which is the difference
//!   between a smooth scrub and a GC stall.
//! * The **buffer pool** recycles the `Vec<u8>` allocations behind decoded
//!   frames.

use std::collections::VecDeque;

use vimo_foundation::error::{exhausted, VimoResult};
use vimo_foundation::map::FxHashMap;
use vimo_media::{Decoder, FrameBuffer, PixelFormat};

/// A handle to a leased decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DecoderHandle(pub u32);

/// A decoder owned by the pool.
struct PooledDecoder {
    decoder: Box<dyn Decoder>,
    /// The source the decoder is currently configured for.
    source: Option<String>,
    leased: bool,
}

/// A pool of decoders that respects the hardware instance limit.
pub struct DecoderPool {
    decoders: Vec<PooledDecoder>,
    free: Vec<usize>,
    max_instances: usize,
    created: usize,
    peak_live: usize,
}

impl std::fmt::Debug for DecoderPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecoderPool")
            .field("decoders", &self.decoders.len())
            .field("free", &self.free.len())
            .field("max_instances", &self.max_instances)
            .finish()
    }
}

impl DecoderPool {
    /// A pool that allows at most `max_instances` live decoders.
    #[must_use]
    pub fn new(max_instances: usize) -> Self {
        DecoderPool {
            decoders: Vec::new(),
            free: Vec::new(),
            max_instances: max_instances.max(1),
            created: 0,
            peak_live: 0,
        }
    }

    /// The number of live decoder instances.
    #[must_use]
    pub fn live(&self) -> usize {
        self.decoders.len()
    }

    /// The number of idle decoders.
    #[must_use]
    pub fn idle(&self) -> usize {
        self.free.len()
    }

    /// The highest number of simultaneously live decoders.
    #[must_use]
    pub fn peak_live(&self) -> usize {
        self.peak_live
    }

    /// The hardware instance limit.
    #[must_use]
    pub fn max_instances(&self) -> usize {
        self.max_instances
    }

    /// Lease a decoder.
    ///
    /// If the pool is empty and the hardware limit has been reached, this
    /// fails with [`ErrorKind::Exhausted`](vimo_foundation::ErrorKind::Exhausted)
    /// rather than creating an instance the device cannot support.
    pub fn acquire(
        &mut self,
        factory: &mut dyn FnMut() -> Box<dyn Decoder>,
    ) -> VimoResult<DecoderHandle> {
        let index = match self.free.pop() {
            Some(i) => i,
            None => {
                if self.decoders.len() >= self.max_instances {
                    return Err(exhausted("decoder pool: hardware instance limit reached"));
                }
                let decoder = factory();
                self.decoders.push(PooledDecoder { decoder, source: None, leased: true });
                self.created += 1;
                self.peak_live = self.peak_live.max(self.decoders.len());
                
                self.decoders.len() - 1
            }
        };
        self.decoders[index].leased = true;
        Ok(DecoderHandle(index as u32))
    }

    /// Borrow a leased decoder.
    #[must_use]
    pub fn get_mut(&mut self, handle: DecoderHandle) -> Option<&mut dyn Decoder> {
        let slot = self.decoders.get_mut(handle.0 as usize)?;
        if !slot.leased {
            return None;
        }
        Some(slot.decoder.as_mut())
    }

    /// The source a decoder is configured for.
    #[must_use]
    pub fn source_of(&self, handle: DecoderHandle) -> Option<&str> {
        self.decoders
            .get(handle.0 as usize)
            .and_then(|s| s.source.as_deref())
    }

    /// Remember which source a decoder is configured for.
    pub fn set_source(&mut self, handle: DecoderHandle, source: impl Into<String>) {
        if let Some(slot) = self.decoders.get_mut(handle.0 as usize) {
            slot.source = Some(source.into());
        }
    }

    /// Return a decoder to the pool.
    pub fn release(&mut self, handle: DecoderHandle) {
        if let Some(slot) = self.decoders.get_mut(handle.0 as usize) {
            if !slot.leased {
                return;
            }
            slot.leased = false;
            slot.source = None;
            let _ = slot.decoder.reset();
            self.free.push(handle.0 as usize);
        }
    }

    /// Release every idle decoder (used under memory pressure).
    pub fn release_idle(&mut self) -> usize {
        let count = self.free.len();
        let mut free = core::mem::take(&mut self.free);
        // Release from the back so the swap-remove indices stay valid.
        while let Some(index) = free.pop() {
            let mut slot = self.decoders.swap_remove(index);
            slot.decoder.release();
            // Fix up the remaining free indices after the swap-remove.
            for f in free.iter_mut() {
                if *f > index {
                    *f -= 1;
                }
            }
        }
        count
    }
}

/// A GPU texture handle, owned by the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureHandle(pub u32);

/// Allocates and frees GPU textures.
pub trait TextureAllocator {
    /// Allocate a texture.
    fn allocate(&mut self, width: u32, height: u32, format: PixelFormat) -> VimoResult<TextureHandle>;
    /// Free a texture.
    fn free(&mut self, handle: TextureHandle);
}

/// A texture tracked by the pool.
#[derive(Debug, Clone, Copy)]
struct PooledTexture {
    handle: TextureHandle,
    width: u32,
    height: u32,
    format: PixelFormat,
    bytes: usize,
    leased: bool,
}

/// A pool that recycles GPU textures of the same size.
pub struct TexturePool {
    textures: Vec<PooledTexture>,
    /// Free textures bucketed by `(width, height, format)`.
    free: FxHashMap<(u32, u32, u8), Vec<usize>>,
    bytes: usize,
    max_bytes: usize,
}

impl std::fmt::Debug for TexturePool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TexturePool")
            .field("textures", &self.textures.len())
            .field("bytes", &self.bytes)
            .field("max_bytes", &self.max_bytes)
            .finish()
    }
}

impl TexturePool {
    /// A pool holding at most `max_bytes` of textures.
    #[must_use]
    pub fn new(max_bytes: usize) -> Self {
        TexturePool {
            textures: Vec::new(),
            free: FxHashMap::default(),
            bytes: 0,
            max_bytes,
        }
    }

    /// The bytes held by the pool.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The number of tracked textures.
    #[must_use]
    pub fn len(&self) -> usize {
        self.textures.len()
    }

    /// Whether the pool is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }

    /// Lease a texture, reusing a free one of the same size when possible.
    pub fn acquire(
        &mut self,
        allocator: &mut dyn TextureAllocator,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> VimoResult<TextureHandle> {
        let key = (width, height, format as u8);
        if let Some(bucket) = self.free.get_mut(&key) {
            if let Some(index) = bucket.pop() {
                let slot = &mut self.textures[index];
                slot.leased = true;
                return Ok(slot.handle);
            }
        }
        let bytes = format.frame_size(width, height);
        if self.bytes + bytes > self.max_bytes {
            self.trim_to(allocator, self.max_bytes.saturating_sub(bytes));
        }
        let handle = allocator.allocate(width, height, format)?;
        self.textures.push(PooledTexture { handle, width, height, format, bytes, leased: true });
        self.bytes += bytes;
        Ok(handle)
    }

    /// Return a texture to the pool.
    pub fn release(&mut self, handle: TextureHandle) {
        if let Some(index) = self.textures.iter().position(|t| t.handle == handle) {
            if !self.textures[index].leased {
                return;
            }
            self.textures[index].leased = false;
            let key = (
                self.textures[index].width,
                self.textures[index].height,
                self.textures[index].format as u8,
            );
            self.free.entry(key).or_default().push(index);
        }
    }

    /// Release every idle texture (used under memory pressure).
    pub fn release_idle(&mut self, allocator: &mut dyn TextureAllocator) -> usize {
        let mut freed = 0usize;
        let indices: Vec<usize> = (0..self.textures.len())
            .filter(|i| !self.textures[*i].leased)
            .collect();
        for index in indices {
            let slot = self.textures[index];
            allocator.free(slot.handle);
            self.bytes = self.bytes.saturating_sub(slot.bytes);
            self.textures.swap_remove(index);
            freed += 1;
        }
        self.free.clear();
        freed
    }

    /// Trim the pool to at most `target_bytes` by releasing idle textures.
    pub fn trim_to(&mut self, allocator: &mut dyn TextureAllocator, target_bytes: usize) -> usize {
        let mut freed = 0usize;
        while self.bytes > target_bytes {
            let Some(index) = (0..self.textures.len()).find(|i| !self.textures[*i].leased) else {
                break;
            };
            let slot = self.textures[index];
            allocator.free(slot.handle);
            self.bytes = self.bytes.saturating_sub(slot.bytes);
            self.textures.swap_remove(index);
            freed += 1;
        }
        self.free.clear();
        freed
    }
}

/// A pool of frame buffers, bucketed by size class.
#[derive(Debug, Default)]
pub struct BufferPool {
    buckets: FxHashMap<usize, VecDeque<Vec<u8>>>,
    bytes: usize,
    max_bytes: usize,
}

impl BufferPool {
    /// A pool holding at most `max_bytes` of recycled buffers.
    #[must_use]
    pub fn new(max_bytes: usize) -> Self {
        BufferPool { buckets: FxHashMap::default(), bytes: 0, max_bytes }
    }

    /// The bytes held.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Take a buffer of at least `size` bytes.
    pub fn take(&mut self, size: usize) -> Vec<u8> {
        if let Some(bucket) = self.buckets.get_mut(&size) {
            if let Some(mut buf) = bucket.pop_front() {
                self.bytes = self.bytes.saturating_sub(size);
                buf.clear();
                buf.resize(size, 0);
                return buf;
            }
        }
        vec![0u8; size]
    }

    /// Take a frame buffer for a video format.
    pub fn take_frame(&mut self, format: &vimo_media::VideoFormat) -> FrameBuffer {
        let size = format.frame_bytes();
        let mut buf = FrameBuffer::with_len(size);
        buf.format = format.pixel_format;
        buf.width = format.width;
        buf.height = format.height;
        let data = self.take(size);
        buf.set_data(data);
        buf
    }

    /// Return a buffer to the pool.
    pub fn put(&mut self, mut buf: Vec<u8>) {
        if buf.is_empty() {
            return;
        }
        let size = buf.len();
        if self.bytes + size > self.max_bytes {
            return;
        }
        buf.clear();
        self.buckets.entry(size).or_default().push_back(buf);
        self.bytes += size;
    }

    /// Return a frame buffer's storage to the pool.
    pub fn put_frame(&mut self, mut buf: FrameBuffer) {
        let data = buf.take_data();
        self.put(data);
    }

    /// Drop every pooled buffer.
    pub fn clear(&mut self) {
        self.buckets.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vimo_media::{DecoderConfig, FakeDecoder, MediaFormat};

    #[test]
    fn decoder_pool_respects_the_instance_limit() {
        let mut pool = DecoderPool::new(2);
        let mut factory = || Box::new(FakeDecoder::new()) as Box<dyn Decoder>;
        let a = pool.acquire(&mut factory).unwrap();
        let b = pool.acquire(&mut factory).unwrap();
        assert!(pool.acquire(&mut factory).is_err());
        pool.release(a);
        assert!(pool.acquire(&mut factory).is_ok());
        pool.release(b);
        assert_eq!(pool.peak_live(), 2);
    }

    #[test]
    fn decoder_pool_reuses_released_instances() {
        let mut pool = DecoderPool::new(4);
        let mut factory = || Box::new(FakeDecoder::new()) as Box<dyn Decoder>;
        let a = pool.acquire(&mut factory).unwrap();
        pool.release(a);
        let b = pool.acquire(&mut factory).unwrap();
        assert_eq!(a, b);
        assert_eq!(pool.live(), 1);
    }

    struct CountingAllocator {
        allocated: usize,
        freed: usize,
    }

    impl TextureAllocator for CountingAllocator {
        fn allocate(&mut self, _w: u32, _h: u32, _f: PixelFormat) -> VimoResult<TextureHandle> {
            self.allocated += 1;
            Ok(TextureHandle(self.allocated as u32 - 1))
        }
        fn free(&mut self, _handle: TextureHandle) {
            self.freed += 1;
        }
    }

    #[test]
    fn texture_pool_recycles_matching_textures() {
        let mut pool = TexturePool::new(1 << 20);
        let mut alloc = CountingAllocator { allocated: 0, freed: 0 };
        let a = pool.acquire(&mut alloc, 64, 64, PixelFormat::Rgba8888).unwrap();
        pool.release(a);
        let b = pool.acquire(&mut alloc, 64, 64, PixelFormat::Rgba8888).unwrap();
        assert_eq!(a, b);
        assert_eq!(alloc.allocated, 1, "no new texture should have been allocated");
    }

    #[test]
    fn texture_pool_frees_idle_textures_under_pressure() {
        let mut pool = TexturePool::new(1 << 20);
        let mut alloc = CountingAllocator { allocated: 0, freed: 0 };
        let a = pool.acquire(&mut alloc, 64, 64, PixelFormat::Rgba8888).unwrap();
        pool.release(a);
        assert_eq!(pool.release_idle(&mut alloc), 1);
        assert_eq!(alloc.freed, 1);
        assert!(pool.is_empty());
    }

    #[test]
    fn buffer_pool_recycles_storage() {
        let mut pool = BufferPool::new(1 << 20);
        let buf = pool.take(1024);
        assert_eq!(buf.len(), 1024);
        pool.put(buf);
        let again = pool.take(1024);
        assert_eq!(again.len(), 1024);
        assert_eq!(pool.bytes(), 0);
    }

    #[test]
    fn decoder_pool_release_resets_the_decoder() {
        let mut pool = DecoderPool::new(1);
        let mut factory = || Box::new(FakeDecoder::new()) as Box<dyn Decoder>;
        let a = pool.acquire(&mut factory).unwrap();
        let cfg = DecoderConfig::new(MediaFormat::new());
        pool.get_mut(a).unwrap().configure(&cfg).unwrap();
        pool.release(a);
        let b = pool.acquire(&mut factory).unwrap();
        assert!(pool.get_mut(b).is_some());
    }
}
