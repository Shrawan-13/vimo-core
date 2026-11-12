//! The GL API surface.
//!
//! Vimo Core does not link against OpenGL ES directly. It talks to this
//! trait, which the Android layer implements on top of the NDK's
//! `GLES3/gl3.h` and a desktop build implements on top of a desktop GL
//! loader.
//!
//! Keeping the boundary at this level has three advantages:
//!
//! * The renderer — where the bugs live — is testable on a desktop CI with
//!   [`crate::mock::MockGl`], which records every call.
//! * The core keeps its zero-dependency promise.
//! * Swapping GLES 3.0 for GLES 2.0 (older devices) is a change of
//!   implementation, not of architecture.

use vimo_foundation::error::{ErrorKind, VimoError, VimoResult};

/// A shader object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ShaderId(pub u32);

/// A linked program object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ProgramId(pub u32);

/// A texture object.
///
/// This is the same handle the render graph uses, so a plan's texture
/// references can be passed straight to the GL layer.
pub use vimo_render::TextureId;

/// A buffer object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct BufferId(pub u32);

/// A vertex array object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct VertexArrayId(pub u32);

/// A framebuffer object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct FramebufferId(pub u32);

/// A uniform location inside a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UniformLocation {
    /// The program the location belongs to.
    pub program: ProgramId,
    /// The location index, or `-1` when the uniform does not exist.
    pub location: i32,
}

impl Default for UniformLocation {
    fn default() -> Self {
        UniformLocation {
            program: ProgramId(0),
            location: -1,
        }
    }
}

impl UniformLocation {
    /// Whether the uniform exists.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.location >= 0
    }
}

/// The kind of shader being compiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderKind {
    /// A vertex shader.
    Vertex,
    /// A fragment shader.
    Fragment,
}

/// The texture target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureTarget {
    /// `GL_TEXTURE_2D`
    Texture2D,
}

/// The internal format of a texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFormat {
    /// `GL_RGBA8`
    Rgba8,
}

/// A texture parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureParam {
    /// `GL_TEXTURE_MIN_FILTER`
    MinFilter,
    /// `GL_TEXTURE_MAG_FILTER`
    MagFilter,
    /// `GL_TEXTURE_WRAP_S`
    WrapS,
    /// `GL_TEXTURE_WRAP_T`
    WrapT,
}

/// The filter applied when a texture is minified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFilter {
    /// `GL_NEAREST`
    Nearest,
    /// `GL_LINEAR`
    Linear,
}

/// The wrap mode of a texture coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureWrap {
    /// `GL_CLAMP_TO_EDGE`
    ClampToEdge,
    /// `GL_REPEAT`
    Repeat,
}

/// The buffer target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferTarget {
    /// `GL_ARRAY_BUFFER`
    ArrayBuffer,
}

/// The framebuffer target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramebufferTarget {
    /// `GL_FRAMEBUFFER`
    Framebuffer,
}

/// The framebuffer attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramebufferAttachment {
    /// `GL_COLOR_ATTACHMENT0`
    Color0,
}

/// A GL capability (an `enable`/`disable` bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// `GL_BLEND`
    Blend,
    /// `GL_DEPTH_TEST`
    DepthTest,
    /// `GL_CULL_FACE`
    CullFace,
    /// `GL_SCISSOR_TEST`
    ScissorTest,
}

/// The blend equation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendEquation {
    /// `GL_FUNC_ADD`
    Add,
    /// `GL_FUNC_SUBTRACT`
    Subtract,
    /// `GL_FUNC_REVERSE_SUBTRACT`
    ReverseSubtract,
}

/// The primitive to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawMode {
    /// `GL_TRIANGLES`
    Triangles,
    /// `GL_TRIANGLE_STRIP`
    TriangleStrip,
}

/// The bits cleared by `clear`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClearMask(pub u32);

impl ClearMask {
    /// `GL_COLOR_BUFFER_BIT`
    pub const COLOR: ClearMask = ClearMask(0x0000_4000);
    /// `GL_DEPTH_BUFFER_BIT`
    pub const DEPTH: ClearMask = ClearMask(0x0000_0100);
}

/// A GL error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlError {
    /// No error.
    #[default]
    None,
    /// `GL_INVALID_ENUM`
    InvalidEnum,
    /// `GL_INVALID_VALUE`
    InvalidValue,
    /// `GL_INVALID_OPERATION`
    InvalidOperation,
    /// `GL_OUT_OF_MEMORY`
    OutOfMemory,
    /// `GL_INVALID_FRAMEBUFFER_OPERATION`
    InvalidFramebufferOperation,
    /// An unknown code.
    Other(u32),
}

