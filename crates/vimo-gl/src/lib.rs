//! # vimo-gl
//!
//! The portable OpenGL ES renderer for Vimo Core.
//!
//! The renderer turns a [`FramePlan`](vimo_render::FramePlan) into GL calls.
//! It does not link against OpenGL itself: it talks to the [`GlApi`] trait,
//! which the Android layer implements on top of the NDK and a desktop build
//! implements on top of a desktop GL loader.
//!
//! That indirection is what makes the renderer testable: [`mock::MockGl`]
//! records every call, so a test can assert the exact blend factors, the
//! uniform values and the draw order without a GPU.
//!
//! ```
//! use vimo_gl::{GlRenderer, MockGl, NoShaders};
//! use vimo_manifest::EffectRegistry;
//! use vimo_render::FramePlan;
//!
//! let mock = MockGl::new();
//! let mut renderer =
//!     GlRenderer::new(Box::new(mock), EffectRegistry::new(), 1920, 1080).unwrap();
//! renderer.draw(&FramePlan::new(), &mut NoShaders).unwrap();
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod api;
pub mod library;
pub mod mock;
pub mod renderer;
pub mod shaders;

pub use api::{
    BufferId, BufferTarget, Capability, ClearMask, DrawMode, FramebufferAttachment, FramebufferId,
    FramebufferTarget, GlApi, GlError, ProgramId, ShaderId, ShaderKind, TextureFormat, TextureId,
    TextureParam, TextureTarget, UniformLocation, VertexArrayId,
};
pub use library::ShaderLibrary;
pub use mock::{Call, MockGl};
pub use renderer::{GlRenderer, NoShaders, ShaderSourceProvider};
pub use shaders::{
    PASSTHROUGH_FRAGMENT_SHADER, TRANSITION_FRAGMENT_SHADER, TRANSITION_VERTEX_SHADER,
    VERTEX_SHADER,
};
