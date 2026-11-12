//! The OpenGL ES renderer.
//!
//! The renderer consumes a [`FramePlan`] and turns it into GL calls. It is
//! completely portable: everything platform specific lives behind
//! [`GlApi`].
//!
//! # Pass structure
//!
//! A layer with an effect chain is drawn in several passes:
//!
//! ```text
//!   source texture ──pass 0──► target[0] ──pass 1──► target[1] ── … ──► target[n]
//!                                    (the layer transform is applied in pass 0)
//!   target[n] ──final pass──► the framebuffer (with the blend mode)
//! ```
//!
//! Without effects, the source is composited directly onto the framebuffer.
//!
//! # Coordinate conventions
//!
//! Vimo's space has its origin at the **top left**, with `y` growing
//! downwards — the same convention the CPU reference rasteriser uses, so the
//! two agree.
//!
//! GL's window space has its origin at the bottom left. The renderer keeps
//! every intermediate target in Vimo's convention (the first row of a
//! texture is the top of the image) and flips only when it draws to the
//! final framebuffer, which is what makes an image appear upright on an
//! Android surface.

use std::collections::HashMap;

use vimo_foundation::error::{VimoError, VimoResult};
use vimo_foundation::math::Mat3;
use vimo_manifest::{EffectRegistry, EffectTypeId};
use vimo_project::model::BlendMode;
use vimo_render::{DrawStep, FramePlan, TextureId};

use crate::api::{
    BlendEquation, BufferTarget, Capability, ClearMask, DrawMode, FramebufferAttachment,
    FramebufferTarget, GlApi, GlError, ProgramId, ShaderId, ShaderKind, TextureFormat,
    TextureParam, UniformLocation, VertexArrayId,
};
use crate::shaders;

/// The GLSL fragment shader for an effect type.
pub trait ShaderSourceProvider {
    /// The fragment shader source, or an error if the effect has no shader.
    fn fragment_source(&mut self, effect_type: EffectTypeId) -> VimoResult<String>;
}

/// A provider that has no shaders, for effects that do not need one.
#[derive(Debug, Default)]
pub struct NoShaders;

impl ShaderSourceProvider for NoShaders {
    fn fragment_source(&mut self, _effect_type: EffectTypeId) -> VimoResult<String> {
        Err(vimo_foundation::error::unsupported("gl: no shader source available"))
    }
}

/// The uniform locations of a program.
#[derive(Debug, Default, Clone)]
struct UniformLocations {
    transform: UniformLocation,
    aspect: UniformLocation,
    flip_y: UniformLocation,
    texture: UniformLocation,
    texture2: UniformLocation,
    opacity: UniformLocation,
    time: UniformLocation,
    resolution: UniformLocation,
    texel: UniformLocation,
    progress: UniformLocation,
    /// The parameter uniforms, in manifest declaration order.
    params: Vec<UniformLocation>,
}

impl UniformLocations {
    fn invalid(program: ProgramId) -> Self {
        let loc = UniformLocation { program, location: -1 };
        UniformLocations {
            transform: loc,
            aspect: loc,
            flip_y: loc,
            texture: loc,
            texture2: loc,
            opacity: loc,
            time: loc,
            resolution: loc,
            texel: loc,
            progress: loc,
            params: Vec::new(),
        }
    }
}

/// A compiled effect program.
#[derive(Debug)]
struct ProgramEntry {
    program: ProgramId,
    uniforms: UniformLocations,
}

/// The uniform locations of the built-in passthrough program.
#[derive(Debug, Clone)]
struct PassthroughUniforms {
    transform: UniformLocation,
    aspect: UniformLocation,
    flip_y: UniformLocation,
    opacity: UniformLocation,
    texture: UniformLocation,
}

/// The uniform locations of the built-in transition program.
#[derive(Debug, Clone)]
struct TransitionUniforms {
    transform: UniformLocation,
    aspect: UniformLocation,
    flip_y: UniformLocation,
    texture: UniformLocation,
    texture2: UniformLocation,
    progress: UniformLocation,
}

/// An off-screen render target.
#[derive(Debug)]
struct RenderTarget {
    framebuffer: crate::api::FramebufferId,
    texture: TextureId,
}

/// The quad geometry.
#[derive(Debug)]
struct Quad {
    vao: VertexArrayId,
    vbo: crate::api::BufferId,
}

