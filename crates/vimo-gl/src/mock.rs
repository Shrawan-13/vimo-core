//! A mock GL implementation for tests.
//!
//! The mock records every call, so a test can assert exactly what the
//! renderer did: which blend factors were set, which uniforms were uploaded
//! and in what order, how many draw calls were issued and which programs were
//! used.
//!
//! This is what makes the renderer testable without a GPU — and therefore
//! what makes the GL bugs findable on CI instead of on a phone.
//!
//! The mock is a **handle**: cloning it gives another view of the same
//! recording, so a test can keep a clone, hand the original to the renderer
//! and inspect the calls afterwards.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use vimo_foundation::error::{ErrorKind, VimoError, VimoResult};

use crate::api::{
    BufferId, BufferTarget, Capability, ClearMask, DrawMode, FramebufferAttachment, FramebufferId,
    FramebufferTarget, GlApi, GlError, ProgramId, ShaderId, ShaderKind, TextureFormat, TextureId,
    TextureParam, TextureTarget, UniformLocation, VertexArrayId,
};

/// A recorded GL call.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// A shader was compiled.
    CreateShader {
        /// The kind of shader.
        kind: ShaderKind,
        /// The source.
        source: String,
    },
    /// A shader was deleted.
    DeleteShader(ShaderId),
    /// A program was linked.
    CreateProgram {
        /// The vertex shader.
        vertex: ShaderId,
        /// The fragment shader.
        fragment: ShaderId,
    },
    /// A program was deleted.
    DeleteProgram(ProgramId),
    /// A program was made current.
    UseProgram(ProgramId),
    /// A float uniform was uploaded.
    Uniform1f {
        /// The uniform's name.
        name: String,
        /// The value.
        value: f32,
    },
    /// A 2D uniform was uploaded.
    Uniform2f {
        /// The uniform's name.
        name: String,
        /// The values.
        value: [f32; 2],
    },
    /// A 3D uniform was uploaded.
    Uniform3f {
        /// The uniform's name.
        name: String,
        /// The values.
        value: [f32; 3],
    },
    /// A 4D uniform was uploaded.
    Uniform4f {
        /// The uniform's name.
        name: String,
        /// The values.
        value: [f32; 4],
    },
    /// An integer uniform was uploaded.
    Uniform1i {
        /// The uniform's name.
        name: String,
        /// The value.
        value: i32,
    },
    /// A matrix uniform was uploaded.
    UniformMatrix3fv {
        /// The uniform's name.
        name: String,
        /// The values.
        value: [f32; 9],
    },
    /// A texture was created.
    CreateTexture(TextureId),
    /// A texture was deleted.
    DeleteTexture(TextureId),
    /// A texture was bound to a unit.
    BindTexture {
        /// The unit.
        unit: u32,
        /// The texture.
        texture: TextureId,
    },
    /// The active texture unit was selected.
    ActiveTexture(u32),
    /// A texture's storage was allocated.
    TexImage2D {
        /// The width.
        width: u32,
        /// The height.
        height: u32,
        /// The format.
        format: TextureFormat,
    },
    /// A texture parameter was set.
    TexParameter {
        /// The parameter.
        param: TextureParam,
        /// The value.
        value: i32,
    },
    /// A buffer was created.
    CreateBuffer(BufferId),
    /// A buffer was deleted.
    DeleteBuffer(BufferId),
    /// A buffer was bound.
    BindBuffer(BufferTarget, BufferId),
    /// Buffer data was uploaded.
    BufferData {
        /// The size in bytes.
        size: usize,
    },
    /// A vertex array was created.
    CreateVertexArray(VertexArrayId),
    /// A vertex array was deleted.
    DeleteVertexArray(VertexArrayId),
    /// A vertex array was bound.
    BindVertexArray(VertexArrayId),
    /// A vertex attribute was enabled.
    EnableVertexAttrib(u32),
    /// A vertex attribute was described.
    VertexAttribPointer {
        /// The attribute index.
        index: u32,
        /// The number of components.
        size: u32,
        /// The stride.
        stride: u32,
        /// The offset.
        offset: u32,
    },
    /// A framebuffer was created.
    CreateFramebuffer(FramebufferId),
    /// A framebuffer was deleted.
    DeleteFramebuffer(FramebufferId),
    /// A framebuffer was bound.
    BindFramebuffer(Option<FramebufferId>),
    /// A texture was attached to a framebuffer.
    FramebufferTexture(FramebufferAttachment, TextureId),
    /// The viewport was set.
    Viewport {
        /// The width.
        width: i32,
        /// The height.
        height: i32,
    },
    /// The clear colour was set.
    ClearColor([f32; 4]),
    /// The buffers were cleared.
    Clear(ClearMask),
    /// A capability was enabled.
    Enable(Capability),
    /// A capability was disabled.
    Disable(Capability),
    /// The blend function was set.
    BlendFuncSeparate {
        /// The source RGB factor.
        src_rgb: u32,
        /// The destination RGB factor.
        dst_rgb: u32,
        /// The source alpha factor.
        src_alpha: u32,
        /// The destination alpha factor.
        dst_alpha: u32,
    },
    /// The blend equation was set.
    BlendEquation(u32),
    /// The colour write mask was set.
    ColorMask([bool; 4]),
    /// Primitives were drawn.
    DrawArrays {
        /// The primitive mode.
        mode: DrawMode,
        /// The first vertex.
        first: i32,
        /// The number of vertices.
        count: i32,
    },
}

