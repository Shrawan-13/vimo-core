//! The C ABI.
//!
//! # Contract
//!
//! * The engine is an **opaque handle**. The caller never sees the Rust types.
//! * **Commands in, events out.** The UI sends commands and receives events;
//!   it reads the project through [`vimo_engine_snapshot`].
//! * **Every entry point is panic proof.** A Rust panic that reaches the
//!   boundary is caught and turned into an error code instead of unwinding
//!   into the JVM.
//! * **No per-frame calls across the FFI.** Frames are rendered on the
//!   engine's own thread into an `ANativeWindow` that was handed over once.
//!
//! # Threading
//!
//! The handle is `Send + Sync`. Commands may be issued from any thread; the
//! engine serialises them internally.

#![deny(missing_docs)]

use std::ffi::{c_char, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use vimo_device::DeviceProfile;
use vimo_engine::clock::MonotonicClock;
use vimo_engine::{Engine, EngineEvent, ExportPreset};
use vimo_foundation::error::ErrorKind;
use vimo_foundation::math::Vec2;
use vimo_foundation::time::{Time, TimeRange};
use vimo_manifest::EffectRegistry;
use vimo_project::anim::{Interpolation, Keyframe};
use vimo_project::model::{
    Asset, BlendMode, Clip, EffectNode, EffectOwner, Track, TrackKind, Transform, Transition,
};
use vimo_project::Project;
use vimo_render::uniforms::uniform_name;

/// The version of the ABI. The Kotlin side checks it at load time.
pub const VIMO_ABI_VERSION: u32 = 2;

/// Status codes returned by every entry point.
pub mod status {
    /// The call succeeded.
    pub const OK: i32 = 0;
    /// The handle was null or already destroyed.
    pub const INVALID_HANDLE: i32 = -1;
    /// The command or event was malformed.
    pub const INVALID_ARGUMENT: i32 = -2;
    /// The engine reported an error.
    pub const ERROR: i32 = -3;
    /// A panic was caught at the boundary.
    pub const PANIC: i32 = -4;
    /// The feature is not implemented on this platform.
    pub const UNSUPPORTED: i32 = -5;
}

/// Command tags. **Never renumber an existing tag.**
pub mod command_tag {
    /// Start playback.
    pub const PLAY: u32 = 1;
    /// Pause playback.
    pub const PAUSE: u32 = 2;
    /// Stop playback and rewind.
    pub const STOP: u32 = 3;
    /// Seek to a position (nanoseconds in `a`).
    pub const SEEK: u32 = 4;
    /// Set the playback rate (`a` is the rate times 1000).
    pub const SET_RATE: u32 = 5;
    /// Undo the last edit.
    pub const UNDO: u32 = 6;
    /// Redo the last undone edit.
    pub const REDO: u32 = 7;
    /// Add a clip (`a` = track, `b` = asset, `c` = start in nanoseconds).
    pub const ADD_CLIP: u32 = 8;
    /// Remove a clip (`a` = track, `b` = clip index).
    pub const REMOVE_CLIP: u32 = 9;
    /// Set an effect parameter (`a` = effect, `b` = param, `c` = float bits).
    pub const SET_PARAM: u32 = 10;
    /// Load a project from a path (`text`).
    pub const LOAD_PROJECT: u32 = 11;
    /// Save the project to a path (`text`).
    pub const SAVE_PROJECT: u32 = 12;
    /// Start an export with the preset in `a`.
    pub const START_EXPORT: u32 = 13;
    /// Cancel the running export.
    pub const CANCEL_EXPORT: u32 = 14;
    /// Hand over an `ANativeWindow` (in `a` as a pointer).
    pub const SET_SURFACE: u32 = 15;
    /// Report a thermal reading (`a` = state).
    pub const THERMAL: u32 = 16;
    /// Report memory usage.
    pub const MEMORY: u32 = 17;
    /// Set the preview scale (`a` = scale times 1000).
    pub const SET_PREVIEW_SCALE: u32 = 18;
    /// Move a clip (`a` = clip, `b` = track, `c` = new start in nanoseconds).
    pub const MOVE_CLIP: u32 = 19;
    /// Trim a clip (`a` = clip, `b` = new start ns, `c` = new end ns).
    pub const TRIM_CLIP: u32 = 20;
    /// Split a clip at a time (`a` = clip, `b` = time in nanoseconds).
    pub const SPLIT_CLIP: u32 = 21;
    /// Enable or disable a clip (`a` = clip, `b` = 0/1).
    pub const SET_CLIP_ENABLED: u32 = 22;
    /// Set a clip's opacity (`a` = clip, `b` = float bits).
    pub const SET_CLIP_OPACITY: u32 = 23;
    /// Set a clip's blend mode (`a` = clip, `b` = blend mode).
    pub const SET_CLIP_BLEND: u32 = 24;
    /// Set a clip's transform (`a` = clip, `floats` = 8 values).
    pub const SET_CLIP_TRANSFORM: u32 = 25;
    /// Set a clip's volume (`a` = clip, `b` = float bits).
    pub const SET_CLIP_VOLUME: u32 = 26;
    /// Set a clip's speed (`a` = clip, `b` = float bits).
    pub const SET_CLIP_SPEED: u32 = 27;
    /// Mute or unmute a clip (`a` = clip, `b` = 0/1).
    pub const SET_CLIP_MUTED: u32 = 28;
    /// Add a track (`a` = kind, `b` = 0/1 audio).
    pub const ADD_TRACK: u32 = 29;
    /// Remove a track (`a` = track).
    pub const REMOVE_TRACK: u32 = 30;
    /// Enable or disable a track (`a` = track, `b` = 0/1).
    pub const SET_TRACK_ENABLED: u32 = 31;
    /// Mute or unmute a track (`a` = track, `b` = 0/1).
    pub const SET_TRACK_MUTED: u32 = 32;
    /// Solo or unsolo a track (`a` = track, `b` = 0/1).
    pub const SET_TRACK_SOLO: u32 = 33;
    /// Lock or unlock a track (`a` = track, `b` = 0/1).
    pub const SET_TRACK_LOCKED: u32 = 34;
    /// Set a track's volume (`a` = track, `b` = float bits).
    pub const SET_TRACK_VOLUME: u32 = 35;
    /// Add an effect (`a` = owner kind, `b` = owner, `c` = effect type).
    pub const ADD_EFFECT: u32 = 36;
    /// Remove an effect (`a` = owner kind, `b` = owner, `c` = effect).
    pub const REMOVE_EFFECT: u32 = 37;
    /// Enable or disable an effect (`a` = effect, `b` = 0/1).
    pub const SET_EFFECT_ENABLED: u32 = 38;
    /// Add a keyframe (`a` = effect, `b` = track, `c` = time ns, `floats[0]` = value).
    pub const ADD_KEYFRAME: u32 = 39;
    /// Remove a keyframe (`a` = effect, `b` = track, `c` = index).
    pub const REMOVE_KEYFRAME: u32 = 40;
    /// Set a keyframe's interpolation (`a` = effect, `b` = track, `c` = index).
    pub const SET_KEYFRAME_INTERPOLATION: u32 = 41;
    /// Add an asset (`text` = source id, `floats[0..2]` = duration and width).
    pub const ADD_ASSET: u32 = 42;
    /// Remove an asset (`a` = asset).
    pub const REMOVE_ASSET: u32 = 43;
    /// Add a transition (`a` = track, `b` = from clip, `c` = to clip).
    pub const ADD_TRANSITION: u32 = 44;
    /// Remove a transition (`a` = transition).
    pub const REMOVE_TRANSITION: u32 = 45;
    /// Set the project name (`text`).
    pub const SET_PROJECT_NAME: u32 = 46;
    /// Set the resolution (`a` = width, `b` = height).
    pub const SET_RESOLUTION: u32 = 47;
    /// Set the frame rate (`a` = numerator, `b` = denominator).
    pub const SET_FPS: u32 = 48;
    /// Set the autosave directory (`text`).
    pub const SET_AUTOSAVE_DIR: u32 = 49;
    /// Recover the newest autosave.
    pub const RECOVER_AUTOSAVE: u32 = 50;
    /// Set the loop range (`a` = start ns, `b` = end ns, `c` = 1 to enable).
    pub const SET_LOOP: u32 = 51;
    /// Pause the export.
    pub const EXPORT_PAUSE: u32 = 52;
    /// Resume the export.
    pub const EXPORT_RESUME: u32 = 53;
    /// Ask the engine to emit a snapshot event.
    pub const REQUEST_SNAPSHOT: u32 = 54;
    /// Start a new, empty project.
    pub const NEW_PROJECT: u32 = 55;
}

/// Event tags. **Never renumber an existing tag.**
pub mod event_tag {
    /// The project changed.
    pub const PROJECT_CHANGED: u32 = 1;
    /// The playback state changed.
    pub const PLAYBACK_STATE: u32 = 2;
    /// The playhead moved.
    pub const POSITION: u32 = 3;
    /// A frame was rendered.
    pub const FRAME_RENDERED: u32 = 4;
    /// The memory pressure level changed.
    pub const MEMORY_PRESSURE: u32 = 5;
    /// The thermal state changed.
    pub const THERMAL: u32 = 6;
    /// The export progressed.
    pub const EXPORT_PROGRESS: u32 = 7;
    /// The export finished.
    pub const EXPORT_FINISHED: u32 = 8;
    /// An error occurred (`code` and `message`).
    pub const ERROR: u32 = 9;
    /// A project snapshot is available in the message.
    pub const SNAPSHOT: u32 = 10;
}

/// Engine configuration.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VimoConfig {
    /// The width of the output surface in pixels.
    pub width: u32,
    /// The height of the output surface in pixels.
    pub height: u32,
    /// The audio sample rate.
    pub sample_rate: u32,
    /// The audio channel count.
    pub channels: u16,
    /// Reserved, must be zero.
    pub flags: u32,
}

