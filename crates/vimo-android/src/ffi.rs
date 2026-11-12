//! Native bindings.
//!
//! The Android platform layer talks to EGL, OpenGL ES 3 and `MediaCodec`
//! through these declarations. They are declared here rather than generated
//! from the NDK headers so that the crate **compiles on a desktop**, which
//! means the Rust code is type checked by CI even when the Android build is
//! not running.
//!
//! The signatures follow the NDK headers exactly:
//!
//! * `EGL14/egl.h`
//! * `GLES3/gl3.h`
//! * `media/NdkMediaCodec.h`
//! * `android/native_window.h`

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

use core::ffi::{c_char, c_int, c_void};

/// An `EGLDisplay`.
pub type EGLDisplay = *mut c_void;
/// An `EGLContext`.
pub type EGLContext = *mut c_void;
/// An `EGLSurface`.
pub type EGLSurface = *mut c_void;
/// An `EGLConfig`.
pub type EGLConfig = *mut c_void;
/// An `EGLNativeWindowType`.
pub type EGLNativeWindowType = *mut c_void;
/// An `EGLint`.
pub type EGLint = c_int;
/// An `EGLBoolean`.
pub type EGLBoolean = c_int;
/// An `ANativeWindow`.
pub type ANativeWindow = c_void;

// --- EGL constants ----------------------------------------------------

/// `EGL_DEFAULT_DISPLAY`
pub const EGL_DEFAULT_DISPLAY: EGLint = 0;
/// `EGL_NO_DISPLAY`
pub const EGL_NO_DISPLAY: EGLDisplay = core::ptr::null_mut();
/// `EGL_NO_CONTEXT`
pub const EGL_NO_CONTEXT: EGLContext = core::ptr::null_mut();
/// `EGL_NO_SURFACE`
pub const EGL_NO_SURFACE: EGLSurface = core::ptr::null_mut();
/// `EGL_NONE`
pub const EGL_NONE: EGLint = 0x3038;
/// `EGL_RENDERABLE_TYPE`
pub const EGL_RENDERABLE_TYPE: EGLint = 0x3040;
/// `EGL_OPENGL_ES3_BIT_KHR`
pub const EGL_OPENGL_ES3_BIT: EGLint = 0x0040;
/// `EGL_SURFACE_TYPE`
pub const EGL_SURFACE_TYPE: EGLint = 0x3033;
/// `EGL_WINDOW_BIT`
pub const EGL_WINDOW_BIT: EGLint = 0x0004;
/// `EGL_RED_SIZE`
pub const EGL_RED_SIZE: EGLint = 8;
/// `EGL_GREEN_SIZE`
pub const EGL_GREEN_SIZE: EGLint = 9;
/// `EGL_BLUE_SIZE`
pub const EGL_BLUE_SIZE: EGLint = 10;
/// `EGL_ALPHA_SIZE`
pub const EGL_ALPHA_SIZE: EGLint = 11;
/// `EGL_DEPTH_SIZE`
pub const EGL_DEPTH_SIZE: EGLint = 12;
/// `EGL_STENCIL_SIZE`
pub const EGL_STENCIL_SIZE: EGLint = 13;
/// `EGL_CONTEXT_CLIENT_VERSION`
pub const EGL_CONTEXT_CLIENT_VERSION: EGLint = 0x3098;
/// `EGL_WIDTH`
pub const EGL_WIDTH: EGLint = 0x3057;
/// `EGL_HEIGHT`
pub const EGL_HEIGHT: EGLint = 0x3056;
/// `EGL_VENDOR`
pub const EGL_VENDOR: EGLint = 0x3053;
/// `EGL_VERSION`
pub const EGL_VERSION: EGLint = 0x3054;
/// `EGL_EXTENSIONS`
pub const EGL_EXTENSIONS: EGLint = 0x3055;
/// `EGL_CONTEXT_LOST`
pub const EGL_CONTEXT_LOST: EGLint = 0x300E;

