//! The dedicated render thread.
//!
//! On Android, the GL context belongs to exactly one thread. The core
//! therefore runs the renderer on its **own** thread and communicates with it
//! through a channel. The UI thread never touches GL.
//!
//! ```text
//!   UI thread                    render thread
//!   ──────────                   ─────────────
//!   render_at(t) ──channel──►   build the frame plan
//!                 ◄──channel──   draw it into the surface
//! ```
//!
//! The context is owned by the thread: it is created on the render thread
//! (via [`RenderContext::on_start`]) and destroyed there, so no GL object ever
//! crosses a thread boundary.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread::JoinHandle;

use vimo_foundation::error::{cancelled, VimoResult};
use vimo_foundation::time::Time;
use vimo_render::FramePlan;

/// A command sent to the render thread.
pub enum RenderCommand {
    /// Build and draw the frame at `time`.
    Render {
        /// The timeline position to render.
        time: Time,
        /// The plan to fill in (the caller owns it).
        plan: std::sync::Arc<std::sync::Mutex<FramePlan>>,
        /// The result channel.
        response: SyncSender<VimoResult<()>>,
    },
    /// The output surface changed size.
    Resize {
        /// The new width.
        width: u32,
        /// The new height.
        height: u32,
    },
    /// Stop the thread.
    Stop,
}

/// The platform's renderer, owned by the render thread.
///
/// Implementations create and use the GL context; they are only ever called
/// from the render thread.
pub trait RenderContext: Send {
    /// Called on the render thread before the first frame.
    fn on_start(&mut self) -> VimoResult<()>;

    /// Draw a frame plan.
    fn draw(&mut self, plan: &FramePlan) -> VimoResult<()>;

    /// Called on the render thread when the thread stops.
    fn on_stop(&mut self);
}

/// A handle to the render thread.
pub struct RenderThread {
    sender: std::sync::mpsc::Sender<RenderCommand>,
    handle: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for RenderThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderThread")
            .field("running", &self.handle.is_some())
            .finish()
    }
}

impl RenderThread {
    /// Start a render thread that owns `context`.
    #[must_use]
    pub fn spawn(context: Box<dyn RenderContext>) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel::<RenderCommand>();
        let handle = std::thread::Builder::new()
            .name("vimo-render".to_string())
            .spawn(move || run(context, receiver))
            .ok();
        RenderThread { sender, handle }
    }

    /// Render a frame and wait for it to be drawn.
    ///
    /// The plan is shared with the render thread, which fills it in and then
    /// draws it.
    pub fn render(
        &self,
        time: Time,
        plan: std::sync::Arc<std::sync::Mutex<FramePlan>>,
    ) -> VimoResult<()> {
        let (response_tx, response_rx) = sync_channel(1);
        self.sender
            .send(RenderCommand::Render {
                time,
                plan,
                response: response_tx,
            })
            .map_err(|_| cancelled("render thread: the thread has stopped"))?;
        response_rx
            .recv()
            .map_err(|_| cancelled("render thread: the thread has stopped"))?
    }

    /// Tell the thread that the output size changed.
    pub fn resize(&self, width: u32, height: u32) -> VimoResult<()> {
        self.sender
            .send(RenderCommand::Resize { width, height })
            .map_err(|_| cancelled("render thread: the thread has stopped"))
    }

    /// Stop the thread and wait for it to finish.
    pub fn stop(&mut self) {
        let _ = self.sender.send(RenderCommand::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for RenderThread {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The render thread's main loop.
fn run(mut context: Box<dyn RenderContext>, receiver: Receiver<RenderCommand>) {
    if context.on_start().is_err() {
        return;
    }
    while let Ok(command) = receiver.recv() {
        match command {
            RenderCommand::Render {
                time,
                plan,
                response,
            } => {
                // The plan is filled in by the caller; the render thread only
                // draws it. Building the plan is the engine's job.
                let result = match plan.lock() {
                    Ok(plan) => context.draw(&plan),
                    Err(_) => Err(cancelled("render thread: the plan was poisoned")),
                };
                let _ = time;
                let _ = response.send(result);
            }
            RenderCommand::Resize { width, height } => {
                let _ = (width, height);
            }
            RenderCommand::Stop => break,
        }
    }
    context.on_stop();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct CountingContext {
        drawn: Arc<AtomicUsize>,
        started: Arc<AtomicUsize>,
    }

    impl RenderContext for CountingContext {
        fn on_start(&mut self) -> VimoResult<()> {
            self.started.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn draw(&mut self, _plan: &FramePlan) -> VimoResult<()> {
            self.drawn.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn on_stop(&mut self) {}
    }

    #[test]
    fn the_thread_starts_and_draws() {
        let drawn = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let mut thread = RenderThread::spawn(Box::new(CountingContext {
            drawn: drawn.clone(),
            started: started.clone(),
        }));
        let plan = std::sync::Arc::new(std::sync::Mutex::new(FramePlan::new()));
        thread
            .render(vimo_foundation::time::Time::ZERO, plan)
            .unwrap();
        assert_eq!(drawn.load(Ordering::SeqCst), 1);
        assert_eq!(started.load(Ordering::SeqCst), 1);
        thread.stop();
    }

    #[test]
    fn rendering_after_stop_fails_gracefully() {
        let drawn = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let mut thread = RenderThread::spawn(Box::new(CountingContext {
            drawn: drawn.clone(),
            started: started.clone(),
        }));
        thread.stop();
        let plan = std::sync::Arc::new(std::sync::Mutex::new(FramePlan::new()));
        assert!(thread
            .render(vimo_foundation::time::Time::ZERO, plan)
            .is_err());
    }
}
