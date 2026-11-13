//! The Android render context.
//!
//! This is what the core's render thread draws with: it owns the EGL
//! context, the GL renderer and the shaders, and it is the only thing that
//! touches the GPU.
//!
//! Everything in here runs on the render thread. The GL context belongs to
//! one thread, so nothing crosses a thread boundary.

use vimo_engine::render_thread::RenderContext;
use vimo_foundation::error::{invalid_state, VimoResult};
use vimo_gl::{GlRenderer, ShaderLibrary};
use vimo_render::FramePlan;

use crate::egl::EglRenderer;
use crate::ffi::ANativeWindow;

/// The output surface, as a raw `ANativeWindow`.
pub type NativeWindow = *mut std::ffi::c_void;

/// The renderer of the preview.
pub struct AndroidRenderContext {
    /// The window the frames are drawn into.
    window: usize,
    /// The width of the output, in pixels.
    width: u32,
    /// The height of the output, in pixels.
    height: u32,
    /// The EGL context.
    egl: Option<EglRenderer>,
    /// The renderer.
    renderer: Option<GlRenderer>,
    /// The effects and their shaders.
    shaders: ShaderLibrary,
}

impl AndroidRenderContext {
    /// A renderer for the given window.
    ///
    /// The window is kept as an integer so that the type stays `Send`: it is
    /// only ever used on the render thread, which owns it.
    #[must_use]
    pub fn new(window: NativeWindow, width: u32, height: u32, shaders: ShaderLibrary) -> Self {
        AndroidRenderContext {
            window: window as usize,
            width: width.max(1),
            height: height.max(1),
            egl: None,
            renderer: None,
            shaders,
        }
    }
}

impl RenderContext for AndroidRenderContext {
    fn on_start(&mut self) -> VimoResult<()> {
        let egl = EglRenderer::new(self.window as *mut ANativeWindow, self.width, self.height)?;
        let api = egl.gl_api();
        let renderer = GlRenderer::new(
            Box::new(api),
            self.shaders.registry().clone(),
            self.width,
            self.height,
        )?;
        self.egl = Some(egl);
        self.renderer = Some(renderer);
        Ok(())
    }

    fn draw(&mut self, plan: &FramePlan) -> VimoResult<()> {
        let Some(renderer) = self.renderer.as_mut() else {
            return Err(invalid_state("render: the context has not started"));
        };
        renderer.draw(plan, &mut self.shaders)?;
        if let Some(egl) = self.egl.as_mut() {
            egl.swap()?;
        }
        Ok(())
    }

    fn on_stop(&mut self) {
        self.renderer = None;
        self.egl = None;
    }
}