impl Default for VimoConfig {
    fn default() -> Self {
        VimoConfig {
            width: 1920,
            height: 1080,
            sample_rate: 48_000,
            channels: 2,
            flags: 0,
        }
    }
}

/// A command sent from the UI to the engine.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VimoCommand {
    /// One of [`command_tag`].
    pub tag: u32,
    /// First integer argument.
    pub a: i64,
    /// Second integer argument.
    pub b: i64,
    /// Third integer argument.
    pub c: i64,
    /// Optional UTF-8 text (not NUL terminated, `text_len` bytes).
    pub text: *const u8,
    /// The length of `text` in bytes.
    pub text_len: u32,
    /// Optional parameter array.
    pub floats: *const f32,
    /// The number of floats.
    pub float_count: u32,
}

impl Default for VimoCommand {
    fn default() -> Self {
        VimoCommand {
            tag: 0,
            a: 0,
            b: 0,
            c: 0,
            text: std::ptr::null(),
            text_len: 0,
            floats: std::ptr::null(),
            float_count: 0,
        }
    }
}

/// An event sent from the engine to the UI.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VimoEvent {
    /// One of [`event_tag`].
    pub tag: u32,
    /// First integer payload.
    pub a: i64,
    /// Second integer payload.
    pub b: i64,
    /// An error code, when the event is an error.
    pub code: i32,
    /// An optional UTF-8 message. Valid until the next `vimo_engine_poll_events`
    /// call on the same thread.
    pub message: *const u8,
    /// The length of `message` in bytes.
    pub message_len: u32,
}

impl Default for VimoEvent {
    fn default() -> Self {
        VimoEvent {
            tag: 0,
            a: 0,
            b: 0,
            code: 0,
            message: std::ptr::null(),
            message_len: 0,
        }
    }
}

