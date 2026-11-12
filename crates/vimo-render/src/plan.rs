//! The frame plan: a flat, ordered list of drawing operations.
//!
//! A plan is **data**, not commands. The platform layer interprets it (with
//! OpenGL ES on Android), and the reference rasteriser in
//! [`crate::software`] interprets it on the CPU. Because both consume the same
//! structure, preview and export are guaranteed to produce the same image.

use vimo_foundation::color::Rgba8;
use vimo_foundation::math::{Mat3, Rect};
use vimo_foundation::time::Time;
use vimo_manifest::EffectTypeId;

use crate::uniforms::{BuiltinUniforms, UniformArena, UniformRange};

/// Index of a layer inside a [`crate::graph::LayerTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct LayerIndex(pub u32);

/// Index of an effect inside a frame plan's effect table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct EffectIndex(pub u32);

/// One effect instance within a frame plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameEffect {
    /// The effect type (an index into the effect registry).
    pub effect_type: EffectTypeId,
    /// The built-in uniforms for this effect.
    pub builtins: BuiltinUniforms,
    /// Where the effect's parameter values live in the plan's arena.
    pub range: UniformRange,
}

/// A handle to a texture that the renderer must bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct TextureId(pub u32);

impl TextureId {
    /// The raw value.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
    /// The "no texture" handle.
    pub const NONE: TextureId = TextureId(u32::MAX);
}

/// A request for the picture of a clip at a particular source time.
///
/// The render graph produces these; the engine resolves them to real textures
/// through the decoder and the frame cache. Keeping the request symbolic means
/// the graph never touches the GPU and never allocates a texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureRef {
    /// The clip the picture comes from.
    pub clip: vimo_project::model::ClipId,
    /// The position inside the source media.
    pub source_time: Time,
    /// The resolved texture, filled in by the engine.
    pub texture: TextureId,
}

impl TextureRef {
    /// An unresolved texture request.
    #[must_use]
    pub const fn unresolved(clip: vimo_project::model::ClipId, source_time: Time) -> Self {
        TextureRef { clip, source_time, texture: TextureId::NONE }
    }
}

/// Resolves a picture request into a GPU texture.
///
/// The engine owns the resolver; the platform implements it by asking the
/// decoder (and the frame cache) for the picture.
pub trait TextureResolver {
    /// The texture showing the requested picture.
    ///
    /// Returning [`TextureId::NONE`] means the picture is not available (yet),
    /// and the renderer should skip the layer.
    fn resolve(&mut self, request: &TextureRef) -> TextureId;
}

/// One drawing operation.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawStep {
    /// Clear the target to a colour.
    Clear {
        /// The colour to clear to.
        color: Rgba8,
    },
    /// Composite a source texture onto the target.
    Composite {
        /// The layer being drawn.
        layer: LayerIndex,
        /// The picture to sample.
        source: TextureRef,
        /// Layer space → target space transform (normalised coordinates).
        transform: Mat3,
        /// The region of the source texture to sample, in normalised UV.
        src_rect: Rect,
        /// The region of the target to write, in normalised coordinates.
        dst_rect: Rect,
        /// How the source is blended with the target.
        blend: vimo_project::model::BlendMode,
        /// Layer opacity in `0..=1`.
        opacity: f32,
        /// The effect chain applied to the layer before compositing.
        effects: Vec<EffectIndex>,
    },
    /// Composite a transition between two textures.
    Transition {
        /// The layer the transition belongs to.
        layer: LayerIndex,
        /// The outgoing picture.
        from_source: TextureRef,
        /// The incoming picture.
        to_source: TextureRef,
        /// Layer space → target space transform.
        transform: Mat3,
        /// The region of the source textures to sample.
        src_rect: Rect,
        /// The region of the target to write.
        dst_rect: Rect,
        /// Transition kind.
        kind: vimo_project::model::TransitionKind,
        /// Progress in `0..=1`.
        progress: f32,
        /// The effect chain applied to the transition.
        effects: Vec<EffectIndex>,
    },
}

/// A complete description of one frame.
///
/// The plan owns its buffers and is reused between frames, so rendering a
/// steady-state timeline performs **no allocations**.
#[derive(Debug, Clone, Default)]
pub struct FramePlan {
    /// The time this plan represents.
    pub time: Time,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// The preview scale factor (1.0 = full resolution).
    pub scale: f32,
    /// Whether this plan is a preview (allows lower fidelity) or an export.
    pub preview: bool,
    /// The draw steps, in execution order.
    pub steps: Vec<DrawStep>,
    /// The number of layers that contributed to this plan.
    pub layer_count: u32,
    /// The effects used by this frame.
    pub effects: Vec<FrameEffect>,
    /// The uniform values for this frame's effects.
    pub uniforms: UniformArena,
}

impl FramePlan {
    /// An empty plan.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset the plan for a new frame, keeping the allocated buffers.
    pub fn reset(&mut self, time: Time, width: u32, height: u32, scale: f32, preview: bool) {
        self.time = time;
        self.width = width;
        self.height = height;
        self.scale = scale;
        self.preview = preview;
        self.steps.clear();
        self.effects.clear();
        self.uniforms.clear();
        self.layer_count = 0;
    }

    /// Number of steps.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the plan is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Approximate memory used by the plan, in bytes.
    #[must_use]
    pub fn memory_bytes(&self) -> usize {
        self.steps
            .iter()
            .map(|s| match s {
                DrawStep::Composite { effects, .. } | DrawStep::Transition { effects, .. } => {
                    core::mem::size_of::<DrawStep>() + effects.capacity() * core::mem::size_of::<EffectIndex>()
                }
                DrawStep::Clear { .. } => core::mem::size_of::<DrawStep>(),
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_clears_steps_but_keeps_capacity() {
        let mut p = FramePlan::new();
        p.steps.push(DrawStep::Clear { color: Rgba8::BLACK });
        let cap = p.steps.capacity();
        p.reset(Time::ZERO, 100, 100, 1.0, true);
        assert!(p.steps.is_empty());
        assert_eq!(p.steps.capacity(), cap);
    }
}
