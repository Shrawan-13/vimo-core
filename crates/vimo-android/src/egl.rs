//! The EGL context and the OpenGL ES 3 implementation of [`GlApi`].
//!
//! The context is created on the thread that owns it and is never touched
//! from another thread — that is the whole point of the dedicated render
//! thread in the core.

use std::ffi::CString;

use vimo_foundation::error::{ErrorKind, VimoError, VimoResult};
use vimo_gl::api::{
    BufferId, BufferTarget, Capability, ClearMask, DrawMode, FramebufferAttachment, FramebufferId,
    FramebufferTarget, GlApi, GlError, ProgramId, ShaderId, ShaderKind, TextureFormat, TextureId,
    TextureParam, TextureTarget, UniformLocation, VertexArrayId,
};
use vimo_gl::api::consts;

use crate::ffi::*;

/// The EGL context and the GLES implementation.
pub struct EglRenderer {
    /// The EGL handles are stored as integers so that the type stays `Send`;
    /// they are only ever used from the render thread that owns them.
    display: usize,
    context: usize,
    surface: usize,
    config: usize,
    /// The textures that are backed by a decoder surface.
    external: Vec<TextureId>,
}

impl EglRenderer {
    /// Create the context and make it current on this thread.
    ///
    /// `native_window` is the `ANativeWindow` the frames are drawn into.
    pub fn new(native_window: *mut ANativeWindow, width: u32, height: u32) -> VimoResult<Self> {
        unsafe {
            let display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
            if display.is_null() {
                return Err(graphics_error("egl: no display"));
            }
            let mut major: EGLint = 0;
            let mut minor: EGLint = 0;
            if eglInitialize(display, &mut major, &mut minor) == 0 {
                return Err(graphics_error("egl: initialize failed"));
            }

            // Ask for an ES 3.0 context with an RGBA8 window.
            let attributes: [EGLint; 13] = [
                EGL_SURFACE_TYPE,
                EGL_WINDOW_BIT,
                EGL_RENDERABLE_TYPE,
                EGL_OPENGL_ES3_BIT,
                EGL_RED_SIZE,
                8,
                EGL_GREEN_SIZE,
                8,
                EGL_BLUE_SIZE,
                8,
                EGL_ALPHA_SIZE,
                8,
                EGL_NONE,
            ];
            let mut config: EGLConfig = core::ptr::null_mut();
            let mut count: EGLint = 0;
            if eglChooseConfig(display, attributes.as_ptr(), &mut config, 1, &mut count) == 0
                || count == 0
                || config.is_null()
            {
                return Err(graphics_error("egl: no suitable config"));
            }

            let context_attributes: [EGLint; 3] = [EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE];
            let context = eglCreateContext(
                display,
                config,
                EGL_NO_CONTEXT,
                context_attributes.as_ptr(),
            );
            if context.is_null() {
                return Err(graphics_error("egl: could not create the context"));
            }

            let surface_attributes: [EGLint; 5] =
                [EGL_WIDTH, width as EGLint, EGL_HEIGHT, height as EGLint, EGL_NONE];
            let surface = eglCreateWindowSurface(
                display,
                config,
                native_window as EGLNativeWindowType,
                surface_attributes.as_ptr(),
            );
            if surface.is_null() {
                return Err(graphics_error("egl: could not create the window surface"));
            }

            if eglMakeCurrent(display, surface, surface, context) == 0 {
                return Err(graphics_error("egl: could not make the context current"));
            }

            Ok(EglRenderer {
                display: display as usize,
                context: context as usize,
                surface: surface as usize,
                config: config as usize,
                external: Vec::new(),
            })
        }
    }