extern "C" {
    /// `eglGetDisplay`
    pub fn eglGetDisplay(display_id: EGLint) -> EGLDisplay;
    /// `eglInitialize`
    pub fn eglInitialize(dpy: EGLDisplay, major: *mut EGLint, minor: *mut EGLint) -> EGLBoolean;
    /// `eglTerminate`
    pub fn eglTerminate(dpy: EGLDisplay) -> EGLBoolean;
    /// `eglChooseConfig`
    pub fn eglChooseConfig(
        dpy: EGLDisplay,
        attributes: *const EGLint,
        configs: *mut EGLConfig,
        config_size: EGLint,
        num_config: *mut EGLint,
    ) -> EGLBoolean;
    /// `eglCreateContext`
    pub fn eglCreateContext(
        dpy: EGLDisplay,
        config: EGLConfig,
        share_context: EGLContext,
        attributes: *const EGLint,
    ) -> EGLContext;
    /// `eglDestroyContext`
    pub fn eglDestroyContext(dpy: EGLDisplay, ctx: EGLContext) -> EGLBoolean;
    /// `eglCreateWindowSurface`
    pub fn eglCreateWindowSurface(
        dpy: EGLDisplay,
        config: EGLConfig,
        win: EGLNativeWindowType,
        attributes: *const EGLint,
    ) -> EGLSurface;
    /// `eglDestroySurface`
    pub fn eglDestroySurface(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean;
    /// `eglMakeCurrent`
    pub fn eglMakeCurrent(
        dpy: EGLDisplay,
        draw: EGLSurface,
        read: EGLSurface,
        ctx: EGLContext,
    ) -> EGLBoolean;
    /// `eglSwapBuffers`
    pub fn eglSwapBuffers(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean;
    /// `eglQueryString`
    pub fn eglQueryString(dpy: EGLDisplay, name: EGLint) -> *const c_char;
    /// `eglGetError`
    pub fn eglGetError() -> EGLint;
    /// `eglGetDisplayConfigCount` is not portable; `eglGetConfigs` is.
    pub fn eglGetConfigs(
        dpy: EGLDisplay,
        configs: *mut EGLConfig,
        config_size: EGLint,
        num_config: *mut EGLint,
    ) -> EGLBoolean;
}

// --- GLES3 constants ---------------------------------------------------

/// `GL_TEXTURE_EXTERNAL_OES`
pub const GL_TEXTURE_EXTERNAL_OES: u32 = 0x8D65;
/// `GL_FRAMEBUFFER`
pub const GL_FRAMEBUFFER: u32 = 0x8D40;
/// `GL_RENDERBUFFER`
pub const GL_RENDERBUFFER: u32 = 0x8D41;
/// `GL_COLOR_ATTACHMENT0`
pub const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
/// `GL_TEXTURE_2D`
pub const GL_TEXTURE_2D: u32 = 0x0DE1;
/// `GL_TEXTURE0`
pub const GL_TEXTURE0: u32 = 0x84C0;
/// `GL_FLOAT`
pub const GL_FLOAT: u32 = 0x1406;
/// `GL_TEXTURE_EXTERNAL_OES` as a target
pub const GL_TEXTURE_EXTERNAL_OES_TARGET: u32 = 0x8D65;
/// `GL_UNPACK_ALIGNMENT`
pub const GL_UNPACK_ALIGNMENT: u32 = 0x0CF5;
/// `GL_TEXTURE_MAX_AGE`
pub const GL_TEXTURE_MAX_AGE: u32 = 0x8D7E;

extern "C" {
    // --- shaders and programs
    /// `glCreateShader`
    pub fn glCreateShader(type_: u32) -> u32;
    /// `glShaderSource`
    pub fn glShaderSource(shader: u32, count: EGLint, string: *const *const c_char, length: *const EGLint);
    /// `glCompileShader`
    pub fn glCompileShader(shader: u32);
    /// `glGetShaderiv`
    pub fn glGetShaderiv(shader: u32, pname: u32, params: *mut EGLint);
    /// `glGetShaderInfoLog`
    pub fn glGetShaderInfoLog(shader: u32, bufSize: EGLint, length: *mut EGLint, infoLog: *mut c_char);
    /// `glDeleteShader`
    pub fn glDeleteShader(shader: u32);
    /// `glCreateProgram`
    pub fn glCreateProgram() -> u32;
    /// `glAttachShader`
    pub fn glAttachShader(program: u32, shader: u32);
    /// `glLinkProgram`
    pub fn glLinkProgram(program: u32);
    /// `glGetProgramiv`
    pub fn glGetProgramiv(program: u32, pname: u32, params: *mut EGLint);
    /// `glGetProgramInfoLog`
    pub fn glGetProgramInfoLog(program: u32, bufSize: EGLint, length: *mut EGLint, infoLog: *mut c_char);
    /// `glUseProgram`
    pub fn glUseProgram(program: u32);
    /// `glDeleteProgram`
    pub fn glDeleteProgram(program: u32);
    /// `glGetUniformLocation`
    pub fn glGetUniformLocation(program: u32, name: *const c_char) -> EGLint;
    /// `glUniform1f`
    pub fn glUniform1f(location: EGLint, v0: f32);
    /// `glUniform2f`
    pub fn glUniform2f(location: EGLint, v0: f32, v1: f32);
    /// `glUniform3f`
    pub fn glUniform3f(location: EGLint, v0: f32, v1: f32, v2: f32);
    /// `glUniform4f`
    pub fn glUniform4f(location: EGLint, v0: f32, v1: f32, v2: f32, v3: f32);
    /// `glUniform1i`
    pub fn glUniform1i(location: EGLint, v0: EGLint);
    /// `glUniformMatrix3fv`
    pub fn glUniformMatrix3fv(
        location: EGLint,
        count: EGLint,
        transpose: EGLBoolean,
        value: *const f32,
    );

    // --- textures
    /// `glGenTextures`
    pub fn glGenTextures(n: EGLint, textures: *mut u32);
    /// `glDeleteTextures`
    pub fn glDeleteTextures(n: EGLint, textures: *const u32);
    /// `glBindTexture`
    pub fn glBindTexture(target: u32, texture: u32);
    /// `glActiveTexture`
    pub fn glActiveTexture(texture: u32);
    /// `glTexImage2D`
    pub fn glTexImage2D(
        target: u32,
        level: EGLint,
        internalformat: EGLint,
        width: EGLint,
        height: EGLint,
        border: EGLint,
        format: u32,
        type_: u32,
        pixels: *const c_void,
    );
    /// `glTexParameteri`
    pub fn glTexParameteri(target: u32, pname: u32, param: EGLint);
    /// `glPixelStorei`
    pub fn glPixelStorei(pname: u32, param: EGLint);

    // --- buffers and vertex arrays
    /// `glGenBuffers`
    pub fn glGenBuffers(n: EGLint, buffers: *mut u32);
    /// `glDeleteBuffers`
    pub fn glDeleteBuffers(n: EGLint, buffers: *const u32);
    /// `glBindBuffer`
    pub fn glBindBuffer(target: u32, buffer: u32);
    /// `glBufferData`
    pub fn glBufferData(target: u32, size: isize, data: *const c_void, usage: u32);
    /// `glGenVertexArrays`
    pub fn glGenVertexArrays(n: EGLint, arrays: *mut u32);
    /// `glDeleteVertexArrays`
    pub fn glDeleteVertexArrays(n: EGLint, arrays: *const u32);
    /// `glBindVertexArray`
    pub fn glBindVertexArray(array: u32);
    /// `glEnableVertexAttribArray`
    pub fn glEnableVertexAttribArray(index: u32);
    /// `glVertexAttribPointer`
    pub fn glVertexAttribPointer(
        index: u32,
        size: EGLint,
        type_: u32,
        normalized: EGLBoolean,
        stride: EGLint,
        pointer: *const c_void,
    );

    // --- framebuffers
    /// `glGenFramebuffers`
    pub fn glGenFramebuffers(n: EGLint, framebuffers: *mut u32);
    /// `glDeleteFramebuffers`
    pub fn glDeleteFramebuffers(n: EGLint, framebuffers: *const u32);
    /// `glBindFramebuffer`
    pub fn glBindFramebuffer(target: u32, framebuffer: u32);
    /// `glFramebufferTexture2D`
    pub fn glFramebufferTexture2D(
        target: u32,
        attachment: u32,
        textarget: u32,
        texture: u32,
        level: EGLint,
    );
    /// `glCheckFramebufferStatus`
    pub fn glCheckFramebufferStatus(target: u32) -> u32;

    // --- state
    /// `glViewport`
    pub fn glViewport(x: EGLint, y: EGLint, width: EGLint, height: EGLint);
    /// `glClearColor`
    pub fn glClearColor(red: f32, green: f32, blue: f32, alpha: f32);
    /// `glClear`
    pub fn glClear(mask: u32);
    /// `glEnable`
    pub fn glEnable(cap: u32);
    /// `glDisable`
    pub fn glDisable(cap: u32);
    /// `glBlendFuncSeparate`
    pub fn glBlendFuncSeparate(src_rgb: u32, dst_rgb: u32, src_alpha: u32, dst_alpha: u32);
    /// `glBlendEquation`
    pub fn glBlendEquation(mode: u32);
    /// `glColorMask`
    pub fn glColorMask(red: EGLBoolean, green: EGLBoolean, blue: EGLBoolean, alpha: EGLBoolean);
    /// `glDrawArrays`
    pub fn glDrawArrays(mode: u32, first: EGLint, count: EGLint);
    /// `glReadPixels`
    pub fn glReadPixels(
        x: EGLint,
        y: EGLint,
        width: EGLint,
        height: EGLint,
        format: u32,
        type_: u32,
        pixels: *mut c_void,
    );
    /// `glGetError`
    pub fn glGetError() -> u32;
    /// `glFinish`
    pub fn glFinish();
    /// `glFlush`
    pub fn glFlush();
}

// --- MediaCodec --------------------------------------------------------

/// An `AMediaCodec`.
pub type AMediaCodec = c_void;
/// An `AMediaFormat`.
pub type AMediaFormat = c_void;
/// An `AMediaBuffer`.
pub type AMediaBuffer = c_void;

/// `AMEDIA_OK`
pub const AMEDIA_OK: c_int = 0;
/// `AMEDIA_ERROR_UNKNOWN`
pub const AMEDIA_ERROR_UNKNOWN: c_int = -1;
/// `AMEDIA_ERROR_MALFORMED`
pub const AMEDIA_ERROR_MALFORMED: c_int = -2;
/// `AMEDIA_ERROR_UNSUPPORTED`
pub const AMEDIA_ERROR_UNSUPPORTED: c_int = -3;
/// `AMEDIA_ERROR_INVALID_PARAMETER`
pub const AMEDIA_ERROR_INVALID_PARAMETER: c_int = -4;
/// `AMEDIA_BUFFER_FLAG_CODEC_CONFIG`
pub const AMEDIA_BUFFER_FLAG_CODEC_CONFIG: u32 = 2;
/// `AMEDIA_BUFFER_FLAG_END_OF_STREAM`
pub const AMEDIA_BUFFER_FLAG_END_OF_STREAM: u32 = 4;
/// `AMEDIA_MIMETYPE_VIDEO_AVC`
pub const AMEDIA_MIMETYPE_VIDEO_AVC: &str = "video/avc";
/// `AMEDIA_MIMETYPE_VIDEO_HEVC`
pub const AMEDIA_MIMETYPE_VIDEO_HEVC: &str = "video/hevc";
/// `AMEDIA_MIMETYPE_VIDEO_VP9`
pub const AMEDIA_MIMETYPE_VIDEO_VP9: &str = "video/x-vnd.on2.vp9";
/// `AMEDIA_MIMETYPE_VIDEO_AV1`
pub const AMEDIA_MIMETYPE_VIDEO_AV1: &str = "video/av01";
/// `AMEDIA_MIMETYPE_AUDIO_AAC`
pub const AMEDIA_MIMETYPE_AUDIO_AAC: &str = "audio/mp4a-latm";

extern "C" {
    /// `AMediaCodec_createCodecByName`
    pub fn AMediaCodec_createCodecByName(name: *const c_char) -> *mut AMediaCodec;
    /// `AMediaCodec_delete`
    pub fn AMediaCodec_delete(codec: *mut AMediaCodec);
    /// `AMediaCodec_configure`
    pub fn AMediaCodec_configure(
        codec: *mut AMediaCodec,
        format: *mut AMediaFormat,
        window: *mut ANativeWindow,
        flags: c_int,
        mime: *mut c_int,
    ) -> c_int;
    /// `AMediaCodec_start`
    pub fn AMediaCodec_start(codec: *mut AMediaCodec) -> c_int;
    /// `AMediaCodec_stop`
    pub fn AMediaCodec_stop(codec: *mut AMediaCodec);
    /// `AMediaCodec_flush`
    pub fn AMediaCodec_flush(codec: *mut AMediaCodec);
    /// `AMediaCodec_dequeueOutputBuffer`
    pub fn AMediaCodec_dequeueOutputBuffer(
        codec: *mut AMediaCodec,
        buffer: *mut *mut AMediaBuffer,
        timeout_us: i64,
    ) -> isize;
    /// `AMediaCodec_releaseOutputBuffer`
    pub fn AMediaCodec_releaseOutputBuffer(codec: *mut AMediaCodec, index: usize, flags: u32);
    /// `AMediaCodec_releaseOutputBufferAtTime`
    pub fn AMediaCodec_releaseOutputBufferAtTime(
        codec: *mut AMediaCodec,
        index: usize,
        timestamp_us: i64,
        flags: u32,
    );
    /// `AMediaCodec_getOutputFormat`
    pub fn AMediaCodec_getOutputFormat(codec: *mut AMediaCodec, format: *mut AMediaFormat) -> c_int;
    /// `AMediaCodec_getInputFormat`
    pub fn AMediaCodec_getInputFormat(codec: *mut AMediaCodec, format: *mut AMediaFormat) -> c_int;

    /// `AMediaFormat_new`
    pub fn AMediaFormat_new() -> *mut AMediaFormat;
    /// `AMediaFormat_delete`
    pub fn AMediaFormat_delete(format: *mut AMediaFormat);
    /// `AMediaFormat_setString`
    pub fn AMediaFormat_setString(format: *mut AMediaFormat, name: *const c_char, value: *const c_char);
    /// `AMediaFormat_setInt32`
    pub fn AMediaFormat_setInt32(format: *mut AMediaFormat, name: *const c_char, value: c_int);
    /// `AMediaFormat_getInt32`
    pub fn AMediaFormat_getInt32(format: *mut AMediaFormat, name: *const c_char, out: *mut c_int) -> c_int;
    /// `AMediaFormat_setBuffer`
    pub fn AMediaFormat_setBuffer(format: *mut AMediaFormat, buffer: *const c_void, size: usize);
    /// `AMediaFormat_getBufferSize`
    pub fn AMediaFormat_getBufferSize(format: *mut AMediaFormat) -> usize;
    /// `AMediaFormat_getBuffer`
    pub fn AMediaFormat_getBuffer(format: *mut AMediaFormat) -> *mut c_void;
}

// --- ANativeWindow -----------------------------------------------------

extern "C" {
    /// `ANativeWindow_acquire`
    pub fn ANativeWindow_acquire(window: *mut ANativeWindow) -> c_int;
    /// `ANativeWindow_release`
    pub fn ANativeWindow_release(window: *mut ANativeWindow);
    /// `ANativeWindow_getWidth`
    pub fn ANativeWindow_getWidth(window: *mut ANativeWindow) -> c_int;
    /// `ANativeWindow_getHeight`
    pub fn ANativeWindow_getHeight(window: *mut ANativeWindow) -> c_int;
    /// `ANativeWindow_setBuffersGeometry`
    pub fn ANativeWindow_setBuffersGeometry(
        window: *mut ANativeWindow,
        width: c_int,
        height: c_int,
        format: c_int,
    ) -> c_int;
}

/// `AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM`
pub const AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM: c_int = 1;