/// Statistics reported to the telemetry layer.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VimoStats {
    /// Frames rendered.
    pub frames_rendered: u64,
    /// Frames dropped because the renderer fell behind.
    pub frames_dropped: u64,
    /// Frames held because the renderer was ahead of the clock.
    pub frames_held: u64,
    /// Cache hits.
    pub cache_hits: u64,
    /// Cache misses.
    pub cache_misses: u64,
    /// Bytes held by the frame cache.
    pub cache_bytes: u64,
    /// The number of live decoder instances.
    pub decoders_live: u32,
}

/// The opaque engine handle.
pub struct VimoEngine {
    inner: Mutex<Engine>,
    /// The `ANativeWindow` handed over by the platform.
    surface: Mutex<usize>,
    /// The last error message, kept alive for the FFI to point at.
    last_error: Mutex<Option<CString>>,
}

impl VimoEngine {
    /// Attach the renderer that draws the frames.
    ///
    /// The context is owned by the render thread, which is created here. The
    /// platform layer calls this once, when the output surface is ready.
    pub fn attach_renderer(
        &self,
        context: Box<dyn vimo_engine::render_thread::RenderContext>,
    ) -> vimo_foundation::error::VimoResult<()> {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.attach_render_thread(vimo_engine::render_thread::RenderThread::spawn(context));
        Ok(())
    }

    /// Draw the next frame.
    ///
    /// The plan is built on the calling thread and drawn on the render
    /// thread, so no GL call ever crosses the boundary.
    pub fn render_once(&self) -> vimo_foundation::error::VimoResult<()> {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.render_to_surface()
    }

    /// The effects and their shaders.
    ///
    /// The renderer needs its own copy, because it runs on the render thread.
    pub fn shaders(&self) -> vimo_foundation::error::VimoResult<vimo_gl::ShaderLibrary> {
        let guard = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        Ok(guard.shaders().clone())
    }

    /// Remember the output surface.
    pub fn set_surface_handle(&self, window: usize) {
        let mut guard = match self.surface.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard = window;
    }

    /// The output surface.
    #[must_use]
    pub fn surface_handle(&self) -> usize {
        let guard = match self.surface.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard
    }
}

thread_local! {
    /// The message buffers handed to the caller. They stay valid until the
    /// next poll on the same thread.
    static MESSAGES: std::cell::RefCell<Vec<CString>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Run `f`, mapping a panic to [`status::PANIC`] and a handled error to its
/// own status code.
fn guard_code<F>(f: F) -> i32
where
    F: FnOnce() -> Result<(), i32>,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => status::OK,
        Ok(Err(code)) => code,
        Err(_) => status::PANIC,
    }
}

/// Like [`guard_code`], but the closure produces the value on success.
///
/// The value must not collide with a negative status code.
fn guard_value<F>(f: F) -> i32
where
    F: FnOnce() -> Result<i32, i32>,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(value)) => value,
        Ok(Err(code)) => code,
        Err(_) => status::PANIC,
    }
}

/// Create an engine.
///
/// The handle must be released with [`vimo_engine_destroy`].
///
/// # Safety
/// `config` must point to a valid [`VimoConfig`].
#[no_mangle]
pub unsafe extern "C" fn vimo_engine_create(config: *const VimoConfig) -> *mut VimoEngine {
    match catch_unwind(AssertUnwindSafe(|| -> Result<*mut VimoEngine, i32> {
        if config.is_null() {
            return Err(status::INVALID_ARGUMENT);
        }
        // SAFETY: the caller guarantees the pointer is valid for the call.
        let cfg = unsafe { *config };
        let profile = DeviceProfile::default();
        let mut engine = Engine::open(
            Project::new(),
            EffectRegistry::new(),
            profile,
            std::sync::Arc::new(MonotonicClock::new()),
        );
        engine.set_resolution(cfg.width, cfg.height);
        Ok(Box::into_raw(Box::new(VimoEngine {
            inner: Mutex::new(engine),
            surface: Mutex::new(0),
            last_error: Mutex::new(None),
        })))
    })) {
        Ok(Ok(handle)) => handle,
        _ => std::ptr::null_mut(),
    }
}

/// Destroy an engine.
///
/// # Safety
/// `engine` must be a handle returned by [`vimo_engine_create`] that has not
/// already been destroyed.
#[no_mangle]
pub unsafe extern "C" fn vimo_engine_destroy(engine: *mut VimoEngine) -> i32 {
    guard_code(|| {
        if engine.is_null() {
            return Err(status::INVALID_HANDLE);
        }
        // SAFETY: the caller guarantees the handle is valid and not used again.
        drop(unsafe { Box::from_raw(engine) });
        Ok(())
    })
}

/// Execute a command.
///
/// # Safety
/// `engine` must be a valid handle and `command` must point to a valid
/// [`VimoCommand`].
#[no_mangle]
pub unsafe extern "C" fn vimo_engine_execute(
    engine: *mut VimoEngine,
    command: *const VimoCommand,
) -> i32 {
    guard_code(|| {
        if engine.is_null() || command.is_null() {
            return Err(status::INVALID_HANDLE);
        }
        // SAFETY: the caller guarantees both pointers are valid.
        let handle = unsafe { &*engine };
        let cmd = unsafe { *command };
        let mut guard = match handle.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let engine = &mut *guard;
        match apply_command(engine, &cmd, handle) {
            Ok(()) => {
                let mut last = match handle.last_error.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                *last = None;
                Ok(())
            }
            Err(code) => {
                let mut last = match handle.last_error.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                *last = CString::new(format!("command {} failed with status {code}", cmd.tag)).ok();
                Err(code)
            }
        }
    })
}

