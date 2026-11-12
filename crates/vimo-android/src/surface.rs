//! The zero-copy surface path.
//!
//! A `MediaCodec` decodes into an `ANativeWindow`. The GPU samples that
//! window directly through an `EGLImage`, so the picture never touches system
//! memory.
//!
//! The core's [`SurfaceSource`] trait is implemented here: it hands out a
//! texture for a picture, and the texture is backed by the decoder's surface.

use std::collections::HashMap;

use vimo_foundation::error::{ErrorKind, VimoError, VimoResult};
use vimo_media::surface::{SurfacePicture, SurfaceSource, SurfaceTexture, TextureBacking};
use vimo_render::TextureId;

/// The textures that are backed by a decoder surface.
#[derive(Debug, Default)]
pub struct SurfaceTextures {
    /// The texture of each source id.
    textures: HashMap<u64, SurfaceTexture>,
    /// The GL texture handle of each source id.
    handles: HashMap<u64, TextureId>,
}

impl SurfaceTextures {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the texture of a source.
    pub fn insert(&mut self, source: u64, texture: SurfaceTexture, handle: TextureId) {
        self.textures.insert(source, texture);
        self.handles.insert(source, handle);
    }

    /// The texture of a source.
    #[must_use]
    pub fn get(&self, source: u64) -> Option<&SurfaceTexture> {
        self.textures.get(&source)
    }

    /// The GL handle of a source.
    #[must_use]
    pub fn handle(&self, source: u64) -> Option<TextureId> {
        self.handles.get(&source).copied()
    }

    /// Forget a source.
    pub fn remove(&mut self, source: u64) {
        self.textures.remove(&source);
        self.handles.remove(&source);
    }

    /// The number of registered sources.
    #[must_use]
    pub fn len(&self) -> usize {
        self.textures.len()
    }

    /// Whether nothing is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }
}

/// A resolver that turns a picture request into the surface texture of its
/// source.
pub struct SurfaceResolver {
    textures: SurfaceTextures,
    /// The interned source id of each clip.
    clip_sources: HashMap<u32, u64>,
}

impl SurfaceResolver {
    /// A resolver with no sources.
    #[must_use]
    pub fn new() -> Self {
        SurfaceResolver { textures: SurfaceTextures::new(), clip_sources: HashMap::new() }
    }

    /// Tell the resolver which source a clip plays.
    pub fn set_clip_source(&mut self, clip: u32, source: u64) {
        self.clip_sources.insert(clip, source);
    }

    /// Register the surface texture of a source.
    pub fn register(&mut self, source: u64, texture: SurfaceTexture, handle: TextureId) {
        self.textures.insert(source, texture, handle);
    }

    /// The texture of a source.
    #[must_use]
    pub fn texture(&self, source: u64) -> Option<&SurfaceTexture> {
        self.textures.get(source)
    }
}

impl Default for SurfaceResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl vimo_render::TextureResolver for SurfaceResolver {
    fn resolve(&mut self, request: &vimo_render::TextureRef) -> TextureId {
        let Some(source) = self.clip_sources.get(&request.clip) else {
            return TextureId::NONE;
        };
        self.textures.handle(*source).unwrap_or(TextureId::NONE)
    }
}

impl SurfaceSource for SurfaceResolver {
    fn acquire(&mut self, picture: SurfacePicture) -> VimoResult<SurfaceTexture> {
        self.textures
            .get(picture.source)
            .copied()
            .ok_or_else(|| {
                VimoError::detailed(
                    ErrorKind::NotFound,
                    "surface",
                    format!("no surface texture for source {}", picture.source),
                )
            })
    }

    fn release(&mut self, texture: SurfaceTexture) {
        // The textures are owned by the decoder; releasing one means the
        // decoder is done with it.
        log::debug!("release surface texture {}", texture.id);
    }
}

/// The backing of a decoder surface.
#[must_use]
pub fn decoder_surface_backing() -> TextureBacking {
    TextureBacking::DecoderSurface
}