/// The recorded state.
#[derive(Debug, Default)]
struct MockState {
    calls: Vec<Call>,
    sources: HashMap<ShaderId, String>,
    uniform_names: HashMap<ProgramId, Vec<String>>,
    next_id: u32,
    error: GlError,
    draw_calls: usize,
    default_uniforms: Vec<String>,
}

/// A mock GL implementation that records what it was asked to do.
///
/// Cloning the mock gives another view of the same recording.
#[derive(Debug, Clone)]
pub struct MockGl {
    state: Arc<Mutex<MockState>>,
}

impl Default for MockGl {
    fn default() -> Self {
        MockGl::new()
    }
}

impl MockGl {
    /// A mock with no calls recorded.
    #[must_use]
    pub fn new() -> Self {
        MockGl {
            state: Arc::new(Mutex::new(MockState {
                next_id: 1,
                error: GlError::None,
                ..Default::default()
            })),
        }
    }

    /// A mock whose programs expose the given uniform names.
    #[must_use]
    pub fn with_uniforms(names: &[&str]) -> Self {
        let mock = MockGl::new();
        mock.lock().default_uniforms = names.iter().map(|s| s.to_string()).collect();
        mock
    }

    fn lock(&self) -> MutexGuard<'_, MockState> {
        match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    /// Every recorded call, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<Call> {
        self.lock().calls.clone()
    }

    /// The calls matching a predicate.
    #[must_use]
    pub fn calls_of(&self, predicate: impl Fn(&Call) -> bool) -> Vec<Call> {
        self.lock().calls.iter().filter(|c| predicate(c)).cloned().collect()
    }

    /// The number of draw calls.
    #[must_use]
    pub fn draw_calls(&self) -> usize {
        self.lock().draw_calls
    }

    /// The value uploaded to a uniform, if any.
    #[must_use]
    pub fn uniform_value(&self, name: &str) -> Option<f32> {
        self.lock().calls.iter().find_map(|c| match c {
            Call::Uniform1f { name: n, value } if n == name => Some(*value),
            _ => None,
        })
    }

    /// The 2D value uploaded to a uniform, if any.
    #[must_use]
    pub fn uniform_value2(&self, name: &str) -> Option<[f32; 2]> {
        self.lock().calls.iter().find_map(|c| match c {
            Call::Uniform2f { name: n, value } if n == name => Some(*value),
            _ => None,
        })
    }

    /// The last blend function that was set.
    #[must_use]
    pub fn last_blend_func(&self) -> Option<(u32, u32, u32, u32)> {
        self.lock().calls.iter().rev().find_map(|c| match c {
            Call::BlendFuncSeparate { src_rgb, dst_rgb, src_alpha, dst_alpha } => {
                Some((*src_rgb, *dst_rgb, *src_alpha, *dst_alpha))
            }
            _ => None,
        })
    }

    /// The programs that were made current, in order.
    #[must_use]
    pub fn used_programs(&self) -> Vec<ProgramId> {
        self.lock()
            .calls
            .iter()
            .filter_map(|c| match c {
                Call::UseProgram(p) => Some(*p),
                _ => None,
            })
            .collect()
    }

    /// The textures that were bound, in order.
    #[must_use]
    pub fn bound_textures(&self) -> Vec<TextureId> {
        self.lock()
            .calls
            .iter()
            .filter_map(|c| match c {
                Call::BindTexture { texture, .. } => Some(*texture),
                _ => None,
            })
            .collect()
    }

    /// The framebuffers that were bound, in order.
    #[must_use]
    pub fn bound_framebuffers(&self) -> Vec<Option<FramebufferId>> {
        self.lock()
            .calls
            .iter()
            .filter_map(|c| match c {
                Call::BindFramebuffer(fb) => Some(*fb),
                _ => None,
            })
            .collect()
    }

    /// The number of framebuffers that were created.
    #[must_use]
    pub fn created_framebuffers(&self) -> usize {
        self.lock()
            .calls
            .iter()
            .filter(|c| matches!(c, Call::CreateFramebuffer(_)))
            .count()
    }

    /// The number of textures that were created.
    #[must_use]
    pub fn created_textures(&self) -> usize {
        self.lock().calls.iter().filter(|c| matches!(c, Call::CreateTexture(_))).count()
    }

    /// Set the error that `get_error` reports.
    pub fn set_error(&self, error: GlError) {
        self.lock().error = error;
    }

    /// Forget every recorded call.
    pub fn clear(&self) {
        self.lock().calls.clear();
        self.lock().draw_calls = 0;
    }

    /// The name of a uniform, for the recorded calls.
    fn name_of(state: &MockState, program: ProgramId, location: i32) -> String {
        state
            .uniform_names
            .get(&program)
            .and_then(|names| names.get(location.saturating_sub(1) as usize))
            .cloned()
            .unwrap_or_else(|| format!("uniform#{location}"))
    }
}

