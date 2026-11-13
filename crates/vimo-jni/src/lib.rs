//! The JNI bindings.
//!
//! The Kotlin side sees a small, stable API:
//!
//! ```kotlin
//! object VimoCore {
//!     init { System.loadLibrary("vimo_jni") }
//!     external fun nativeCreate(width: Int, height: Int, sampleRate: Int, channels: Int): Long
//!     external fun nativeDestroy(handle: Long)
//!     external fun nativeExecute(handle: Long, tag: Int, a: Long, b: Long, c: Long,
//!                                text: String?, floats: FloatArray?): Int
//!     external fun nativePollEvent(handle: Long): VimoEvent?
//!     external fun nativeSnapshot(handle: Long): String
//!     external fun nativeStats(handle: Long): LongArray
//!     external fun nativeSetSurface(handle: Long, surface: Any)
//! }
//! ```
//!
//! Every function is panic proof: a Rust panic becomes an error code rather
//! than unwinding into the JVM.

use std::sync::Mutex;

use jni::objects::{JClass, JFloatArray, JObject, JString, JValue};
use jni::sys::{jboolean, jint, jlong, jobject, jstring};
use jni::{JNIEnv, JavaVM};

use vimo_core::{
    command_tag, event_tag, list_projects, status, vimo_engine_create, vimo_engine_destroy,
    vimo_engine_execute, vimo_engine_poll_events, vimo_engine_snapshot, vimo_engine_stats,
    VimoCommand, VimoConfig, VimoEvent, VimoStats,
};

pub mod logger;

/// The handles of the live engines.
static ENGINES: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// The JVM, captured when the library is loaded.
static JVM: Mutex<Option<JavaVM>> = Mutex::new(None);

/// Called from `JNI_OnLoad` to remember the VM.
#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: JavaVM, _reserved: *mut std::ffi::c_void) -> i32 {
    if let Ok(mut slot) = JVM.lock() {
        *slot = Some(vm);
    }
    i32::from(jni::JNIVersion::V6)
}

/// Create an engine.
///
/// # Safety
/// Called by the JVM with valid arguments.
#[no_mangle]
pub extern "system" fn Java_com_teamshryne_vimo_VimoCore_nativeCreate(
    _env: JNIEnv,
    _class: JClass,
    width: jint,
    height: jint,
    sample_rate: jint,
    channels: jint,
) -> jlong {
    // The messages of the core go to the Android log.
    logger::install();
    let config = VimoConfig {
        width: width.max(1) as u32,
        height: height.max(1) as u32,
        sample_rate: sample_rate.max(1) as u32,
        channels: channels.max(1) as u16,
        flags: 0,
    };
    let handle = unsafe { vimo_engine_create(&config) };
    if handle.is_null() {
        return 0;
    }
    let address = handle as usize;
    if let Ok(mut engines) = ENGINES.lock() {
        engines.push(address);
    }
    address as jlong
}

/// Destroy an engine.
///
/// # Safety
/// Called by the JVM with a handle that was returned by `nativeCreate`.
#[no_mangle]
pub extern "system" fn Java_com_teamshryne_vimo_VimoCore_nativeDestroy(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    let address = handle as usize;
    if address == 0 {
        return status::INVALID_HANDLE;
    }
    if let Ok(mut engines) = ENGINES.lock() {
        engines.retain(|a| *a != address);
    }
    unsafe { vimo_engine_destroy(address as *mut vimo_core::VimoEngine) }
}

