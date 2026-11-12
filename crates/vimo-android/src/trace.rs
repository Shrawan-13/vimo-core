//! The Perfetto trace sink.
//!
//! The core emits named spans; the Android layer turns them into Perfetto
//! track events. The implementation uses the `ATrace` C API, which the
//! platform's tracing infrastructure understands.

use std::ffi::CString;

use vimo_foundation::trace::{SpanInfo, TraceSink};

extern "C" {
    /// `ATrace_beginSection` from `<android/trace.h>`.
    fn ATrace_beginSection(name: *const std::os::raw::c_char);
    /// `ATrace_endSection` from `<android/trace.h>`.
    fn ATrace_endSection();
    /// `ATrace_isEnabled` from `<android/trace.h>`.
    fn ATrace_isEnabled() -> bool;
}

/// A sink that writes the spans to the Android trace.
#[derive(Debug, Default)]
pub struct PerfettoSink;

impl PerfettoSink {
    /// A new sink.
    #[must_use]
    pub fn new() -> Self {
        PerfettoSink
    }

    /// Whether tracing is currently enabled on the platform.
    #[must_use]
    pub fn is_enabled() -> bool {
        unsafe { ATrace_isEnabled() }
    }
}

impl TraceSink for PerfettoSink {
    fn on_start(&self, info: &SpanInfo) {
        // ATrace sections are strictly nested, so the spans are emitted as
        // begin/end pairs. The name is cached by the platform, so the cost is
        // small.
        if let Ok(name) = CString::new(info.name) {
            unsafe { ATrace_beginSection(name.as_ptr()) };
        }
    }

    fn on_end(&self, _info: &SpanInfo) {
        unsafe { ATrace_endSection() };
    }
}

/// A sink that writes the spans to `logcat`.
///
/// Useful in debug builds; it is far too slow for release.
#[derive(Debug, Default)]
pub struct LogcatSink;

impl TraceSink for LogcatSink {
    fn on_start(&self, info: &SpanInfo) {
        log::trace!("span start {} #{}", info.name, info.id);
    }

    fn on_end(&self, info: &SpanInfo) {
        log::trace!("span end {} #{} ({} ns)", info.name, info.id, info.duration_ns);
    }
}