/// Poll for events.
///
/// Writes up to `max_events` events into `out` and returns the number written,
/// or a negative status code.
///
/// # Safety
/// `engine` must be a valid handle and `out` must point to at least
/// `max_events` writable [`VimoEvent`]s.
#[no_mangle]
pub unsafe extern "C" fn vimo_engine_poll_events(
    engine: *mut VimoEngine,
    out: *mut VimoEvent,
    max_events: i32,
) -> i32 {
    guard_value(|| {
        if engine.is_null() || out.is_null() || max_events < 0 {
            return Err(status::INVALID_HANDLE);
        }
        // SAFETY: the caller guarantees both pointers are valid.
        let handle = unsafe { &*engine };
        let mut guard = match handle.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let events = guard.poll_events();
        let count = events.len().min(max_events as usize);
        let converted: Vec<(u32, i64, i64, i32, CString)> =
            events.iter().take(count).map(convert_event).collect();
        // The messages must outlive this call, so they are kept in a
        // thread-local buffer that is replaced (and therefore invalidated) on
        // the next poll.
        MESSAGES.with(|m| {
            let mut m = m.borrow_mut();
            *m = converted.iter().map(|(_, _, _, _, s)| s.clone()).collect();
        });
        MESSAGES.with(|m| {
            let m = m.borrow();
            for (i, (tag, a, b, code, _)) in converted.iter().enumerate() {
                let message = &m[i];
                // SAFETY: `out` has room for `max_events` entries and the
                // messages live in the thread-local buffer above.
                unsafe {
                    *out.add(i) = VimoEvent {
                        tag: *tag,
                        a: *a,
                        b: *b,
                        code: *code,
                        message: message.as_ptr() as *const u8,
                        message_len: message.as_bytes().len() as u32,
                    };
                }
            }
        });
        Ok(count as i32)
    })
}

/// Read the engine statistics.
///
/// # Safety
/// `engine` must be a valid handle and `out` must point to a writable
/// [`VimoStats`].
#[no_mangle]
pub unsafe extern "C" fn vimo_engine_stats(engine: *mut VimoEngine, out: *mut VimoStats) -> i32 {
    guard_code(|| {
        if engine.is_null() || out.is_null() {
            return Err(status::INVALID_HANDLE);
        }
        // SAFETY: the caller guarantees both pointers are valid.
        let handle = unsafe { &*engine };
        let guard = match handle.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let stats = guard.stats();
        let cache = guard.cache().stats();
        // SAFETY: `out` is writable.
        unsafe {
            *out = VimoStats {
                frames_rendered: stats.frames_rendered,
                frames_dropped: stats.frames_dropped,
                frames_held: stats.frames_held,
                cache_hits: cache.hits,
                cache_misses: cache.misses,
                cache_bytes: guard.cache().bytes() as u64,
                decoders_live: 0,
            };
        }
        Ok(())
    })
}

/// Write a JSON snapshot of the project into `out`.
///
/// Returns the number of bytes written. When the buffer is too small the
/// required size is returned instead, so the caller can allocate and retry.
///
/// # Safety
/// `engine` must be a valid handle and `out` must point to `capacity` writable
/// bytes.
#[no_mangle]
pub unsafe extern "C" fn vimo_engine_snapshot(
    engine: *mut VimoEngine,
    out: *mut u8,
    capacity: i32,
) -> i32 {
    guard_value(|| {
        if engine.is_null() || capacity < 0 {
            return Err(status::INVALID_HANDLE);
        }
        // SAFETY: the caller guarantees the handle is valid.
        let handle = unsafe { &*engine };
        let guard = match handle.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let json = guard.snapshot_json();
        let bytes = json.as_bytes();
        if bytes.len() > capacity as usize {
            // Tell the caller how much room it needs.
            return Ok(bytes.len() as i32);
        }
        if !out.is_null() {
            // SAFETY: the caller guarantees `out` has `capacity` bytes.
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
            }
        }
        Ok(bytes.len() as i32)
    })
}

/// Load the effect manifest and the shader sources into the engine.
///
/// The platform reads the text from its own asset store and passes it here;
/// the core registers the effects, pairs them with their shaders and checks
/// that the two agree.
///
/// Returns the number of interface mismatches (zero is good) or a negative
/// status code.
///
/// # Safety
/// `engine` must be a valid handle and both strings must be valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn vimo_engine_load_effects(
    engine: *mut VimoEngine,
    manifest: *const u8,
    manifest_len: usize,
    shaders: *const u8,
    shaders_len: usize,
) -> i32 {
    guard_value(|| {
        if engine.is_null() {
            return Err(status::INVALID_HANDLE);
        }
        let manifest_text = match std::str::from_utf8(unsafe {
            std::slice::from_raw_parts(manifest, manifest_len)
        }) {
            Ok(text) => text,
            Err(_) => return Err(status::INVALID_ARGUMENT),
        };
        let shaders_text = match std::str::from_utf8(unsafe {
            std::slice::from_raw_parts(shaders, shaders_len)
        }) {
            Ok(text) => text,
            Err(_) => return Err(status::INVALID_ARGUMENT),
        };

        // SAFETY: the caller guarantees the handle is valid.
        let handle = unsafe { &*engine };
        let mut guard = match handle.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let engine = &mut *guard;
        let sources: std::collections::HashMap<String, String> =
            match serde_json::from_str(shaders_text) {
                Ok(sources) => sources,
                Err(_) => return Err(status::INVALID_ARGUMENT),
            };
        // The interface is checked now, so a manifest that does not match its
        // shaders is found at start-up rather than on the user's device.
        match engine.load_effects(manifest_text, &sources) {
            Ok(mismatches) => Ok(mismatches as i32),
            Err(_) => Err(status::ERROR),
        }
    })
}

