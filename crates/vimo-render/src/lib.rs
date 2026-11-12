//! # vimo-render
//!
//! The Vimo render graph: it turns a project plus a time into a **frame plan**
//! — a flat list of drawing operations.
//!
//! The same plan drives the on-device GPU renderer and the CPU reference
//! rasteriser, so preview and export cannot drift apart.
//!
//! ```
//! use vimo_render::RenderGraph;
//! use vimo_project::Project;
//! use vimo_manifest::EffectRegistry;
//! use vimo_foundation::time::Time;
//!
//! let mut graph = RenderGraph::new(Project::new(), EffectRegistry::new());
//! let plan = graph.frame_at(Time::ZERO).unwrap();
//! assert_eq!(plan.steps.len(), 1); // just the clear
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod graph;
pub mod plan;
pub mod shader;
pub mod software;
pub mod uniforms;

pub use graph::{source_time_for, GraphStats, RenderGraph};
pub use plan::{
    DrawStep, EffectIndex, FrameEffect, FramePlan, LayerIndex, TextureId, TextureRef,
    TextureResolver,
};
pub use shader::{
    parse_uniforms, validate_uniforms, GlType, NullShaderProvider, ProgramId, ShaderProvider,
    ShaderUniform, UniformMismatch,
};
pub use software::{diff, execute, FrameBuffer, FrameProvider, FrameView};
pub use uniforms::{write_param, BuiltinUniforms, UniformArena, UniformBlock, UniformRange};