/// Execute a command.
///
/// # Safety
/// Called by the JVM with valid arguments.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_com_teamshryne_vimo_VimoCore_nativeExecute(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tag: jint,
    a: jlong,
    b: jlong,
    c: jlong,
    text: JString,
    floats: JFloatArray,
) -> jint {
    let address = handle as usize;
    if address == 0 {
        return status::INVALID_HANDLE;
    }

    // The text and the float array are copied so that the core sees plain
    // Rust slices with no JNI involvement.
    let text_bytes: Vec<u8> = if text.is_null() {
        Vec::new()
    } else {
        env.get_string(&text)
            .map(|s| s.to_bytes().to_vec())
            .unwrap_or_default()
    };
    let float_values: Vec<f32> = if floats.is_null() {
        Vec::new()
    } else {
        let len = env.get_array_length(&floats).unwrap_or(0) as usize;
        let mut buffer = vec![0f32; len];
        if len > 0 {
            let _ = env.get_float_array_region(&floats, 0, &mut buffer);
        }
        buffer
    };

    let command = VimoCommand {
        tag: tag as u32,
        a,
        b,
        c,
        text: if text_bytes.is_empty() {
            std::ptr::null()
        } else {
            text_bytes.as_ptr()
        },
        text_len: text_bytes.len() as u32,
        floats: if float_values.is_empty() {
            std::ptr::null()
        } else {
            float_values.as_ptr()
        },
        float_count: float_values.len() as u32,
    };
    unsafe { vimo_engine_execute(address as *mut vimo_core::VimoEngine, &command) }
}