impl GlError {
    /// Decode a raw `glGetError` value.
    #[must_use]
    pub const fn from_raw(v: u32) -> GlError {
        match v {
            0 => GlError::None,
            0x0500 => GlError::InvalidEnum,
            0x0501 => GlError::InvalidValue,
            0x0502 => GlError::InvalidOperation,
            0x0505 => GlError::OutOfMemory,
            0x0506 => GlError::InvalidFramebufferOperation,
            other => GlError::Other(other),
        }
    }
}

/// A GL constant.
pub mod consts {
    /// `GL_NO_ERROR`
    pub const NO_ERROR: u32 = 0;
    /// `GL_ARRAY_BUFFER`
    pub const ARRAY_BUFFER: u32 = 0x8892;
    /// `GL_STATIC_DRAW`
    pub const STATIC_DRAW: u32 = 0x88E4;
    /// `GL_TEXTURE_2D`
    pub const TEXTURE_2D: u32 = 0x0DE1;
    /// `GL_TEXTURE0`
    pub const TEXTURE0: u32 = 0x84C0;
    /// `GL_RGBA8`
    pub const RGBA8: u32 = 0x8058;
    /// `GL_RGBA`
    pub const RGBA: u32 = 0x1908;
    /// `GL_UNSIGNED_BYTE`
    pub const UNSIGNED_BYTE: u32 = 0x1401;
    /// `GL_TEXTURE_MIN_FILTER`
    pub const TEXTURE_MIN_FILTER: u32 = 0x2801;
    /// `GL_TEXTURE_MAG_FILTER`
    pub const TEXTURE_MAG_FILTER: u32 = 0x2800;
    /// `GL_TEXTURE_WRAP_S`
    pub const TEXTURE_WRAP_S: u32 = 0x2802;
    /// `GL_TEXTURE_WRAP_T`
    pub const TEXTURE_WRAP_T: u32 = 0x2803;
    /// `GL_NEAREST`
    pub const NEAREST: u32 = 0x2600;
    /// `GL_LINEAR`
    pub const LINEAR: u32 = 0x2601;
    /// `GL_CLAMP_TO_EDGE`
    pub const CLAMP_TO_EDGE: u32 = 0x812F;
    /// `GL_REPEAT`
    pub const REPEAT: u32 = 0x2901;
    /// `GL_FRAMEBUFFER`
    pub const FRAMEBUFFER: u32 = 0x8D40;
    /// `GL_COLOR_ATTACHMENT0`
    pub const COLOR_ATTACHMENT0: u32 = 0x8CE0;
    /// `GL_FRAMEBUFFER_COMPLETE`
    pub const FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
    /// `GL_BLEND`
    pub const BLEND: u32 = 0x0BE2;
    /// `GL_DEPTH_TEST`
    pub const DEPTH_TEST: u32 = 0x0B71;
    /// `GL_CULL_FACE`
    pub const CULL_FACE: u32 = 0x0B44;
    /// `GL_SCISSOR_TEST`
    pub const SCISSOR_TEST: u32 = 0x0C11;
    /// `GL_ZERO`
    pub const ZERO: u32 = 0;
    /// `GL_ONE`
    pub const ONE: u32 = 1;
    /// `GL_SRC_COLOR`
    pub const SRC_COLOR: u32 = 0x0300;
    /// `GL_ONE_MINUS_SRC_COLOR`
    pub const ONE_MINUS_SRC_COLOR: u32 = 0x0301;
    /// `GL_DST_COLOR`
    pub const DST_COLOR: u32 = 0x0306;
    /// `GL_ONE_MINUS_DST_COLOR`
    pub const ONE_MINUS_DST_COLOR: u32 = 0x0307;
    /// `GL_SRC_ALPHA`
    pub const SRC_ALPHA: u32 = 0x0302;
    /// `GL_ONE_MINUS_SRC_ALPHA`
    pub const ONE_MINUS_SRC_ALPHA: u32 = 0x0303;
    /// `GL_DST_ALPHA`
    pub const DST_ALPHA: u32 = 0x0304;
    /// `GL_ONE_MINUS_DST_ALPHA`
    pub const ONE_MINUS_DST_ALPHA: u32 = 0x0305;
    /// `GL_TWO`
    pub const TWO: u32 = 0x0801;
    /// `GL_ONE_MINUS_TWO_SRC_ALPHA`
    pub const ONE_MINUS_TWO_SRC_ALPHA: u32 = 0x0803;
    /// `GL_MAX`
    pub const MAX: u32 = 0x8008;
    /// `GL_MIN`
    pub const MIN: u32 = 0x8007;
    /// `GL_FUNC_ADD`
    pub const FUNC_ADD: u32 = 0x8006;
    /// `GL_FUNC_SUBTRACT`
    pub const FUNC_SUBTRACT: u32 = 0x800A;
    /// `GL_FUNC_REVERSE_SUBTRACT`
    pub const FUNC_REVERSE_SUBTRACT: u32 = 0x800B;
    /// `GL_TRIANGLES`
    pub const TRIANGLES: u32 = 0x0004;
    /// `GL_TRIANGLE_STRIP`
    pub const TRIANGLE_STRIP: u32 = 0x0005;
    /// `GL_VERTEX_SHADER`
    pub const VERTEX_SHADER: u32 = 0x8B31;
    /// `GL_FRAGMENT_SHADER`
    pub const FRAGMENT_SHADER: u32 = 0x8B30;
    /// `GL_COMPILE_STATUS`
    pub const COMPILE_STATUS: u32 = 0x8B81;
    /// `GL_LINK_STATUS`
    pub const LINK_STATUS: u32 = 0x8B82;
    /// `GL_INFO_LOG_LENGTH`
    pub const INFO_LOG_LENGTH: u32 = 0x8B84;
}