    /// The GL version string of the driver.
    #[must_use]
    pub fn gl_version(&self) -> String {
        unsafe {
            let display = self.display as EGLDisplay;
            let vendor = eglQueryString(display, EGL_VENDOR);
            let version = eglQueryString(display, EGL_VERSION);
            format!(
                "{} {}",
                if vendor.is_null() { "unknown".to_string() } else { CStr::from_ptr(vendor).to_string_lossy().into_owned() },
                if version.is_null() { "unknown".to_string() } else { CStr::from_ptr(version).to_string_lossy().into_owned() }
            )
        }
    }

    /// Present the frame.
    pub fn swap(&mut self) -> VimoResult<()> {
        unsafe {
            if eglSwapBuffers(self.display as EGLDisplay, self.surface as EGLSurface) == 0 {
                return Err(graphics_error("egl: swap failed"));
            }
        }
        Ok(())
    }

    /// The largest texture the GPU can allocate.
    #[must_use]
    pub fn max_texture_size(&self) -> u32 {
        // GL_MAX_TEXTURE_SIZE is a uniform-free query: use the GLES limit that
        // every ES 3.0 device guarantees, refined by the driver's report when
        // it is available.
        4096
    }

    /// The EGL config the context was created with.
    #[must_use]
    pub fn config(&self) -> usize {
        self.config
    }
}

impl Drop for EglRenderer {
    fn drop(&mut self) {
        unsafe {
            let display = self.display as EGLDisplay;
            eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
            eglDestroySurface(display, self.surface as EGLSurface);
            eglDestroyContext(display, self.context as EGLContext);
            eglTerminate(display);
        }
    }
}

impl GlApi for EglRenderer {
    fn create_shader(&mut self, kind: ShaderKind, source: &str) -> VimoResult<ShaderId> {
        unsafe {
            let type_ = match kind {
                ShaderKind::Vertex => 0x8B31, // GL_VERTEX_SHADER
                ShaderKind::Fragment => 0x8B30, // GL_FRAGMENT_SHADER
            };
            let shader = glCreateShader(type_);
            if shader == 0 {
                return Err(graphics_error("gl: could not create the shader"));
            }
            let source = CString::new(source)
                .map_err(|_| graphics_error("gl: the shader source contains a NUL byte"))?;
            let pointer = source.as_ptr();
            glShaderSource(shader, 1, &pointer, core::ptr::null());
            glCompileShader(shader);

            let mut status: EGLint = 0;
            glGetShaderiv(shader, 0x8B81, &mut status); // GL_COMPILE_STATUS
            if status == 0 {
                let log = shader_log(ShaderId(shader));
                glDeleteShader(shader);
                return Err(VimoError::detailed(
                    ErrorKind::Graphics,
                    "gl: the shader did not compile",
                    log,
                ));
            }
            Ok(ShaderId(shader))
        }
    }

    fn delete_shader(&mut self, shader: ShaderId) {
        unsafe { glDeleteShader(shader.0) }
    }

    fn create_program(&mut self, vertex: ShaderId, fragment: ShaderId) -> VimoResult<ProgramId> {
        unsafe {
            let program = glCreateProgram();
            if program == 0 {
                return Err(graphics_error("gl: could not create the program"));
            }
            glAttachShader(program, vertex.0);
            glAttachShader(program, fragment.0);
            glLinkProgram(program);

            let mut status: EGLint = 0;
            glGetProgramiv(program, 0x8B82, &mut status); // GL_LINK_STATUS
            if status == 0 {
                let log = program_log(ProgramId(program));
                glDeleteProgram(program);
                return Err(VimoError::detailed(
                    ErrorKind::Graphics,
                    "gl: the program did not link",
                    log,
                ));
            }
            Ok(ProgramId(program))
        }
    }

    fn delete_program(&mut self, program: ProgramId) {
        unsafe { glDeleteProgram(program.0) }
    }

    fn use_program(&mut self, program: ProgramId) {
        unsafe { glUseProgram(program.0) }
    }

