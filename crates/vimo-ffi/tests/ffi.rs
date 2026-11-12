//! FFI boundary tests.
//!
//! These exercise the C ABI exactly as the Android layer would: opaque
//! handles, commands in, events out. They also check the panic-safety
//! guarantee — a bad argument must produce an error code, never a crash.

use std::ffi::c_void;

use vimo_core::{
    command_tag, event_tag, status, vimo_abi_version, vimo_engine_create, vimo_engine_destroy,
    vimo_engine_execute, vimo_engine_poll_events, vimo_engine_stats, VimoCommand, VimoConfig,
    VimoEvent, VimoStats,
};

fn make_config() -> VimoConfig {
    VimoConfig { width: 1280, height: 720, sample_rate: 48_000, channels: 2, flags: 0 }
}

#[test]
fn create_and_destroy() {
    let config = make_config();
    // SAFETY: the config outlives the call and the handle is destroyed below.
    let engine = unsafe { vimo_engine_create(&config) };
    assert!(!engine.is_null());
    // SAFETY: the handle was just created and is not used afterwards.
    assert_eq!(unsafe { vimo_engine_destroy(engine) }, status::OK);
}

#[test]
fn null_handles_are_rejected() {
    // SAFETY: null pointers are explicitly handled by the boundary.
    assert_eq!(
        unsafe { vimo_engine_destroy(std::ptr::null_mut()) },
        status::INVALID_HANDLE
    );
    // SAFETY: null pointers are explicitly handled by the boundary.
    assert_eq!(
        unsafe { vimo_engine_execute(std::ptr::null_mut(), std::ptr::null()) },
        status::INVALID_HANDLE
    );
}

#[test]
fn create_with_null_config_is_rejected() {
    // SAFETY: null is explicitly handled.
    let engine = unsafe { vimo_engine_create(std::ptr::null()) };
    assert!(engine.is_null());
}

#[test]
fn commands_are_executed() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };
    assert!(!engine.is_null());

    for tag in [command_tag::PLAY, command_tag::PAUSE, command_tag::STOP, command_tag::SEEK] {
        let cmd = VimoCommand { tag, a: 1_000_000_000, ..Default::default() };
        // SAFETY: both pointers are valid.
        assert_eq!(unsafe { vimo_engine_execute(engine, &cmd) }, status::OK);
    }

    // SAFETY: the handle is valid.
    unsafe { vimo_engine_destroy(engine) };
}

#[test]
fn unknown_commands_are_rejected() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };
    let cmd = VimoCommand { tag: 9999, ..Default::default() };
    // SAFETY: both pointers are valid.
    assert_eq!(unsafe { vimo_engine_execute(engine, &cmd) }, status::INVALID_ARGUMENT);
    // SAFETY: the handle is valid.
    unsafe { vimo_engine_destroy(engine) };
}

#[test]
fn events_are_polled() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };

    let cmd = VimoCommand { tag: command_tag::PLAY, ..Default::default() };
    // SAFETY: both pointers are valid.
    unsafe { vimo_engine_execute(engine, &cmd) };

    let mut events = [VimoEvent::default(); 16];
    // SAFETY: the handle and the buffer are valid.
    let count = unsafe { vimo_engine_poll_events(engine, events.as_mut_ptr(), 16) };
    assert!(count >= 0, "poll failed: {count}");
    // SAFETY: the handle and the buffer are valid.
    unsafe { vimo_engine_destroy(engine) };
}

#[test]
fn stats_are_readable() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };
    let mut stats = VimoStats::default();
    // SAFETY: the handle and the pointer are valid.
    assert_eq!(unsafe { vimo_engine_stats(engine, &mut stats) }, status::OK);
    // SAFETY: the handle is valid.
    unsafe { vimo_engine_destroy(engine) };
}

#[test]
fn the_surface_is_handed_over_once() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };

    // Pretend to hand over an `ANativeWindow`.
    let fake_window = 0x1234_5678usize as *mut c_void;
    let cmd = VimoCommand {
        tag: command_tag::SET_SURFACE,
        a: fake_window as i64,
        ..Default::default()
    };
    // SAFETY: both pointers are valid.
    assert_eq!(unsafe { vimo_engine_execute(engine, &cmd) }, status::OK);
    // SAFETY: the handle is valid.
    unsafe { vimo_engine_destroy(engine) };
}

#[test]
fn the_abi_version_is_exposed() {
    assert_eq!(vimo_abi_version(), 1);
}

#[test]
fn polling_with_a_null_buffer_is_rejected() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };
    // SAFETY: null is explicitly handled.
    assert_eq!(
        unsafe { vimo_engine_poll_events(engine, std::ptr::null_mut(), 4) },
        status::INVALID_HANDLE
    );
    // SAFETY: the handle is valid.
    unsafe { vimo_engine_destroy(engine) };
}

#[test]
fn event_tags_are_stable() {
    // The Kotlin side switches on these numbers; they must never change.
    assert_eq!(event_tag::PROJECT_CHANGED, 1);
    assert_eq!(event_tag::PLAYBACK_STATE, 2);
    assert_eq!(event_tag::POSITION, 3);
    assert_eq!(event_tag::FRAME_RENDERED, 4);
    assert_eq!(event_tag::MEMORY_PRESSURE, 5);
    assert_eq!(event_tag::THERMAL, 6);
    assert_eq!(event_tag::EXPORT_PROGRESS, 7);
    assert_eq!(event_tag::EXPORT_FINISHED, 8);
    assert_eq!(event_tag::ERROR, 9);
}
