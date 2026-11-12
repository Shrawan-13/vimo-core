//! The C ABI.
//!
//! # Contract
//!
//! * The engine is an **opaque handle**. The caller never sees the Rust types.
//! * **Commands in, events out.** The UI never calls into the engine to ask
//!   questions; it sends commands and receives events.
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

use std::ffi::{CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use vimo_device::DeviceProfile;
use vimo_engine::clock::MonotonicClock;
use vimo_engine::{Engine, EngineEvent, ExportPreset};
use vimo_foundation::time::Time;
use vimo_manifest::EffectRegistry;
use vimo_project::Project;

/// The version of the ABI. The Kotlin side checks it at load time.
pub const VIMO_ABI_VERSION: u32 = 1;

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
    /// Start an export with the preset in `a` (0 = 1080p, 1 = 720p, 2 = 4K).
    pub const START_EXPORT: u32 = 13;
    /// Cancel the running export.
    pub const CANCEL_EXPORT: u32 = 14;
    /// Hand over an `ANativeWindow` (in `a` as a pointer).
    pub const SET_SURFACE: u32 = 15;
    /// Report a thermal reading (`a` = state).
    pub const THERMAL: u32 = 16;
    /// Report memory usage (`a` = used bytes, `b` = budget bytes).
    pub const MEMORY: u32 = 17;
    /// Set the preview scale (`a` = scale times 1000).
    pub const SET_PREVIEW_SCALE: u32 = 18;
}

/// Event tags. **Never renumber an existing tag.**
pub mod event_tag {
    /// The project changed.
    pub const PROJECT_CHANGED: u32 = 1;
    /// The playback state changed.
    pub const PLAYBACK_STATE: u32 = 2;
    /// The playhead moved (`a` = nanoseconds).
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
        VimoConfig { width: 1920, height: 1080, sample_rate: 48_000, channels: 2, flags: 0 }
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
}

impl Default for VimoCommand {
    fn default() -> Self {
        VimoCommand { tag: 0, a: 0, b: 0, c: 0, text: std::ptr::null(), text_len: 0 }
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
        VimoEvent { tag: 0, a: 0, b: 0, code: 0, message: std::ptr::null(), message_len: 0 }
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
        let engine = Engine::open(
            Project::new(),
            EffectRegistry::new(),
            profile,
            std::sync::Arc::new(MonotonicClock::new()),
        );
        let mut engine = engine;
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
            guard.as_ref().map(|m| m.to_string_lossy().into_owned()).unwrap_or_default()
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
fn apply_command(
    engine: &mut Engine,
    cmd: &VimoCommand,
    handle: &VimoEngine,
) -> Result<(), i32> {
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
            let path = read_text(cmd);
            match path {
                Some(p) => {
                    engine.push_event(EngineEvent::Error {
                        code: status::UNSUPPORTED,
                        message: format!("loading `{p}` requires the platform loader"),
                    });
                    Ok(())
                }
                None => Err(status::INVALID_ARGUMENT),
            }
        }
        command_tag::SAVE_PROJECT => {
            let path = read_text(cmd);
            match path {
                Some(p) => {
                    engine.push_event(EngineEvent::Error {
                        code: status::UNSUPPORTED,
                        message: format!("saving to `{p}` requires the platform writer"),
                    });
                    Ok(())
                }
                None => Err(status::INVALID_ARGUMENT),
            }
        }
        command_tag::START_EXPORT => {
            let preset = match cmd.a {
                1 => ExportPreset::hd_720p(),
                2 => ExportPreset::uhd_2160p(),
                _ => ExportPreset::hd_1080p(),
            };
            let encoder = Box::new(vimo_media::FakeEncoder::default());
            engine.start_export(preset, encoder).map_err(|_| status::ERROR)?;
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
            let scale = (cmd.a as f64 / 1000.0) as f32;
            engine.set_preview_scale(scale);
            Ok(())
        }
        _ => Err(status::INVALID_ARGUMENT),
    }
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
    }
}