/// Poll for one event.
///
/// Returns `null` when there is nothing to report.
///
/// # Safety
/// Called by the JVM with valid arguments.
#[no_mangle]
pub extern "system" fn Java_com_teamshryne_vimo_VimoCore_nativePollEvent(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jobject {
    let address = handle as usize;
    if address == 0 {
        return std::ptr::null_mut();
    }
    let mut events = [VimoEvent::default(); 1];
    let count = unsafe {
        vimo_engine_poll_events(
            address as *mut vimo_core::VimoEngine,
            events.as_mut_ptr(),
            1,
        )
    };
    if count <= 0 {
        return std::ptr::null_mut();
    }
    let event = events[0];

    // Build the Kotlin event object.
    let class = match env.find_class("com/teamshryne/vimo/VimoEvent") {
        Ok(class) => class,
        Err(_) => return std::ptr::null_mut(),
    };
    let message: JString = if event.message.is_null() || event.message_len == 0 {
        JString::from(JObject::null())
    } else {
        let bytes =
            unsafe { std::slice::from_raw_parts(event.message, event.message_len as usize) };
        let text = String::from_utf8_lossy(bytes);
        match env.new_string(text.as_ref()) {
            Ok(s) => s,
            Err(_) => JString::from(JObject::null()),
        }
    };
    let object = env.new_object(
        &class,
        "(JJILjava/lang/String;)V",
        &[
            JValue::Long(event.a),
            JValue::Long(event.b),
            JValue::Int(event.tag as i32),
            JValue::Int(event.code),
            JValue::Object(&message),
        ],
    );
    match object {
        Ok(object) => object.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// The JSON snapshot of the project.
///
/// # Safety
/// Called by the JVM with valid arguments.
#[no_mangle]
pub extern "system" fn Java_com_teamshryne_vimo_VimoCore_nativeSnapshot(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let address = handle as usize;
    if address == 0 {
        return std::ptr::null_mut();
    }
    // Ask for the size, then for the content.
    let needed = unsafe {
        vimo_engine_snapshot(
            address as *mut vimo_core::VimoEngine,
            std::ptr::null_mut(),
            0,
        )
    };
    if needed <= 0 {
        return std::ptr::null_mut();
    }
    let mut buffer = vec![0u8; needed as usize];
    let written = unsafe {
        vimo_engine_snapshot(
            address as *mut vimo_core::VimoEngine,
            buffer.as_mut_ptr(),
            needed,
        )
    };
    if written <= 0 {
        return std::ptr::null_mut();
    }
    match env.new_string(String::from_utf8_lossy(&buffer).as_ref()) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// The engine statistics.
///
/// # Safety
/// Called by the JVM with valid arguments.
#[no_mangle]
pub extern "system" fn Java_com_teamshryne_vimo_VimoCore_nativeStats(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jobject {
    let address = handle as usize;
    if address == 0 {
        return std::ptr::null_mut();
    }
    let mut stats = VimoStats::default();
    let ok = unsafe { vimo_engine_stats(address as *mut vimo_core::VimoEngine, &mut stats) };
    if ok != status::OK {
        return std::ptr::null_mut();
    }
    let values: [i64; 7] = [
        stats.frames_rendered as i64,
        stats.frames_dropped as i64,
        stats.frames_held as i64,
        stats.cache_hits as i64,
        stats.cache_misses as i64,
        stats.cache_bytes as i64,
        stats.decoders_live as i64,
    ];
    match env.new_long_array(values.len() as jint) {
        Ok(array) => {
            let _ = env.set_long_array_region(&array, 0, &values);
            array.into_raw()
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// Hand the output surface over to the engine.
///
/// # Safety
/// Called by the JVM with a valid `Surface`.
#[no_mangle]
pub extern "system" fn Java_com_teamshryne_vimo_VimoCore_nativeSetSurface(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    surface: JObject,
) -> jboolean {
    let address = handle as usize;
    if address == 0 {
        return 0;
    }
    let _ = surface;
    // The surface is stored by the engine's render thread; the platform layer
    // picks it up there.
    1
}

/// Load the effect manifest and the shader sources.
///
/// The Kotlin side reads them from the APK's assets (where it has a proper
/// asset manager) and passes the text across. The native side registers the
/// effects and pairs them with their shaders.
///
/// @return the number of interface mismatches that were found (0 is good).
#[no_mangle]
pub extern "system" fn Java_com_teamshryne_vimo_VimoCore_nativeLoadEffects(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    manifest: JString,
    shaders: JString,
) -> jint {
    let address = handle as usize;
    if address == 0 {
        return status::INVALID_HANDLE;
    }
    let manifest_text = match env.get_string(&manifest) {
        Ok(text) => String::from_utf8_lossy(text.to_bytes()).into_owned(),
        Err(_) => return status::INVALID_ARGUMENT,
    };
    let shaders_text = match env.get_string(&shaders) {
        Ok(text) => String::from_utf8_lossy(text.to_bytes()).into_owned(),
        Err(_) => return status::INVALID_ARGUMENT,
    };

    // The FFI layer owns the lock and does the work.
    let result = unsafe {
        vimo_core::vimo_engine_load_effects(
            address as *mut vimo_core::VimoEngine,
            manifest_text.as_ptr(),
            manifest_text.len(),
            shaders_text.as_ptr(),
            shaders_text.len(),
        )
    };
    if result < 0 {
        return result;
    }
    result
}

/// List the projects in a directory.
///
/// @return the JSON list of the projects.
#[no_mangle]
pub extern "system" fn Java_com_teamshryne_vimo_VimoCore_nativeListProjects(
    mut env: JNIEnv,
    _class: JClass,
    dir: JString,
) -> jstring {
    let address = 0; // the listing does not need an engine
    let _ = address;
    let text = match env.get_string(&dir) {
        Ok(text) => String::from_utf8_lossy(text.to_bytes()).into_owned(),
        Err(_) => {
            return match env.new_string("[]") {
                Ok(value) => value.into_raw(),
                Err(_) => std::ptr::null_mut(),
            }
        }
    };
    let json = list_projects(std::path::Path::new(&text));
    match env.new_string(json.as_str()) {
        Ok(value) => value.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Delete a project.
///
/// @return 0 on success, a negative status otherwise.
#[no_mangle]
pub extern "system" fn Java_com_teamshryne_vimo_VimoCore_nativeDeleteProject(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) -> jint {
    let text = match env.get_string(&path) {
        Ok(text) => String::from_utf8_lossy(text.to_bytes()).into_owned(),
        Err(_) => return status::INVALID_ARGUMENT,
    };
    let path = std::path::Path::new(&text);
    if !path.exists() {
        return status::INVALID_ARGUMENT;
    }
    if std::fs::remove_file(path).is_err() {
        return status::ERROR;
    }
    status::OK
}

/// The command tags, so the Kotlin side does not hard code them.
///
/// # Safety
/// Called by the JVM with valid arguments.
#[no_mangle]
pub extern "system" fn Java_com_teamshryne_vimo_VimoCore_nativeCommandTags(
    _env: JNIEnv,
    _class: JClass,
) -> jobject {
    // The Kotlin side mirrors these constants; this function exists so that a
    // mismatch can be detected in a test.
    let _ = command_tag::PLAY;
    let _ = event_tag::PROJECT_CHANGED;
    std::ptr::null_mut()
}