impl GlApi for MockGl {
    fn create_shader(&mut self, kind: ShaderKind, source: &str) -> VimoResult<ShaderId> {
        let mut state = self.lock();
        let id = ShaderId(state.next_id);
        state.next_id += 1;
        state.sources.insert(id, source.to_string());
        state.calls.push(Call::CreateShader { kind, source: source.to_string() });
        Ok(id)
    }

    fn delete_shader(&mut self, shader: ShaderId) {
        self.lock().calls.push(Call::DeleteShader(shader));
    }

    fn create_program(&mut self, vertex: ShaderId, fragment: ShaderId) -> VimoResult<ProgramId> {
        let mut state = self.lock();
        let id = ProgramId(state.next_id);
        state.next_id += 1;
        let names = state.default_uniforms.clone();
        state.uniform_names.insert(id, names);
        state.calls.push(Call::CreateProgram { vertex, fragment });
        Ok(id)
    }

    fn delete_program(&mut self, program: ProgramId) {
        self.lock().calls.push(Call::DeleteProgram(program));
    }

    fn use_program(&mut self, program: ProgramId) {
        self.lock().calls.push(Call::UseProgram(program));
    }

    fn uniform_location(&mut self, program: ProgramId, name: &str) -> UniformLocation {
        let state = self.lock();
        // Every uniform gets a distinct location, so that the recorded calls
        // name the right uniform.
        let location = state
            .uniform_names
            .get(&program)
            .and_then(|names| names.iter().position(|n| n == name))
            .map(|index| index as i32 + 1)
            .unwrap_or(-1);
        UniformLocation { program, location }
    }