/// The subset of OpenGL ES that Vimo uses.
///
/// Implementations are expected to be cheap: these are called once per draw
/// call, not once per pixel.
pub trait GlApi: Send {
    // --- shaders and programs -------------------------------------------
    /// Compile a shader.
    fn create_shader(&mut self, kind: ShaderKind, source: &str) -> VimoResult<ShaderId>;
    /// Delete a shader.
    fn delete_shader(&mut self, shader: ShaderId);
    /// Link a program from a vertex and a fragment shader.
    fn create_program(&mut self, vertex: ShaderId, fragment: ShaderId) -> VimoResult<ProgramId>;
    /// Delete a program.
    fn delete_program(&mut self, program: ProgramId);
    /// Make a program current.
    fn use_program(&mut self, program: ProgramId);
    /// Look up a uniform location.
    fn uniform_location(&mut self, program: ProgramId, name: &str) -> UniformLocation;
    /// Upload one float.
    fn uniform_1f(&mut self, location: UniformLocation, value: f32);
    /// Upload two floats.
    fn uniform_2f(&mut self, location: UniformLocation, x: f32, y: f32);
    /// Upload three floats.
    fn uniform_3f(&mut self, location: UniformLocation, x: f32, y: f32, z: f32);
    /// Upload four floats.
    fn uniform_4f(&mut self, location: UniformLocation, x: f32, y: f32, z: f32, w: f32);
    /// Upload one integer.
    fn uniform_1i(&mut self, location: UniformLocation, value: i32);
    /// Upload a 3x3 matrix (column major).
    fn uniform_matrix_3fv(&mut self, location: UniformLocation, data: &[f32; 9]);

    // --- textures -------------------------------------------------------
    /// Create a texture.
    fn create_texture(&mut self) -> VimoResult<TextureId>;
    /// Delete a texture.
    fn delete_texture(&mut self, texture: TextureId);
    /// Make a texture active on a unit.
    fn bind_texture(&mut self, unit: u32, texture: TextureId);
    /// Select the active texture unit.
    fn active_texture(&mut self, unit: u32);
    /// Allocate the storage of a texture.
    fn tex_image_2d(
        &mut self,
        target: TextureTarget,
        width: u32,
        height: u32,
        format: TextureFormat,
    );
    /// Set a texture parameter.
    fn tex_parameter(&mut self, target: TextureTarget, param: TextureParam, value: i32);

    /// Create a texture backed by an `ANativeWindow`.
    ///
    /// This is the zero-copy path: the decoder writes into the window and the
    /// GPU samples it directly, with no copy in between. The default
    /// implementation reports that the platform does not support it.
    fn create_external_texture(
        &mut self,
        _native_window: u64,
        _width: u32,
        _height: u32,
    ) -> VimoResult<TextureId> {
        Err(vimo_foundation::error::unsupported(
            "gl: external textures are not supported by this platform",
        ))
    }

