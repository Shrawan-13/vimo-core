//! FFI boundary tests.
//!
//! These exercise the C ABI exactly as the Android layer would: opaque
//! handles, commands in, events out. They also check the panic-safety
//! guarantee — a bad argument must produce an error code, never a crash.

use std::ffi::c_void;

use vimo_core::{
    command_tag, event_tag, status, vimo_abi_version, vimo_engine_create, vimo_engine_destroy,
    vimo_engine_execute, vimo_engine_poll_events, vimo_engine_snapshot, vimo_engine_stats,
    VimoCommand, VimoConfig, VimoEvent, VimoStats,
};

fn make_config() -> VimoConfig {
    VimoConfig {
        width: 1280,
        height: 720,
        sample_rate: 48_000,
        channels: 2,
        flags: 0,
    }
}

/// A command with no payload.
fn cmd(tag: u32) -> VimoCommand {
    VimoCommand {
        tag,
        ..Default::default()
    }
}

/// A command with a text payload.
fn cmd_text(tag: u32, text: &str) -> (VimoCommand, Vec<u8>) {
    let bytes = text.as_bytes().to_vec();
    (
        VimoCommand {
            tag,
            text: bytes.as_ptr(),
            text_len: bytes.len() as u32,
            ..Default::default()
        },
        bytes,
    )
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
fn playback_commands_are_executed() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };
    assert!(!engine.is_null());

    for tag in [
        command_tag::PLAY,
        command_tag::PAUSE,
        command_tag::STOP,
        command_tag::SEEK,
    ] {
        let c = VimoCommand {
            tag,
            a: 1_000_000_000,
            ..Default::default()
        };
        // SAFETY: both pointers are valid.
        assert_eq!(unsafe { vimo_engine_execute(engine, &c) }, status::OK);
    }

    // SAFETY: the handle is valid.
    unsafe { vimo_engine_destroy(engine) };
}

#[test]
fn unknown_commands_are_rejected() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };
    // SAFETY: both pointers are valid.
    assert_eq!(
        unsafe { vimo_engine_execute(engine, &cmd(9999)) },
        status::INVALID_ARGUMENT
    );
    // SAFETY: the handle is valid.
    unsafe { vimo_engine_destroy(engine) };
}

#[test]
fn editing_commands_are_executed() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };
    assert!(!engine.is_null());

    // Add an asset, a track and a clip, then edit the clip.
    let (add_asset, asset_bytes) = cmd_text(command_tag::ADD_ASSET, "clip.mp4");
    // SAFETY: both pointers are valid and the text outlives the call.
    assert_eq!(
        unsafe { vimo_engine_execute(engine, &add_asset) },
        status::OK
    );
    drop(asset_bytes);

    assert_eq!(
        unsafe { vimo_engine_execute(engine, &cmd(command_tag::ADD_TRACK)) },
        status::OK
    );

    let add_clip = VimoCommand {
        tag: command_tag::ADD_CLIP,
        a: 0, // track
        b: 0, // asset
        c: 0, // start
        ..Default::default()
    };
    // SAFETY: both pointers are valid.
    assert_eq!(
        unsafe { vimo_engine_execute(engine, &add_clip) },
        status::OK
    );

    let opacity = VimoCommand {
        tag: command_tag::SET_CLIP_OPACITY,
        a: 0,
        b: f32::to_bits(0.5) as i64,
        ..Default::default()
    };
    // SAFETY: both pointers are valid.
    assert_eq!(unsafe { vimo_engine_execute(engine, &opacity) }, status::OK);

    // Undo it.
    // SAFETY: both pointers are valid.
    assert_eq!(
        unsafe { vimo_engine_execute(engine, &cmd(command_tag::UNDO)) },
        status::OK
    );
    // SAFETY: both pointers are valid.
    assert_eq!(
        unsafe { vimo_engine_execute(engine, &cmd(command_tag::REDO)) },
        status::OK
    );

    // SAFETY: the handle is valid.
    unsafe { vimo_engine_destroy(engine) };
}