    fn uniform_1f(&mut self, location: UniformLocation, value: f32) {
        let mut state = self.lock();
        let name = MockGl::name_of(&state, location.program, location.location);
        state.calls.push(Call::Uniform1f { name, value });
    }

    fn uniform_2f(&mut self, location: UniformLocation, x: f32, y: f32) {
        let mut state = self.lock();
        let name = MockGl::name_of(&state, location.program, location.location);
        state.calls.push(Call::Uniform2f { name, value: [x, y] });
    }

    fn uniform_3f(&mut self, location: UniformLocation, x: f32, y: f32, z: f32) {
        let mut state = self.lock();
        let name = MockGl::name_of(&state, location.program, location.location);
        state.calls.push(Call::Uniform3f { name, value: [x, y, z] });
    }

    fn uniform_4f(&mut self, location: UniformLocation, x: f32, y: f32, z: f32, w: f32) {
        let mut state = self.lock();
        let name = MockGl::name_of(&state, location.program, location.location);
        state.calls.push(Call::Uniform4f { name, value: [x, y, z, w] });
    }

    fn uniform_1i(&mut self, location: UniformLocation, value: i32) {
        let mut state = self.lock();
        let name = MockGl::name_of(&state, location.program, location.location);
        state.calls.push(Call::Uniform1i { name, value });
    }

    fn uniform_matrix_3fv(&mut self, location: UniformLocation, data: &[f32; 9]) {
        let mut state = self.lock();
        let name = MockGl::name_of(&state, location.program, location.location);
        state.calls.push(Call::UniformMatrix3fv { name, value: *data });
    }

    fn create_texture(&mut self) -> VimoResult<TextureId> {
        let mut state = self.lock();
        let id = TextureId(state.next_id);
        state.next_id += 1;
        state.calls.push(Call::CreateTexture(id));
        Ok(id)
    }

    fn delete_texture(&mut self, texture: TextureId) {
        self.lock().calls.push(Call::DeleteTexture(texture));
    }

    fn bind_texture(&mut self, unit: u32, texture: TextureId) {
        self.lock().calls.push(Call::BindTexture { unit, texture });
    }

    fn active_texture(&mut self, unit: u32) {
        self.lock().calls.push(Call::ActiveTexture(unit));
    }

    fn tex_image_2d(&mut self, _target: TextureTarget, width: u32, height: u32, format: TextureFormat) {
        self.lock().calls.push(Call::TexImage2D { width, height, format });
    }

    fn tex_parameter(&mut self, _target: TextureTarget, param: TextureParam, value: i32) {
        self.lock().calls.push(Call::TexParameter { param, value });
    }

    fn create_buffer(&mut self) -> VimoResult<BufferId> {
        let mut state = self.lock();
        let id = BufferId(state.next_id);
        state.next_id += 1;
        state.calls.push(Call::CreateBuffer(id));
        Ok(id)
    }

    fn delete_buffer(&mut self, buffer: BufferId) {
        self.lock().calls.push(Call::DeleteBuffer(buffer));
    }

    fn bind_buffer(&mut self, target: BufferTarget, buffer: BufferId) {
        self.lock().calls.push(Call::BindBuffer(target, buffer));
    }

    fn buffer_data(&mut self, _target: BufferTarget, size: usize, _data: &[u8]) {
        self.lock().calls.push(Call::BufferData { size });
    }

    fn create_vertex_array(&mut self) -> VimoResult<VertexArrayId> {
        let mut state = self.lock();
        let id = VertexArrayId(state.next_id);
        state.next_id += 1;
        state.calls.push(Call::CreateVertexArray(id));
        Ok(id)
    }