    /// Whether a texture is backed by a surface (and therefore needs the
    /// external sampler).
    fn is_external_texture(&self, _texture: TextureId) -> bool {
        false
    }

    // --- buffers and vertex arrays -------------------------------------
    /// Create a buffer.
    fn create_buffer(&mut self) -> VimoResult<BufferId>;
    /// Delete a buffer.
    fn delete_buffer(&mut self, buffer: BufferId);
    /// Bind a buffer to a target.
    fn bind_buffer(&mut self, target: BufferTarget, buffer: BufferId);
    /// Upload data into the bound buffer.
    fn buffer_data(&mut self, target: BufferTarget, size: usize, data: &[u8]);
    /// Create a vertex array.
    fn create_vertex_array(&mut self) -> VimoResult<VertexArrayId>;
    /// Delete a vertex array.
    fn delete_vertex_array(&mut self, vao: VertexArrayId);
    /// Bind a vertex array.
    fn bind_vertex_array(&mut self, vao: VertexArrayId);
    /// Enable a vertex attribute array.
    fn enable_vertex_attrib(&mut self, index: u32);
    /// Describe the layout of a vertex attribute.
    fn vertex_attrib_pointer(&mut self, index: u32, size: u32, stride: u32, offset: u32);

    // --- framebuffers ---------------------------------------------------
    /// Create a framebuffer.
    fn create_framebuffer(&mut self) -> VimoResult<FramebufferId>;
    /// Delete a framebuffer.
    fn delete_framebuffer(&mut self, framebuffer: FramebufferId);
    /// Bind a framebuffer (`None` binds the default one).
    fn bind_framebuffer(&mut self, target: FramebufferTarget, framebuffer: Option<FramebufferId>);
    /// Attach a texture to a framebuffer.
    fn framebuffer_texture(&mut self, attachment: FramebufferAttachment, texture: TextureId);
    /// Check that a framebuffer is complete.
    fn check_framebuffer(&mut self, framebuffer: FramebufferId) -> VimoResult<()>;

    // --- state ----------------------------------------------------------
    /// Set the viewport.
    fn viewport(&mut self, x: i32, y: i32, width: i32, height: i32);
    /// Set the clear colour.
    fn clear_color(&mut self, r: f32, g: f32, b: f32, a: f32);
    /// Clear the given buffers.
    fn clear(&mut self, mask: ClearMask);
    /// Enable a capability.
    fn enable(&mut self, capability: Capability);
    /// Disable a capability.
    fn disable(&mut self, capability: Capability);
    /// Set the blend function.
    fn blend_func(&mut self, src: u32, dst: u32);
    /// Set the blend function for RGB and alpha separately.
    fn blend_func_separate(&mut self, src_rgb: u32, dst_rgb: u32, src_alpha: u32, dst_alpha: u32);
    /// Set the blend equation.
    fn blend_equation(&mut self, equation: u32);
    /// Set the colour write mask.
    fn color_mask(&mut self, r: bool, g: bool, b: bool, a: bool);
    /// Draw primitives.
    fn draw_arrays(&mut self, mode: DrawMode, first: i32, count: i32);
    /// Read pixels back (used by the golden tests and the frame grabber).
    fn read_pixels(&mut self, x: i32, y: i32, width: i32, height: i32, out: &mut [u8]);
    /// Fetch the last error.
    fn get_error(&mut self) -> GlError;
    /// Block until the GPU is done.
    fn finish(&mut self);
}

/// Turn a GL error into a [`VimoError`].
pub fn gl_error_to_vimo(context: &'static str, error: GlError) -> Option<VimoError> {
    match error {
        GlError::None => None,
        other => Some(VimoError::detailed(
            ErrorKind::Graphics,
            context,
            format!("{other:?}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_locations_report_validity() {
        let l = UniformLocation {
            program: ProgramId(1),
            location: -1,
        };
        assert!(!l.is_valid());
        let l = UniformLocation {
            program: ProgramId(1),
            location: 3,
        };
        assert!(l.is_valid());
    }

    #[test]
    fn gl_errors_decode() {
        assert_eq!(GlError::from_raw(0), GlError::None);
        assert_eq!(GlError::from_raw(0x0500), GlError::InvalidEnum);
        assert_eq!(GlError::from_raw(0x9999), GlError::Other(0x9999));
    }
}