#[test]
fn a_missing_asset_is_rejected() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };
    let add_clip = VimoCommand {
        tag: command_tag::ADD_CLIP,
        a: 0,
        b: 42, // no such asset
        c: 0,
        ..Default::default()
    };
    // SAFETY: both pointers are valid.
    assert_eq!(
        unsafe { vimo_engine_execute(engine, &add_clip) },
        status::INVALID_ARGUMENT
    );
    // SAFETY: the handle is valid.
    unsafe { vimo_engine_destroy(engine) };
}

#[test]
fn events_are_polled() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };

    // SAFETY: both pointers are valid.
    unsafe { vimo_engine_execute(engine, &cmd(command_tag::PLAY)) };

    let mut events = [VimoEvent::default(); 16];
    // SAFETY: the handle and the buffer are valid.
    let count = unsafe { vimo_engine_poll_events(engine, events.as_mut_ptr(), 16) };
    assert!(count >= 0, "poll failed: {count}");
    assert!(count > 0, "playing should emit an event");
    // SAFETY: the handle is valid.
    unsafe { vimo_engine_destroy(engine) };
}

#[test]
fn the_snapshot_is_readable() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };

    // SAFETY: the handle is valid; a null buffer asks for the required size.
    let needed = unsafe { vimo_engine_snapshot(engine, std::ptr::null_mut(), 0) };
    assert!(needed > 0, "the snapshot must not be empty");

    let mut buffer = vec![0u8; needed as usize];
    // SAFETY: the handle and the buffer are valid.
    let written = unsafe { vimo_engine_snapshot(engine, buffer.as_mut_ptr(), needed) };
    assert_eq!(written, needed);
    let text = String::from_utf8_lossy(&buffer);
    assert!(
        text.contains("\"tracks\""),
        "the snapshot must describe the project"
    );
    assert!(text.contains("\"meta\""));

    // SAFETY: the handle is valid.
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
    let c = VimoCommand {
        tag: command_tag::SET_SURFACE,
        a: fake_window as i64,
        ..Default::default()
    };
    // SAFETY: both pointers are valid.
    assert_eq!(unsafe { vimo_engine_execute(engine, &c) }, status::OK);
    // SAFETY: the handle is valid.
    unsafe { vimo_engine_destroy(engine) };
}

#[test]
fn the_abi_version_is_exposed() {
    assert_eq!(vimo_abi_version(), 2);
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
    assert_eq!(event_tag::SNAPSHOT, 10);
}

#[test]
fn command_tags_are_stable() {
    // The Kotlin side sends these numbers; they must never change.
    assert_eq!(command_tag::PLAY, 1);
    assert_eq!(command_tag::ADD_CLIP, 8);
    assert_eq!(command_tag::SET_PARAM, 10);
    assert_eq!(command_tag::SPLIT_CLIP, 21);
    assert_eq!(command_tag::ADD_EFFECT, 36);
    assert_eq!(command_tag::REQUEST_SNAPSHOT, 54);
}

#[test]
fn a_project_can_be_saved_and_loaded() {
    let config = make_config();
    // SAFETY: the config outlives the call.
    let engine = unsafe { vimo_engine_create(&config) };

    let dir = std::env::temp_dir().join(format!("vimo-ffi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("project.vimo");
    let path_text = path.to_string_lossy().into_owned();

    let (save, save_bytes) = cmd_text(command_tag::SAVE_PROJECT, &path_text);
    // SAFETY: both pointers are valid and the text outlives the call.
    assert_eq!(unsafe { vimo_engine_execute(engine, &save) }, status::OK);
    drop(save_bytes);
    assert!(path.exists());

    let (load, load_bytes) = cmd_text(command_tag::LOAD_PROJECT, &path_text);
    // SAFETY: both pointers are valid and the text outlives the call.
    assert_eq!(unsafe { vimo_engine_execute(engine, &load) }, status::OK);
    drop(load_bytes);

    // SAFETY: the handle is valid.
    unsafe { vimo_engine_destroy(engine) };
    let _ = std::fs::remove_dir_all(&dir);
}