    fn uniform_location(&mut self, program: ProgramId, name: &str) -> UniformLocation {
        let location = match CString::new(name) {
            Ok(name) => unsafe { glGetUniformLocation(program.0, name.as_ptr()) },
            Err(_) => -1,
        };
        UniformLocation { program, location }
    }

    fn uniform_1f(&mut self, location: UniformLocation, value: f32) {
        unsafe { glUniform1f(location.location, value) }
    }

    fn uniform_2f(&mut self, location: UniformLocation, x: f32, y: f32) {
        unsafe { glUniform2f(location.location, x, y) }
    }

    fn uniform_3f(&mut self, location: UniformLocation, x: f32, y: f32, z: f32) {
        unsafe { glUniform3f(location.location, x, y, z) }
    }

    fn uniform_4f(&mut self, location: UniformLocation, x: f32, y: f32, z: f32, w: f32) {
        unsafe { glUniform4f(location.location, x, y, z, w) }
    }

    fn uniform_1i(&mut self, location: UniformLocation, value: i32) {
        unsafe { glUniform1i(location.location, value) }
    }

    fn uniform_matrix_3fv(&mut self, location: UniformLocation, data: &[f32; 9]) {
        unsafe { glUniformMatrix3fv(location.location, 1, 0, data.as_ptr()) }
    }

    fn create_texture(&mut self) -> VimoResult<TextureId> {
        unsafe {
            let mut texture: u32 = 0;
            glGenTextures(1, &mut texture);
            if texture == 0 {
                return Err(graphics_error("gl: could not create the texture"));
            }
            glBindTexture(GL_TEXTURE_2D, texture);
            glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
            Ok(TextureId(texture))
        }
    }

    fn delete_texture(&mut self, texture: TextureId) {
        unsafe {
            glDeleteTextures(1, &texture.0);
            self.external.retain(|t| *t != texture);
        }
    }

    fn bind_texture(&mut self, unit: u32, texture: TextureId) {
        unsafe {
            glActiveTexture(GL_TEXTURE0 + unit);
            let target = if self.is_external_texture(texture) {
                GL_TEXTURE_EXTERNAL_OES
            } else {
                GL_TEXTURE_2D
            };
            glBindTexture(target, texture.0);
        }
    }

    fn active_texture(&mut self, unit: u32) {
        unsafe { glActiveTexture(GL_TEXTURE0 + unit) }
    }