/// The portable GL renderer.
pub struct GlRenderer {
    gl: Box<dyn GlApi>,
    registry: EffectRegistry,
    programs: HashMap<EffectTypeId, Option<ProgramEntry>>,
    vertex_shader: ShaderId,
    passthrough: ProgramId,
    passthrough_uniforms: PassthroughUniforms,
    external_passthrough: ProgramId,
    external_uniforms: PassthroughUniforms,
    transition: ProgramId,
    transition_uniforms: TransitionUniforms,
    quad: Quad,
    targets: Vec<RenderTarget>,
    width: u32,
    height: u32,
    /// Whether the final flip is applied (true for a top-left target).
    flip_final: bool,
}

impl std::fmt::Debug for GlRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlRenderer")
            .field("programs", &self.programs.len())
            .field("targets", &self.targets.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

impl GlRenderer {
    /// Create a renderer on top of `gl`.
    ///
    /// The renderer compiles its built-in shaders immediately, so a failure
    /// is reported at start-up rather than on the first frame.
    pub fn new(
        mut gl: Box<dyn GlApi>,
        registry: EffectRegistry,
        width: u32,
        height: u32,
    ) -> VimoResult<Self> {
        let vertex_shader = gl.create_shader(ShaderKind::Vertex, shaders::VERTEX_SHADER)?;
        let passthrough_fragment =
            gl.create_shader(ShaderKind::Fragment, shaders::PASSTHROUGH_FRAGMENT_SHADER)?;
        let passthrough = gl.create_program(vertex_shader, passthrough_fragment)?;
        gl.delete_shader(passthrough_fragment);

        let transition_fragment =
            gl.create_shader(ShaderKind::Fragment, shaders::TRANSITION_FRAGMENT_SHADER)?;
        let transition = gl.create_program(vertex_shader, transition_fragment)?;
        gl.delete_shader(transition_fragment);

        // The external variant samples a decoder surface directly.
        let external_fragment = gl
            .create_shader(ShaderKind::Fragment, shaders::EXTERNAL_PASSTHROUGH_FRAGMENT_SHADER)?;
        let external_passthrough = gl.create_program(vertex_shader, external_fragment)?;
        gl.delete_shader(external_fragment);
        let external_uniforms = PassthroughUniforms {
            transform: gl.uniform_location(external_passthrough, "u_transform"),
            aspect: gl.uniform_location(external_passthrough, "u_aspect"),
            flip_y: gl.uniform_location(external_passthrough, "u_flip_y"),
            opacity: gl.uniform_location(external_passthrough, "u_opacity"),
            texture: gl.uniform_location(external_passthrough, "u_texture"),
        };

        let passthrough_uniforms = PassthroughUniforms {
            transform: gl.uniform_location(passthrough, "u_transform"),
            aspect: gl.uniform_location(passthrough, "u_aspect"),
            flip_y: gl.uniform_location(passthrough, "u_flip_y"),
            opacity: gl.uniform_location(passthrough, "u_opacity"),
            texture: gl.uniform_location(passthrough, "u_texture"),
        };
        let transition_uniforms = TransitionUniforms {
            transform: gl.uniform_location(transition, "u_transform"),
            aspect: gl.uniform_location(transition, "u_aspect"),
            flip_y: gl.uniform_location(transition, "u_flip_y"),
            texture: gl.uniform_location(transition, "u_texture"),
            texture2: gl.uniform_location(transition, "u_texture2"),
            progress: gl.uniform_location(transition, "u_progress"),
        };

        let mut renderer = GlRenderer {
            gl,
            registry,
            programs: HashMap::new(),
            vertex_shader,
            passthrough,
            passthrough_uniforms,
            external_passthrough,
            external_uniforms,
            transition,
            transition_uniforms,
            quad: Quad {
                vao: VertexArrayId(u32::MAX),
                vbo: crate::api::BufferId(u32::MAX),
            },
            targets: Vec::new(),
            width: width.max(1),
            height: height.max(1),
            flip_final: true,
        };
        renderer.create_quad()?;
        Ok(renderer)
    }

    /// Replace the effect registry (for example after the manifest changed).
    pub fn set_registry(&mut self, registry: EffectRegistry) {
        self.registry = registry;
        self.programs.clear();
    }

    /// The effect registry.
    #[must_use]
    pub fn registry(&self) -> &EffectRegistry {
        &self.registry
    }

    /// Set the output size, reallocating the off-screen targets.
    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        self.release_targets();
    }

    /// Whether the final pass flips the image (top-left targets need it).
    pub fn set_flip_final(&mut self, flip: bool) {
        self.flip_final = flip;
    }

    /// Release the off-screen targets.
    fn release_targets(&mut self) {
        for target in self.targets.drain(..) {
            self.gl.delete_framebuffer(target.framebuffer);
            self.gl.delete_texture(target.texture);
        }
    }

    /// Create the quad geometry.
    fn create_quad(&mut self) -> VimoResult<()> {
        // Two triangles as a strip: position (vec2) then texcoord (vec2).
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Vertex {
            position: [f32; 2],
            texcoord: [f32; 2],
        }
        const QUAD: [Vertex; 4] = [
            Vertex { position: [0.0, 0.0], texcoord: [0.0, 0.0] },
            Vertex { position: [1.0, 0.0], texcoord: [1.0, 0.0] },
            Vertex { position: [0.0, 1.0], texcoord: [0.0, 1.0] },
            Vertex { position: [1.0, 1.0], texcoord: [1.0, 1.0] },
        ];

        let vao = self.gl.create_vertex_array()?;
        let vbo = self.gl.create_buffer()?;
        self.gl.bind_vertex_array(vao);
        self.gl.bind_buffer(BufferTarget::ArrayBuffer, vbo);
        let bytes: Vec<u8> = QUAD
            .iter()
            .flat_map(|v| {
                let mut out = Vec::with_capacity(16);
                for f in v.position.iter().chain(v.texcoord.iter()) {
                    out.extend_from_slice(&f.to_le_bytes());
                }
                out
            })
            .collect();
        self.gl.buffer_data(BufferTarget::ArrayBuffer, bytes.len(), &bytes);
        self.gl.enable_vertex_attrib(0);
        self.gl.vertex_attrib_pointer(0, 2, 16, 0);
        self.gl.enable_vertex_attrib(1);
        self.gl.vertex_attrib_pointer(1, 2, 16, 8);
        self.gl.bind_vertex_array(VertexArrayId(u32::MAX));
        self.quad = Quad { vao, vbo };
        Ok(())
    }

    /// Ensure that at least `count` off-screen targets exist.
    fn ensure_targets(&mut self, count: usize) -> VimoResult<()> {
        while self.targets.len() < count {
            let texture = self.gl.create_texture()?;
            self.gl.bind_texture(0, texture);
            self.gl.tex_image_2d(
                crate::api::TextureTarget::Texture2D,
                self.width,
                self.height,
                TextureFormat::Rgba8,
            );
            self.gl.tex_parameter(
                crate::api::TextureTarget::Texture2D,
                TextureParam::MinFilter,
                crate::api::consts::LINEAR as i32,
            );
            self.gl.tex_parameter(
                crate::api::TextureTarget::Texture2D,
                TextureParam::MagFilter,
                crate::api::consts::LINEAR as i32,
            );
            self.gl.tex_parameter(
                crate::api::TextureTarget::Texture2D,
                TextureParam::WrapS,
                crate::api::consts::CLAMP_TO_EDGE as i32,
            );
            self.gl.tex_parameter(
                crate::api::TextureTarget::Texture2D,
                TextureParam::WrapT,
                crate::api::consts::CLAMP_TO_EDGE as i32,
            );

            let framebuffer = self.gl.create_framebuffer()?;
            self.gl.bind_framebuffer(FramebufferTarget::Framebuffer, Some(framebuffer));
            self.gl.framebuffer_texture(FramebufferAttachment::Color0, texture);
            self.gl.check_framebuffer(framebuffer)?;
            self.gl.bind_framebuffer(FramebufferTarget::Framebuffer, None);
            self.targets.push(RenderTarget { framebuffer, texture });
        }
        Ok(())
    }

    /// The program for an effect type, compiling it on first use.
    fn program_for(
        &mut self,
        effect_type: EffectTypeId,
        shaders: &mut dyn ShaderSourceProvider,
    ) -> VimoResult<Option<&ProgramEntry>> {
        if !self.programs.contains_key(&effect_type) {
            // A shader that fails to compile disables the effect rather than
            // breaking the frame.
            let entry = self.compile_program(effect_type, shaders).ok();
            self.programs.insert(effect_type, entry);
        }
        Ok(self.programs.get(&effect_type).and_then(|e| e.as_ref()))
    }

    /// Compile the program for an effect type.
    fn compile_program(
        &mut self,
        effect_type: EffectTypeId,
        shaders: &mut dyn ShaderSourceProvider,
    ) -> VimoResult<ProgramEntry> {
        let source = shaders.fragment_source(effect_type)?;
        let fragment = self.gl.create_shader(ShaderKind::Fragment, &source)?;
        let program = self.gl.create_program(self.vertex_shader, fragment)?;
        self.gl.delete_shader(fragment);

        let mut uniforms = UniformLocations::invalid(program);
        uniforms.transform = self.gl.uniform_location(program, "u_transform");
        uniforms.aspect = self.gl.uniform_location(program, "u_aspect");
        uniforms.flip_y = self.gl.uniform_location(program, "u_flip_y");
        uniforms.texture = self.gl.uniform_location(program, "u_texture");
        uniforms.texture2 = self.gl.uniform_location(program, "u_texture2");
        uniforms.opacity = self.gl.uniform_location(program, "u_opacity");
        uniforms.time = self.gl.uniform_location(program, "u_time");
        uniforms.resolution = self.gl.uniform_location(program, "u_resolution");
        uniforms.texel = self.gl.uniform_location(program, "u_texel");
        uniforms.progress = self.gl.uniform_location(program, "u_progress");

        if let Some(desc) = self.registry.get(effect_type) {
            uniforms.params = desc
                .params
                .iter()
                .map(|p| self.gl.uniform_location(program, &vimo_render::uniforms::uniform_name(&p.name)))
                .collect();
        }
        Ok(ProgramEntry { program, uniforms })
    }

    /// Draw a frame plan.
    pub fn draw(&mut self, plan: &FramePlan, shaders: &mut dyn ShaderSourceProvider) -> VimoResult<()> {
        self.resize(plan.width, plan.height);
        self.gl.viewport(0, 0, plan.width as i32, plan.height as i32);

        for step in &plan.steps {
            match step {
                DrawStep::Clear { color } => {
                    let c = color.to_f32();
                    self.gl.clear_color(c[0], c[1], c[2], c[3]);
                    self.gl.clear(ClearMask::COLOR);
                }
                DrawStep::Composite {
                    source,
                    transform,
                    blend,
                    opacity,
                    effects,
                    ..
                } => {
                    self.draw_composite(
                        plan,
                        source.texture,
                        *transform,
                        *blend,
                        *opacity,
                        effects,
                        shaders,
                    )?;
                }
                DrawStep::Transition {
                    from_source,
                    to_source,
                    transform,
                    progress,
                    effects,
                    ..
                } => {
                    self.draw_transition(
                        plan,
                        from_source.texture,
                        to_source.texture,
                        *transform,
                        *progress,
                        effects,
                        shaders,
                    )?;
                }
            }
        }
        let error = self.gl.get_error();
        if let Some(err) = crate::api::gl_error_to_vimo("gl: draw", error) {
            return Err(err);
        }
        Ok(())
    }

    /// Draw one composite step.
    #[allow(clippy::too_many_arguments)]
    fn draw_composite(
        &mut self,
        plan: &FramePlan,
        texture: TextureId,
        transform: Mat3,
        blend: BlendMode,
        opacity: f32,
        effects: &[vimo_render::EffectIndex],
        shaders: &mut dyn ShaderSourceProvider,
    ) -> VimoResult<()> {
        if effects.is_empty() {
            self.bind_texture(0, texture);
            self.set_blend_state(blend, opacity);
            if self.is_external(texture) {
                self.use_external_passthrough();
            } else {
                self.use_passthrough();
            }
            self.upload_transform(&transform, 1.0);
            self.upload_opacity(opacity);
            self.draw_quad();
            return Ok(());
        }

        // Pass 0: the source into the first target, with the layer transform.
        self.ensure_targets(effects.len() + 1)?;
        self.gl.disable(Capability::Blend);
        self.gl.bind_framebuffer(
            FramebufferTarget::Framebuffer,
            Some(self.targets[0].framebuffer),
        );
        self.gl.viewport(0, 0, plan.width as i32, plan.height as i32);
        self.bind_texture(0, texture);
        self.use_passthrough();
        self.upload_transform(&transform, 0.0);
        self.upload_opacity(1.0);
        self.draw_quad();

        // Passes 1..n: each effect.
        for (i, effect_index) in effects.iter().enumerate() {
            let Some(effect) = plan.effects.get(effect_index.0 as usize) else {
                continue;
            };
            let entry = match self.program_for(effect.effect_type, shaders)? {
                Some(entry) => entry,
                None => continue,
            };
            let program = entry.program;
            let uniforms = entry.uniforms.clone();

            self.gl.bind_framebuffer(
                FramebufferTarget::Framebuffer,
                Some(self.targets[i + 1].framebuffer),
            );
            self.gl.viewport(0, 0, plan.width as i32, plan.height as i32);
            self.gl.use_program(program);
            self.bind_texture(0, self.targets[i].texture);
            if uniforms.texture.is_valid() {
                self.gl.uniform_1i(uniforms.texture, 0);
            }
            self.upload_effect_uniforms(&uniforms, plan, *effect_index);
            self.upload_transform(&Mat3::IDENTITY, 0.0);
            self.upload_opacity(1.0);
            self.draw_quad();
        }

        // The final pass composites the last target onto the framebuffer.
        self.gl.bind_framebuffer(FramebufferTarget::Framebuffer, None);
        self.gl.viewport(0, 0, plan.width as i32, plan.height as i32);
        self.set_blend_state(blend, opacity);
        self.use_passthrough();
        self.bind_texture(0, self.targets[effects.len()].texture);
        self.upload_transform(&Mat3::IDENTITY, if self.flip_final { 1.0 } else { 0.0 });
        self.upload_opacity(opacity);
        self.draw_quad();
        Ok(())
    }

    /// Draw one transition step.
    #[allow(clippy::too_many_arguments)]
    fn draw_transition(
        &mut self,
        plan: &FramePlan,
        from_texture: TextureId,
        to_texture: TextureId,
        transform: Mat3,
        progress: f32,
        effects: &[vimo_render::EffectIndex],
        shaders: &mut dyn ShaderSourceProvider,
    ) -> VimoResult<()> {
        // The two pictures are first placed into off-screen targets with the
        // transition's transform, then blended by the transition shader.
        self.ensure_targets(2)?;
        self.gl.disable(Capability::Blend);
        for (slot, texture) in [from_texture, to_texture].iter().enumerate() {
            self.gl.bind_framebuffer(
                FramebufferTarget::Framebuffer,
                Some(self.targets[slot].framebuffer),
            );
            self.gl.viewport(0, 0, plan.width as i32, plan.height as i32);
            self.bind_texture(0, *texture);
            self.use_passthrough();
            self.upload_transform(&transform, 0.0);
            self.upload_opacity(1.0);
            self.draw_quad();
        }

        // The transition program blends the two targets.
        self.gl.bind_framebuffer(FramebufferTarget::Framebuffer, None);
        self.gl.viewport(0, 0, plan.width as i32, plan.height as i32);
        self.gl.use_program(self.transition);
        let uniforms = self.transition_uniforms.clone();
        self.bind_texture(0, self.targets[0].texture);
        self.bind_texture(1, self.targets[1].texture);
        if uniforms.texture.is_valid() {
            self.gl.uniform_1i(uniforms.texture, 0);
        }
        if uniforms.texture2.is_valid() {
            self.gl.uniform_1i(uniforms.texture2, 1);
        }
        if uniforms.progress.is_valid() {
            self.gl.uniform_1f(uniforms.progress, progress);
        }
        if uniforms.transform.is_valid() {
            self.gl.uniform_matrix_3fv(uniforms.transform, &Mat3::IDENTITY.data);
        }
        if uniforms.aspect.is_valid() {
            let aspect = if self.height == 0 {
                1.0
            } else {
                self.width as f32 / self.height as f32
            };
            self.gl.uniform_1f(uniforms.aspect, aspect);
        }
        if uniforms.flip_y.is_valid() {
            self.gl
                .uniform_1f(uniforms.flip_y, if self.flip_final { 1.0 } else { 0.0 });
        }
        self.draw_quad();

        // Effects on a transition are applied to the blended result.
        if !effects.is_empty() {
            self.ensure_targets(effects.len() + 1)?;
            for (i, effect_index) in effects.iter().enumerate() {
                let Some(effect) = plan.effects.get(effect_index.0 as usize) else {
                    continue;
                };
                let Some(entry) = self.program_for(effect.effect_type, shaders)? else {
                    continue;
                };
                let program = entry.program;
                let effect_uniforms = entry.uniforms.clone();
                self.gl.bind_framebuffer(
                    FramebufferTarget::Framebuffer,
                    Some(self.targets[i + 1].framebuffer),
                );
                self.gl.use_program(program);
                self.bind_texture(0, self.targets[i].texture);
                if effect_uniforms.texture.is_valid() {
                    self.gl.uniform_1i(effect_uniforms.texture, 0);
                }
                self.upload_effect_uniforms(&effect_uniforms, plan, *effect_index);
                self.upload_transform(&Mat3::IDENTITY, 0.0);
                self.draw_quad();
            }
            self.gl.bind_framebuffer(FramebufferTarget::Framebuffer, None);
            self.use_passthrough();
            self.bind_texture(0, self.targets[effects.len()].texture);
            self.upload_transform(&Mat3::IDENTITY, if self.flip_final { 1.0 } else { 0.0 });
            self.draw_quad();
        }
        Ok(())
    }

    /// Upload the uniforms of one effect.
    fn upload_effect_uniforms(
        &mut self,
        uniforms: &UniformLocations,
        plan: &FramePlan,
        effect_index: vimo_render::EffectIndex,
    ) {
        let Some(effect) = plan.effects.get(effect_index.0 as usize) else {
            return;
        };
        let range = effect.range;
        let floats = plan.uniforms.floats_at(range);
        let vec2s = plan.uniforms.vec2s_at(range);
        let vec4s = plan.uniforms.vec4s_at(range);
        let ints = plan.uniforms.ints_at(range);

        // The built-ins.
        if uniforms.time.is_valid() {
            self.gl.uniform_1f(uniforms.time, effect.builtins.time);
        }
        if uniforms.resolution.is_valid() {
            self.gl.uniform_2f(
                uniforms.resolution,
                effect.builtins.resolution[0],
                effect.builtins.resolution[1],
            );
        }
        if uniforms.texel.is_valid() {
            self.gl.uniform_2f(uniforms.texel, effect.builtins.texel[0], effect.builtins.texel[1]);
        }
        if uniforms.opacity.is_valid() {
            self.gl.uniform_1f(uniforms.opacity, effect.builtins.opacity);
        }

        // The manifest parameters, in declaration order. Each type has its own
        // counter because the arena groups values by type.
        let mut float_index = 0usize;
        let mut vec2_index = 0usize;
        let mut vec4_index = 0usize;
        let mut int_index = 0usize;
        let desc = self.registry.get(effect.effect_type);
        let params = desc.map(|d| d.params.len()).unwrap_or(0);
        for (i, location) in uniforms.params.iter().enumerate().take(params) {
            if !location.is_valid() {
                continue;
            }
            let Some(param) = desc.and_then(|d| d.param(i as u16)) else {
                continue;
            };
            match param.ptype {
                vimo_manifest::ParamType::Float => {
                    if let Some(v) = floats.get(float_index) {
                        self.gl.uniform_1f(*location, *v);
                    }
                    float_index += 1;
                }
                vimo_manifest::ParamType::Vec2 => {
                    if let Some(v) = vec2s.get(vec2_index) {
                        self.gl.uniform_2f(*location, v[0], v[1]);
                    }
                    vec2_index += 1;
                }
                vimo_manifest::ParamType::Vec3 | vimo_manifest::ParamType::Color => {
                    if let Some(v) = vec4s.get(vec4_index) {
                        self.gl.uniform_4f(*location, v[0], v[1], v[2], v[3]);
                    }
                    vec4_index += 1;
                }
                vimo_manifest::ParamType::Int
                | vimo_manifest::ParamType::Bool
                | vimo_manifest::ParamType::Enum => {
                    if let Some(v) = ints.get(int_index) {
                        self.gl.uniform_1i(*location, *v);
                    }
                    int_index += 1;
                }
            }
        }
    }

    /// Bind a texture to a unit.
    fn bind_texture(&mut self, unit: u32, texture: TextureId) {
        self.gl.active_texture(unit);
        self.gl.bind_texture(unit, texture);
    }

    /// Make the passthrough program current.
    fn use_passthrough(&mut self) {
        self.gl.use_program(self.passthrough);
        let location = self.passthrough_uniforms.texture;
        if location.is_valid() {
            self.gl.uniform_1i(location, 0);
        }
    }

    /// Make the passthrough program current for an external (surface) texture.
    fn use_external_passthrough(&mut self) {
        self.gl.use_program(self.external_passthrough);
        let location = self.external_uniforms.texture;
        if location.is_valid() {
            self.gl.uniform_1i(location, 0);
        }
    }

    /// Whether a texture needs the external sampler.
    fn is_external(&self, texture: TextureId) -> bool {
        self.gl.is_external_texture(texture)
    }

    /// Upload the transform, aspect and flip uniforms.
    fn upload_transform(&mut self, transform: &Mat3, flip_y: f32) {
        let uniforms = self.passthrough_uniforms.clone();
        if uniforms.transform.is_valid() {
            self.gl.uniform_matrix_3fv(uniforms.transform, &transform.data);
        }
        if uniforms.aspect.is_valid() {
            let aspect = if self.height == 0 {
                1.0
            } else {
                self.width as f32 / self.height as f32
            };
            self.gl.uniform_1f(uniforms.aspect, aspect);
        }
        if uniforms.flip_y.is_valid() {
            self.gl.uniform_1f(uniforms.flip_y, flip_y);
        }
    }

    /// Upload the opacity uniform of the passthrough program.
    fn upload_opacity(&mut self, opacity: f32) {
        let location = self.passthrough_uniforms.opacity;
        if location.is_valid() {
            self.gl.uniform_1f(location, opacity);
        }
    }

    /// Set the blend state for a layer.
    fn set_blend_state(&mut self, blend: BlendMode, _opacity: f32) {
        use vimo_project::model::BlendFactor::*;
        let factors = if blend.is_separable() {
            blend.gl_factors().unwrap_or((One, OneMinusSrcAlpha, One, OneMinusSrcAlpha))
        } else {
            // The non-separable modes need a shader; fall back to "over".
            (One, OneMinusSrcAlpha, One, OneMinusSrcAlpha)
        };
        self.gl.enable(Capability::Blend);
        self.gl.blend_equation(crate::api::consts::FUNC_ADD);
        self.gl.blend_func_separate(
            factor_value(factors.0),
            factor_value(factors.1),
            factor_value(factors.2),
            factor_value(factors.3),
        );
    }

    /// Draw the quad.
    fn draw_quad(&mut self) {
        self.gl.bind_vertex_array(self.quad.vao);
        self.gl.draw_arrays(DrawMode::TriangleStrip, 0, 4);
    }
}

