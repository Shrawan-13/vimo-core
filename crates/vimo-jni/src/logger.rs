//! The Android logger.
//!
//! The `log` crate does nothing on its own: it needs a sink. This one
//! writes to the Android log, so that the messages of the core show up in
//! `adb logcat` with the tag `Vimo`.
//!
//! The logger is installed once, when the library is loaded, and it never
//! blocks: the Android log is written to from the thread that logs.

use std::ffi::CString;
use std::os::raw::c_char;
use std::sync::Once;

use log::{Level, LevelFilter, Log, Metadata, Record};

/// The priorities of the Android log.
#[repr(i32)]
enum Priority {
    Verbose = 2,
    Debug = 3,
    Info = 4,
    Warn = 5,
    Error = 6,
}

extern "C" {
    fn __android_log_write(prio: i32, tag: *const c_char, text: *const c_char) -> i32;
}

/// The sink that writes to the Android log.
struct AndroidLogger;

impl Log for AndroidLogger {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &Record<'_>) {
        let priority = match record.level() {
            Level::Error => Priority::Error,
            Level::Warn => Priority::Warn,
            Level::Info => Priority::Info,
            Level::Debug => Priority::Debug,
            Level::Trace => Priority::Verbose,
        };
        let text = format!("{}", record.args());
        // The strings live until the call returns, which is all the Android
        // log needs.
        let tag = CString::new("Vimo").unwrap_or_default();
        let message = CString::new(text).unwrap_or_default();
        // SAFETY: both strings are valid NUL terminated C strings that live
        // until the end of the call.
        unsafe {
            __android_log_write(priority as i32, tag.as_ptr(), message.as_ptr());
        }
    }

    fn flush(&self) {}
}

static INSTALL: Once = Once::new();

/// Install the logger.
///
/// It is safe to call this more than once: the logger is only installed the
/// first time.
pub fn install() {
    INSTALL.call_once(|| {
        // The level is read from the `VIMO_LOG` environment variable, which
        // is not set on Android, so the default is used.
        let level = std::env::var("VIMO_LOG")
            .ok()
            .and_then(|value| match value.to_ascii_lowercase().as_str() {
                "trace" => Some(LevelFilter::Trace),
                "debug" => Some(LevelFilter::Debug),
                "info" => Some(LevelFilter::Info),
                "warn" => Some(LevelFilter::Warn),
                "error" => Some(LevelFilter::Error),
                "off" => Some(LevelFilter::Off),
                _ => None,
            })
            .unwrap_or(LevelFilter::Info);
        let _ = log::set_boxed_logger(Box::new(AndroidLogger));
        log::set_max_level(level);
    });
}