/// List the projects in a directory.
///
/// The answer is a JSON array, one object per project: the name, the path,
/// when it was last changed and how big it is. It is written into the
/// caller's buffer, the same way the snapshot is.
///
/// Returns the number of bytes the answer needs, or a negative status.
///
/// # Safety
/// `out` must point to `capacity` writable bytes, or be null.
#[no_mangle]
pub unsafe extern "C" fn vimo_engine_list_projects(
    dir: *const u8,
    dir_len: usize,
    out: *mut u8,
    capacity: i32,
) -> i32 {
    guard_value(|| {
        if capacity < 0 {
            return Err(status::INVALID_ARGUMENT);
        }
        let dir = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(dir, dir_len) }) {
            Ok(text) => text,
            Err(_) => return Err(status::INVALID_ARGUMENT),
        };
        let json = list_projects(std::path::Path::new(dir));
        let bytes = json.as_bytes();
        if bytes.len() > capacity as usize {
            return Ok(bytes.len() as i32);
        }
        if !out.is_null() {
            // SAFETY: the caller guarantees `out` has `capacity` bytes.
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
            }
        }
        Ok(bytes.len() as i32)
    })
}

/// Delete a project.
///
/// # Safety
/// `path` must point to `path_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn vimo_engine_delete_project(path: *const u8, path_len: usize) -> i32 {
    guard_value(|| {
        let path = match std::str::from_utf8(unsafe { std::slice::from_raw_parts(path, path_len) })
        {
            Ok(text) => text,
            Err(_) => return Err(status::INVALID_ARGUMENT),
        };
        let path = std::path::Path::new(path);
        if !path.exists() {
            return Err(status::INVALID_ARGUMENT);
        }
        if std::fs::remove_file(path).is_err() {
            return Err(status::ERROR);
        }
        Ok(0)
    })
}

/// The projects in a directory, as JSON.
pub fn list_projects(dir: &std::path::Path) -> String {
    let mut projects = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // A directory that does not exist holds no projects.
        Err(_) => return "[]".to_string(),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("vimo") {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|since| since.as_secs())
            .unwrap_or(0);
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        projects.push(serde_json::json!({
            "name": name,
            "path": path.to_string_lossy(),
            "modified": modified,
            "size": metadata.len(),
        }));
    }
    serde_json::to_string(&projects).unwrap_or_else(|_| "[]".to_string())
}

/// The ABI version this library implements.
#[no_mangle]
pub extern "C" fn vimo_abi_version() -> u32 {
    VIMO_ABI_VERSION
}