impl Drop for GlRenderer {
    fn drop(&mut self) {
        // The renderer is dropped on the render thread, where the GL context
        // is current, so the objects can be deleted safely.
        self.release_targets();
        self.gl.delete_vertex_array(self.quad.vao);
        self.gl.delete_buffer(self.quad.vbo);
        self.gl.delete_shader(self.vertex_shader);
        self.gl.delete_program(self.passthrough);
        self.gl.delete_program(self.external_passthrough);
        self.gl.delete_program(self.transition);
    }
}

/// The GL constant for a blend factor.
fn factor_value(factor: vimo_project::model::BlendFactor) -> u32 {
    use vimo_project::model::BlendFactor::*;
    match factor {
        Zero => crate::api::consts::ZERO,
        One => crate::api::consts::ONE,
        SrcColor => crate::api::consts::SRC_COLOR,
        OneMinusSrcColor => crate::api::consts::ONE_MINUS_SRC_COLOR,
        DstColor => crate::api::consts::DST_COLOR,
        OneMinusDstColor => crate::api::consts::ONE_MINUS_DST_COLOR,
        SrcAlpha => crate::api::consts::SRC_ALPHA,
        OneMinusSrcAlpha => crate::api::consts::ONE_MINUS_SRC_ALPHA,
        DstAlpha => crate::api::consts::DST_ALPHA,
        OneMinusDstAlpha => crate::api::consts::ONE_MINUS_DST_ALPHA,
        Two => crate::api::consts::TWO,
        OneMinusTwoSrcAlpha => crate::api::consts::ONE_MINUS_TWO_SRC_ALPHA,
        Max => crate::api::consts::MAX,
        Min => crate::api::consts::MIN,
    }
}

/// The error returned when the GL implementation reports a failure.
pub fn graphics_error(context: &'static str, error: GlError) -> VimoError {
    vimo_foundation::error::VimoError::detailed(
        vimo_foundation::ErrorKind::Graphics,
        context,
        format!("{error:?}"),
    )
}

/// The blend equation used by the renderer.
pub const DEFAULT_BLEND_EQUATION: BlendEquation = BlendEquation::Add;