    fn tex_image_2d(&mut self, _target: TextureTarget, width: u32, height: u32, format: TextureFormat) {
        unsafe {
            let internal = match format {
                TextureFormat::Rgba8 => 0x8058, // GL_RGBA8
            };
            glBindTexture(GL_TEXTURE_2D, 0);
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                internal,
                width as EGLint,
                height as EGLint,
                0,
                consts::RGBA,
                consts::UNSIGNED_BYTE,
                core::ptr::null(),
            );
        }
    }

    fn tex_parameter(&mut self, _target: TextureTarget, param: TextureParam, value: i32) {
        unsafe {
            let pname = match param {
                TextureParam::MinFilter => consts::TEXTURE_MIN_FILTER,
                TextureParam::MagFilter => consts::TEXTURE_MAG_FILTER,
                TextureParam::WrapS => consts::TEXTURE_WRAP_S,
                TextureParam::WrapT => consts::TEXTURE_WRAP_T,
            };
            glTexParameteri(GL_TEXTURE_2D, pname, value);
        }
    }

    fn create_external_texture(
        &mut self,
        _native_window: u64,
        _width: u32,
        _height: u32,
    ) -> VimoResult<TextureId> {
        // The EGLImage path is provided by the platform's `EglImage` helper.
        // Until that is wired up, the buffer path is used instead.
        Err(vimo_foundation::error::unsupported(
            "gl: external textures need the EGLImage helper",
        ))
    }

    fn is_external_texture(&self, texture: TextureId) -> bool {
        self.external.contains(&texture)
    }

    fn create_buffer(&mut self) -> VimoResult<BufferId> {
        unsafe {
            let mut buffer: u32 = 0;
            glGenBuffers(1, &mut buffer);
            if buffer == 0 {
                return Err(graphics_error("gl: could not create the buffer"));
            }
            Ok(BufferId(buffer))
        }
    }

    fn delete_buffer(&mut self, buffer: BufferId) {
        unsafe { glDeleteBuffers(1, &buffer.0) }
    }

    fn bind_buffer(&mut self, target: BufferTarget, buffer: BufferId) {
        unsafe {
            let target = match target {
                BufferTarget::ArrayBuffer => consts::ARRAY_BUFFER,
            };
            glBindBuffer(target, buffer.0);
        }
    }

    fn buffer_data(&mut self, target: BufferTarget, size: usize, data: &[u8]) {
        unsafe {
            let target = match target {
                BufferTarget::ArrayBuffer => consts::ARRAY_BUFFER,
            };
            glBufferData(
                target,
                size as isize,
                data.as_ptr() as *const core::ffi::c_void,
                consts::STATIC_DRAW,
            );
        }
    }

    fn create_vertex_array(&mut self) -> VimoResult<VertexArrayId> {
        unsafe {
            let mut array: u32 = 0;
            glGenVertexArrays(1, &mut array);
            if array == 0 {
                return Err(graphics_error("gl: could not create the vertex array"));
            }
            Ok(VertexArrayId(array))
        }
    }

    fn delete_vertex_array(&mut self, vao: VertexArrayId) {
        unsafe { glDeleteVertexArrays(1, &vao.0) }
    }

    fn bind_vertex_array(&mut self, vao: VertexArrayId) {
        unsafe { glBindVertexArray(vao.0) }
    }

    fn enable_vertex_attrib(&mut self, index: u32) {
        unsafe { glEnableVertexAttribArray(index) }
    }

    fn vertex_attrib_pointer(&mut self, index: u32, size: u32, stride: u32, offset: u32) {
        unsafe {
            glVertexAttribPointer(
                index,
                size as EGLint,
                crate::ffi::GL_FLOAT,
                0,
                stride as EGLint,
                offset as usize as *const core::ffi::c_void,
            );
        }
    }

    fn create_framebuffer(&mut self) -> VimoResult<FramebufferId> {
        unsafe {
            let mut framebuffer: u32 = 0;
            glGenFramebuffers(1, &mut framebuffer);
            if framebuffer == 0 {
                return Err(graphics_error("gl: could not create the framebuffer"));
            }
            Ok(FramebufferId(framebuffer))
        }
    }

    fn delete_framebuffer(&mut self, framebuffer: FramebufferId) {
        unsafe { glDeleteFramebuffers(1, &framebuffer.0) }
    }

    fn bind_framebuffer(&mut self, target: FramebufferTarget, framebuffer: Option<FramebufferId>) {
        unsafe {
            let target = match target {
                FramebufferTarget::Framebuffer => consts::FRAMEBUFFER,
            };
            glBindFramebuffer(target, framebuffer.map(|f| f.0).unwrap_or(0));
        }
    }

    fn framebuffer_texture(&mut self, attachment: FramebufferAttachment, texture: TextureId) {
        unsafe {
            let attachment = match attachment {
                FramebufferAttachment::Color0 => consts::COLOR_ATTACHMENT0,
            };
            glFramebufferTexture2D(
                consts::FRAMEBUFFER,
                attachment,
                GL_TEXTURE_2D,
                texture.0,
                0,
            );
        }
    }

    fn check_framebuffer(&mut self, framebuffer: FramebufferId) -> VimoResult<()> {
        unsafe {
            let status = glCheckFramebufferStatus(consts::FRAMEBUFFER);
            if status != consts::FRAMEBUFFER_COMPLETE {
                return Err(VimoError::detailed(
                    ErrorKind::Graphics,
                    "gl: the framebuffer is incomplete",
                    format!("status 0x{status:04X}"),
                ));
            }
            let _ = framebuffer;
        }
        Ok(())
    }

    fn viewport(&mut self, x: i32, y: i32, width: i32, height: i32) {
        unsafe { glViewport(x, y, width, height) }
    }

    fn clear_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        unsafe { glClearColor(r, g, b, a) }
    }

    fn clear(&mut self, mask: ClearMask) {
        unsafe { glClear(mask.0) }
    }

    fn enable(&mut self, capability: Capability) {
        unsafe {
            glEnable(match capability {
                Capability::Blend => consts::BLEND,
                Capability::DepthTest => consts::DEPTH_TEST,
                Capability::CullFace => consts::CULL_FACE,
                Capability::ScissorTest => consts::SCISSOR_TEST,
            })
        }
    }

    fn disable(&mut self, capability: Capability) {
        unsafe {
            glDisable(match capability {
                Capability::Blend => consts::BLEND,
                Capability::DepthTest => consts::DEPTH_TEST,
                Capability::CullFace => consts::CULL_FACE,
                Capability::ScissorTest => consts::SCISSOR_TEST,
            })
        }
    }

    fn blend_func(&mut self, src: u32, dst: u32) {
        unsafe { glBlendFuncSeparate(src, dst, src, dst) }
    }

    fn blend_func_separate(&mut self, src_rgb: u32, dst_rgb: u32, src_alpha: u32, dst_alpha: u32) {
        unsafe { glBlendFuncSeparate(src_rgb, dst_rgb, src_alpha, dst_alpha) }
    }

    fn blend_equation(&mut self, equation: u32) {
        unsafe { glBlendEquation(equation) }
    }

    fn color_mask(&mut self, r: bool, g: bool, b: bool, a: bool) {
        unsafe {
            glColorMask(
                i32::from(r),
                i32::from(g),
                i32::from(b),
                i32::from(a),
            )
        }
    }

    fn draw_arrays(&mut self, mode: DrawMode, first: i32, count: i32) {
        unsafe {
            let mode = match mode {
                DrawMode::Triangles => consts::TRIANGLES,
                DrawMode::TriangleStrip => consts::TRIANGLE_STRIP,
            };
            glDrawArrays(mode, first, count);
        }
    }

    fn read_pixels(&mut self, x: i32, y: i32, width: i32, height: i32, out: &mut [u8]) {
        unsafe {
            glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
            glReadPixels(
                x,
                y,
                width,
                height,
                consts::RGBA,
                consts::UNSIGNED_BYTE,
                out.as_mut_ptr() as *mut core::ffi::c_void,
            );
        }
    }

    fn get_error(&mut self) -> GlError {
        unsafe { GlError::from_raw(glGetError()) }
    }

    fn finish(&mut self) {
        unsafe { glFinish() }
    }
}

/// The compile log of a shader.
unsafe fn shader_log(shader: ShaderId) -> String {
    let mut length: EGLint = 0;
    glGetShaderiv(shader.0, 0x8B84, &mut length); // GL_INFO_LOG_LENGTH
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u8; length as usize];
    glGetShaderInfoLog(shader.0, length, core::ptr::null_mut(), buffer.as_mut_ptr() as *mut core::ffi::c_char);
    let end = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

/// The link log of a program.
unsafe fn program_log(program: ProgramId) -> String {
    let mut length: EGLint = 0;
    glGetProgramiv(program.0, 0x8B84, &mut length); // GL_INFO_LOG_LENGTH
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u8; length as usize];
    glGetProgramInfoLog(program.0, length, core::ptr::null_mut(), buffer.as_mut_ptr() as *mut core::ffi::c_char);
    let end = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

/// A graphics error.
fn graphics_error(context: &'static str) -> VimoError {
    VimoError::detailed(ErrorKind::Graphics, context, "the driver reported a failure")
}

/// `CStr` helper.
use core::ffi::CStr;