/// The last error message for an engine, or an empty string.
///
/// The returned pointer is valid until the next call on the same thread.
///
/// # Safety
/// `engine` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn vimo_engine_last_error(engine: *mut VimoEngine) -> *const c_char {
    thread_local! {
        static BUFFER: std::cell::RefCell<CString> =
            std::cell::RefCell::new(CString::new("").unwrap());
    }
    guard_code(|| {
        if engine.is_null() {
            return Err(status::INVALID_HANDLE);
        }
        // SAFETY: the caller guarantees the handle is valid.
        let handle = unsafe { &*engine };
        let message = {
            let guard = match handle.last_error.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            guard
                .as_ref()
                .map(|m| m.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        BUFFER.with(|b| {
            let mut b = b.borrow_mut();
            *b = CString::new(message).unwrap_or_default();
        });
        Ok(())
    });
    BUFFER.with(|b| b.borrow().as_ptr())
}

/// Apply a command to the engine.
fn apply_command(engine: &mut Engine, cmd: &VimoCommand, handle: &VimoEngine) -> Result<(), i32> {
    match cmd.tag {
        command_tag::PLAY => {
            engine.play();
            Ok(())
        }
        command_tag::PAUSE => {
            engine.pause();
            Ok(())
        }
        command_tag::STOP => {
            engine.stop();
            Ok(())
        }
        command_tag::SEEK => {
            engine.seek(Time::from_nanos(cmd.a));
            Ok(())
        }
        command_tag::SET_RATE => {
            engine.set_rate((cmd.a as f64 / 1000.0) as f32);
            Ok(())
        }
        command_tag::UNDO => {
            engine.undo().map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::REDO => {
            engine.redo().map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::LOAD_PROJECT => {
            let path = read_text(cmd).ok_or(status::INVALID_ARGUMENT)?;
            let path = std::path::PathBuf::from(path);
            engine.load_project(&path).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::SAVE_PROJECT => {
            let path = read_text(cmd).ok_or(status::INVALID_ARGUMENT)?;
            let path = std::path::PathBuf::from(path);
            engine.save_project(&path).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::START_EXPORT => {
            let preset = match cmd.a {
                1 => ExportPreset::hd_720p(),
                2 => ExportPreset::uhd_2160p(),
                _ => ExportPreset::hd_1080p(),
            };
            engine.start_export(preset).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::CANCEL_EXPORT => {
            engine.cancel_export();
            Ok(())
        }
        command_tag::SET_SURFACE => {
            let mut surface = match handle.surface.lock() {
                Ok(s) => s,
                Err(p) => p.into_inner(),
            };
            *surface = cmd.a as usize;
            Ok(())
        }
        command_tag::THERMAL => {
            engine.report_thermal(vimo_device::ThermalReading {
                state: vimo_device::ThermalState::from_android(cmd.a as i32),
                temperature_c: None,
            });
            Ok(())
        }
        command_tag::MEMORY => {
            engine.refresh_memory_pressure();
            Ok(())
        }
        command_tag::SET_PREVIEW_SCALE => {
            engine.set_preview_scale((cmd.a as f64 / 1000.0) as f32);
            Ok(())
        }
        command_tag::ADD_CLIP => {
            let track = cmd.a as u32;
            let asset = cmd.b as u32;
            let start = Time::from_nanos(cmd.c);
            let project = engine.project();
            let Some(asset_data) = project.assets.get(asset as usize) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let duration = asset_data.duration;
            let clip = Clip::new(asset, TimeRange::new(start, start + duration), Time::ZERO);
            engine
                .execute(vimo_project::Command::add_clip(track, 0, clip))
                .map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::REMOVE_CLIP => {
            let track = cmd.a as u32;
            let clip = cmd.b as u32;
            let position = engine
                .project()
                .clip_position(track, clip)
                .ok_or(status::INVALID_ARGUMENT)?;
            engine
                .execute(vimo_project::Command::RemoveClip {
                    track,
                    list_index: position,
                    clip,
                })
                .map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::MOVE_CLIP => {
            let clip = cmd.a as u32;
            let track = cmd.b as u32;
            let new_start = Time::from_nanos(cmd.c);
            let project = engine.project();
            let Some(data) = project.clip(clip) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let duration = data.duration();
            let before = data.clone();
            let command = vimo_project::Command::edit_clip(clip, before, |c| {
                c.timeline = TimeRange::new(new_start, new_start + duration);
            });
            engine.execute(command).map_err(|_| status::ERROR)?;
            // Move it to the new track if the track changed.
            if let Some(from_track) = engine.project().track_of_clip(clip) {
                if from_track != track {
                    if let Some(position) = engine.project().clip_position(from_track, clip) {
                        let move_command = vimo_project::Command::RemoveClip {
                            track: from_track,
                            list_index: position,
                            clip,
                        };
                        engine.execute(move_command).map_err(|_| status::ERROR)?;
                    }
                }
            }
            Ok(())
        }
        command_tag::TRIM_CLIP => {
            let clip = cmd.a as u32;
            let start = Time::from_nanos(cmd.b);
            let end = Time::from_nanos(cmd.c);
            let Some(data) = engine.project().clip(clip) else {
                return Err(status::INVALID_ARGUMENT);
            };
            // The trim keeps the source in sync and is clamped to the media.
            let source_duration = engine
                .project()
                .asset(data.asset)
                .map(|a| a.duration)
                .unwrap_or(Time::ZERO);
            let before = data.clone();
            let trimmed = data.trimmed(TimeRange::new(start, end), source_duration);
            let command = vimo_project::Command::UpdateClip {
                clip,
                before,
                after: trimmed,
            };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::SPLIT_CLIP => {
            let clip = cmd.a as u32;
            let at = Time::from_nanos(cmd.b);
            let Some(data) = engine.project().clip(clip) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let Some(second) = data.split_at(at) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let Some(track) = engine.project().track_of_clip(clip) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let Some(position) = engine.project().clip_position(track, clip) else {
                return Err(status::INVALID_ARGUMENT);
            };
            // Trim the first clip, then add the second one after it.
            let source_duration = engine
                .project()
                .asset(data.asset)
                .map(|a| a.duration)
                .unwrap_or(Time::ZERO);
            let first = data.trimmed(TimeRange::new(data.timeline.start, at), source_duration);
            let trim = vimo_project::Command::UpdateClip {
                clip,
                before: data.clone(),
                after: first,
            };
            engine.execute(trim).map_err(|_| status::ERROR)?;
            let add = vimo_project::Command::add_clip(track, position + 1, second);
            engine.execute(add).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::SET_CLIP_ENABLED => {
            edit_clip(engine, cmd.a as u32, |c| c.enabled = cmd.b != 0)
        }
        command_tag::SET_CLIP_OPACITY => {
            let value = f32::from_bits(cmd.b as u32);
            edit_clip(engine, cmd.a as u32, |c| c.opacity = value.clamp(0.0, 1.0))
        }
        command_tag::SET_CLIP_BLEND => {
            let mode = blend_from_u8(cmd.b as u8);
            edit_clip(engine, cmd.a as u32, |c| c.blend = mode)
        }
        command_tag::SET_CLIP_TRANSFORM => {
            let floats = read_floats(cmd);
            if floats.len() < 7 {
                return Err(status::INVALID_ARGUMENT);
            }
            edit_clip(engine, cmd.a as u32, |c| {
                c.transform = Transform {
                    position: Vec2::new(floats[0], floats[1]),
                    scale: Vec2::new(floats[2], floats[3]),
                    rotation: floats[4],
                    anchor: Vec2::new(floats[5], floats[6]),
                    crop: c.transform.crop,
                };
            })
        }
        command_tag::SET_CLIP_VOLUME => {
            let value = f32::from_bits(cmd.b as u32);
            edit_clip(engine, cmd.a as u32, |c| c.volume = value.max(0.0))
        }
        command_tag::SET_CLIP_SPEED => {
            let value = f32::from_bits(cmd.b as u32);
            if value <= 0.0 {
                return Err(status::INVALID_ARGUMENT);
            }
            edit_clip(engine, cmd.a as u32, |c| c.speed = value)
        }
        command_tag::SET_CLIP_MUTED => edit_clip(engine, cmd.a as u32, |c| c.muted = cmd.b != 0),
        command_tag::ADD_TRACK => {
            let kind = if cmd.b != 0 {
                TrackKind::Audio
            } else {
                TrackKind::Video
            };
            let name = format!(
                "{} {}",
                if kind == TrackKind::Audio { "A" } else { "V" },
                engine.project().tracks.len() + 1
            );
            let command = vimo_project::Command::AddTrack {
                track: Track::new(name, kind),
            };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::REMOVE_TRACK => {
            let command = vimo_project::Command::RemoveTrack {
                index: cmd.a as u32,
            };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::SET_TRACK_ENABLED => {
            edit_track(engine, cmd.a as u32, |t| t.enabled = cmd.b != 0)
        }
        command_tag::SET_TRACK_MUTED => edit_track(engine, cmd.a as u32, |t| t.muted = cmd.b != 0),
        command_tag::SET_TRACK_SOLO => edit_track(engine, cmd.a as u32, |t| t.solo = cmd.b != 0),
        command_tag::SET_TRACK_LOCKED => {
            edit_track(engine, cmd.a as u32, |t| t.locked = cmd.b != 0)
        }
        command_tag::SET_TRACK_VOLUME => {
            let value = f32::from_bits(cmd.b as u32);
            edit_track(engine, cmd.a as u32, |t| t.volume = value.max(0.0))
        }
        command_tag::ADD_EFFECT => {
            let owner = owner_from(cmd.a as u32, cmd.b as u32);
            let type_index = cmd.c as u32;
            let Some(desc) = engine
                .registry()
                .get(vimo_manifest::EffectTypeId::from_index(type_index))
            else {
                return Err(status::INVALID_ARGUMENT);
            };
            let node = EffectNode::new(vimo_manifest::EffectTypeId::from_index(type_index), desc);
            let command = vimo_project::Command::AddEffect {
                owner,
                effect: node,
            };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::REMOVE_EFFECT => {
            let owner = owner_from(cmd.a as u32, cmd.b as u32);
            let effect = cmd.c as u32;
            let list_index = match owner {
                EffectOwner::Clip(id) => engine
                    .project()
                    .clip(id)
                    .and_then(|c| c.effects.iter().position(|e| *e == effect))
                    .unwrap_or(0),
                EffectOwner::Track(id) => engine
                    .project()
                    .track(id)
                    .and_then(|t| t.effects.iter().position(|e| *e == effect))
                    .unwrap_or(0),
                EffectOwner::Transition(_) => 0,
            };
            let command = vimo_project::Command::RemoveEffect {
                owner,
                list_index,
                effect,
            };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::SET_EFFECT_ENABLED => {
            let effect = cmd.a as u32;
            let Some(node) = engine.project().effect(effect) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let before = node.clone();
            let command = vimo_project::Command::edit_effect(effect, before, |e| {
                e.enabled = cmd.b != 0;
            });
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::SET_PARAM => {
            let effect = cmd.a as u32;
            let param = cmd.b as u16;
            let value = f32::from_bits(cmd.c as u32);
            let Some(node) = engine.project().effect(effect) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let Some(desc) = engine.registry().get(node.effect_type) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let Some(param_desc) = desc.param(param) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let before = node
                .params
                .get(param)
                .unwrap_or(vimo_manifest::ParamValue::Float(0.0));
            let after = param_desc.coerce(vimo_manifest::ParamValue::Float(value));
            let command = vimo_project::Command::SetParam {
                effect,
                param,
                before,
                after,
            };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::ADD_KEYFRAME => {
            let effect = cmd.a as u32;
            let track = cmd.b as usize;
            let time = Time::from_nanos(cmd.c);
            let floats = read_floats(cmd);
            let value = floats.first().copied().unwrap_or(0.0);
            let Some(node) = engine.project().effect(effect) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let Some(anim) = node.animation.get(track) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let param = anim.param;
            let Some(desc) = engine.registry().get(node.effect_type) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let Some(param_desc) = desc.param(param) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let key = Keyframe::new(
                time,
                param_desc.coerce(vimo_manifest::ParamValue::Float(value)),
            );
            let command = vimo_project::Command::AddKeyframe { effect, track, key };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::REMOVE_KEYFRAME => {
            let effect = cmd.a as u32;
            let track = cmd.b as usize;
            let index = cmd.c as usize;
            let Some(node) = engine.project().effect(effect) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let Some(anim) = node.animation.get(track) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let Some(key) = anim.keys.get(index) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let command = vimo_project::Command::RemoveKeyframe {
                effect,
                track,
                index,
                key: *key,
            };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::SET_KEYFRAME_INTERPOLATION => {
            let effect = cmd.a as u32;
            let track = cmd.b as usize;
            let index = cmd.c as usize;
            let floats = read_floats(cmd);
            let kind = floats.first().copied().unwrap_or(0.0) as u32;
            let interpolation = match kind {
                1 => Interpolation::Hold,
                2 => Interpolation::Bezier {
                    x1: floats.get(1).copied().unwrap_or(0.0),
                    y1: floats.get(2).copied().unwrap_or(0.0),
                    x2: floats.get(3).copied().unwrap_or(0.0),
                    y2: floats.get(4).copied().unwrap_or(1.0),
                },
                _ => Interpolation::Linear,
            };
            let Some(node) = engine.project().effect(effect) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let Some(anim) = node.animation.get(track) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let Some(key) = anim.keys.get(index) else {
                return Err(status::INVALID_ARGUMENT);
            };
            let before = key.interpolation;
            let command = vimo_project::Command::SetKeyframeInterpolation {
                effect,
                track,
                index,
                before,
                after: interpolation,
            };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::ADD_ASSET => {
            let source = read_text(cmd).ok_or(status::INVALID_ARGUMENT)?;
            let floats = read_floats(cmd);
            let duration = Time::from_secs_f64(floats.first().copied().unwrap_or(0.0) as f64);
            let asset = Asset::video(source, duration);
            let command = vimo_project::Command::AddAsset { asset };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::REMOVE_ASSET => {
            let command = vimo_project::Command::RemoveAsset {
                index: cmd.a as u32,
            };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::ADD_TRANSITION => {
            let track = cmd.a as u32;
            let from_clip = cmd.b as u32;
            let to_clip = cmd.c as u32;
            let floats = read_floats(cmd);
            let duration = Time::from_secs_f64(floats.first().copied().unwrap_or(0.5) as f64);
            let transition = Transition::cross_dissolve(track, from_clip, to_clip, duration);
            let command = vimo_project::Command::AddTransition { transition };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::REMOVE_TRANSITION => {
            let command = vimo_project::Command::RemoveTransition {
                index: cmd.a as u32,
            };
            engine.execute(command).map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::SET_PROJECT_NAME => {
            let name = read_text(cmd).ok_or(status::INVALID_ARGUMENT)?;
            let project = engine.project_mut();
            project.meta.name = name;
            engine.invalidate_project();
            Ok(())
        }
        command_tag::SET_RESOLUTION => {
            let project = engine.project_mut();
            project.meta.width = cmd.a.max(1) as u32;
            project.meta.height = cmd.b.max(1) as u32;
            engine.invalidate_project();
            Ok(())
        }
        command_tag::SET_FPS => {
            let project = engine.project_mut();
            let fps = vimo_foundation::time::Rational::new(cmd.a, cmd.b)
                .unwrap_or(vimo_foundation::time::Rational::ONE);
            project.meta.timebase = vimo_foundation::time::Timebase::from_fps(fps.num(), fps.den());
            engine.invalidate_project();
            Ok(())
        }
        command_tag::SET_AUTOSAVE_DIR => {
            let dir = read_text(cmd).ok_or(status::INVALID_ARGUMENT)?;
            engine.files_mut().set_autosave_dir(dir);
            Ok(())
        }
        command_tag::RECOVER_AUTOSAVE => {
            engine.recover_autosave().map_err(|_| status::ERROR)?;
            Ok(())
        }
        command_tag::SET_LOOP => {
            let range = if cmd.c != 0 {
                Some((Time::from_nanos(cmd.a), Time::from_nanos(cmd.b)))
            } else {
                None
            };
            engine.clock_mut().set_loop(range);
            Ok(())
        }
        command_tag::EXPORT_PAUSE => {
            engine.pause_export();
            Ok(())
        }
        command_tag::EXPORT_RESUME => {
            engine.resume_export();
            Ok(())
        }
        command_tag::NEW_PROJECT => {
            engine.new_project();
            Ok(())
        }
        command_tag::REQUEST_SNAPSHOT => {
            let json = engine.snapshot_json();
            engine.push_event(EngineEvent::Snapshot { json });
            Ok(())
        }
        _ => Err(status::INVALID_ARGUMENT),
    }
}

/// Edit a clip through a command.
fn edit_clip(engine: &mut Engine, clip: u32, mutate: impl FnOnce(&mut Clip)) -> Result<(), i32> {
    let Some(data) = engine.project().clip(clip) else {
        return Err(status::INVALID_ARGUMENT);
    };
    let before = data.clone();
    let command = vimo_project::Command::edit_clip(clip, before, mutate);
    engine.execute(command).map_err(|_| status::ERROR)
}

/// Edit a track through a command.
fn edit_track(engine: &mut Engine, track: u32, mutate: impl FnOnce(&mut Track)) -> Result<(), i32> {
    let Some(data) = engine.project().track(track) else {
        return Err(status::INVALID_ARGUMENT);
    };
    let before = data.clone();
    let command = vimo_project::Command::edit_track(track, before, mutate);
    engine.execute(command).map_err(|_| status::ERROR)
}

/// The effect owner for a command.
fn owner_from(kind: u32, id: u32) -> EffectOwner {
    if kind == 1 {
        EffectOwner::Track(id)
    } else {
        EffectOwner::Clip(id)
    }
}

/// The blend mode for a numeric code.
fn blend_from_u8(v: u8) -> BlendMode {
    BlendMode::from_u8(v)
}

/// Read the optional text payload of a command.
fn read_text(cmd: &VimoCommand) -> Option<String> {
    if cmd.text.is_null() || cmd.text_len == 0 {
        return None;
    }
    // SAFETY: the caller guarantees `text` points to `text_len` readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(cmd.text, cmd.text_len as usize) };
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Read the optional float payload of a command.
fn read_floats(cmd: &VimoCommand) -> Vec<f32> {
    if cmd.floats.is_null() || cmd.float_count == 0 {
        return Vec::new();
    }
    // SAFETY: the caller guarantees `floats` points to `float_count` readable
    // floats.
    let bytes = unsafe { std::slice::from_raw_parts(cmd.floats, cmd.float_count as usize) };
    bytes.to_vec()
}

/// Convert an engine event into its FFI representation.
fn convert_event(event: &EngineEvent) -> (u32, i64, i64, i32, CString) {
    match event {
        EngineEvent::ProjectChanged { revision } => (
            event_tag::PROJECT_CHANGED,
            *revision as i64,
            0,
            0,
            CString::new("").unwrap(),
        ),
        EngineEvent::PlaybackStateChanged(state) => (
            event_tag::PLAYBACK_STATE,
            match state {
                vimo_engine::PlaybackState::Stopped => 0,
                vimo_engine::PlaybackState::Playing => 1,
                vimo_engine::PlaybackState::Paused => 2,
            },
            0,
            0,
            CString::new("").unwrap(),
        ),
        EngineEvent::PositionChanged(t) => (
            event_tag::POSITION,
            t.as_nanos(),
            0,
            0,
            CString::new("").unwrap(),
        ),
        EngineEvent::FrameRendered { time, reused } => (
            event_tag::FRAME_RENDERED,
            time.as_nanos(),
            i64::from(*reused),
            0,
            CString::new("").unwrap(),
        ),
        EngineEvent::MemoryPressure(level) => (
            event_tag::MEMORY_PRESSURE,
            match level {
                vimo_resources::PressureLevel::Normal => 0,
                vimo_resources::PressureLevel::Warning => 1,
                vimo_resources::PressureLevel::Critical => 2,
            },
            0,
            0,
            CString::new("").unwrap(),
        ),
        EngineEvent::Thermal(state) => (
            event_tag::THERMAL,
            *state as i64,
            0,
            0,
            CString::new("").unwrap(),
        ),
        EngineEvent::ExportProgress(progress) => (
            event_tag::EXPORT_PROGRESS,
            progress.frame as i64,
            progress.total_frames as i64,
            0,
            CString::new("").unwrap(),
        ),
        EngineEvent::ExportFinished(state) => (
            event_tag::EXPORT_FINISHED,
            *state as i64,
            0,
            0,
            CString::new("").unwrap(),
        ),
        EngineEvent::Error { code, message } => (
            event_tag::ERROR,
            0,
            0,
            *code,
            CString::new(message.clone()).unwrap_or_default(),
        ),
        EngineEvent::Snapshot { json } => (
            event_tag::SNAPSHOT,
            0,
            0,
            0,
            CString::new(json.clone()).unwrap_or_default(),
        ),
    }
}

/// The uniform name of a manifest parameter, for the platform layer.
#[must_use]
pub fn param_uniform_name(name: &str) -> String {
    uniform_name(name)
}

/// The error code of a kind, for the platform layer.
#[must_use]
pub fn kind_code(kind: ErrorKind) -> i32 {
    kind.code()
}