    fn delete_vertex_array(&mut self, vao: VertexArrayId) {
        self.lock().calls.push(Call::DeleteVertexArray(vao));
    }

    fn bind_vertex_array(&mut self, vao: VertexArrayId) {
        self.lock().calls.push(Call::BindVertexArray(vao));
    }

    fn enable_vertex_attrib(&mut self, index: u32) {
        self.lock().calls.push(Call::EnableVertexAttrib(index));
    }

    fn vertex_attrib_pointer(&mut self, index: u32, size: u32, stride: u32, offset: u32) {
        self.lock().calls.push(Call::VertexAttribPointer { index, size, stride, offset });
    }

    fn create_framebuffer(&mut self) -> VimoResult<FramebufferId> {
        let mut state = self.lock();
        let id = FramebufferId(state.next_id);
        state.next_id += 1;
        state.calls.push(Call::CreateFramebuffer(id));
        Ok(id)
    }

    fn delete_framebuffer(&mut self, framebuffer: FramebufferId) {
        self.lock().calls.push(Call::DeleteFramebuffer(framebuffer));
    }

    fn bind_framebuffer(&mut self, _target: FramebufferTarget, framebuffer: Option<FramebufferId>) {
        self.lock().calls.push(Call::BindFramebuffer(framebuffer));
    }

    fn framebuffer_texture(&mut self, attachment: FramebufferAttachment, texture: TextureId) {
        self.lock().calls.push(Call::FramebufferTexture(attachment, texture));
    }

    fn check_framebuffer(&mut self, _framebuffer: FramebufferId) -> VimoResult<()> {
        let error = self.lock().error;
        if error != GlError::None {
            return Err(VimoError::detailed(
                ErrorKind::Graphics,
                "gl: check framebuffer",
                format!("{error:?}"),
            ));
        }
        Ok(())
    }

    fn viewport(&mut self, _x: i32, _y: i32, width: i32, height: i32) {
        self.lock().calls.push(Call::Viewport { width, height });
    }

    fn clear_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        self.lock().calls.push(Call::ClearColor([r, g, b, a]));
    }

    fn clear(&mut self, mask: ClearMask) {
        self.lock().calls.push(Call::Clear(mask));
    }

    fn enable(&mut self, capability: Capability) {
        self.lock().calls.push(Call::Enable(capability));
    }

    fn disable(&mut self, capability: Capability) {
        self.lock().calls.push(Call::Disable(capability));
    }

    fn blend_func(&mut self, src: u32, dst: u32) {
        self.lock()
            .calls
            .push(Call::BlendFuncSeparate { src_rgb: src, dst_rgb: dst, src_alpha: src, dst_alpha: dst });
    }

    fn blend_func_separate(&mut self, src_rgb: u32, dst_rgb: u32, src_alpha: u32, dst_alpha: u32) {
        self.lock().calls.push(Call::BlendFuncSeparate { src_rgb, dst_rgb, src_alpha, dst_alpha });
    }

    fn blend_equation(&mut self, equation: u32) {
        self.lock().calls.push(Call::BlendEquation(equation));
    }

    fn color_mask(&mut self, r: bool, g: bool, b: bool, a: bool) {
        self.lock().calls.push(Call::ColorMask([r, g, b, a]));
    }

    fn draw_arrays(&mut self, mode: DrawMode, first: i32, count: i32) {
        let mut state = self.lock();
        state.draw_calls += 1;
        state.calls.push(Call::DrawArrays { mode, first, count });
    }

    fn read_pixels(&mut self, _x: i32, _y: i32, _width: i32, _height: i32, out: &mut [u8]) {
        for b in out.iter_mut() {
            *b = 0;
        }
    }

    fn get_error(&mut self) -> GlError {
        self.lock().error
    }

    fn finish(&mut self) {}
}
