//! Structured tracing.
//!
//! The core emits **spans** — named, timed regions — that the Android layer
//! maps onto Perfetto tracks (and desktop builds map onto `tracing`).
//!
//! The facility is designed so that it costs *nothing* when tracing is off:
//! a single relaxed atomic load decides whether anything happens at all.
//! Spans are cheap to write even when they are enabled: no allocation, no
//! formatting, no locking on the hot path.
//!
//! ```
//! use vimo_foundation::trace;
//!
//! let span = trace::span("decode_frame");
//! // ... do the work ...
//! drop(span); // the span ends here
//! ```

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Information about a span, passed to the sink.
#[derive(Debug, Clone, Copy)]
pub struct SpanInfo {
    /// The span's name.
    pub name: &'static str,
    /// A unique id, so start and end events can be paired.
    pub id: u64,
    /// The duration in nanoseconds (only meaningful at the end).
    pub duration_ns: u64,
    /// Arbitrary integer arguments attached to the span.
    pub arg0: i64,
    /// Arbitrary integer arguments attached to the span.
    pub arg1: i64,
}

/// A sink for span events.
pub trait TraceSink: Send + Sync {
    /// A span started.
    fn on_start(&self, info: &SpanInfo);
    /// A span ended.
    fn on_end(&self, info: &SpanInfo);
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

static SINK: RwLock<Option<Arc<dyn TraceSink>>> = RwLock::new(None);

/// Install a sink and enable tracing.
///
/// Passing `None` disables tracing again. The sink can be replaced at any
/// time, which is what lets the Android layer swap the Perfetto sink for a
/// recording one at runtime.
pub fn set_sink(sink: Option<Arc<dyn TraceSink>>) {
    let mut guard = match SINK.write() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let enabled = sink.is_some();
    *guard = sink;
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// A clone of the current sink, if any.
fn current_sink() -> Option<Arc<dyn TraceSink>> {
    match SINK.read() {
        Ok(g) => g.clone(),
        Err(p) => p.into_inner().clone(),
    }
}

/// Whether tracing is currently enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// A sink that discards everything.
#[derive(Debug, Default)]
pub struct NullSink;

impl TraceSink for NullSink {
    fn on_start(&self, _info: &SpanInfo) {}
    fn on_end(&self, _info: &SpanInfo) {}
}

/// A sink that records the spans in memory, for tests.
#[derive(Debug, Default)]
pub struct RecordingSink {
    spans: std::sync::Mutex<Vec<SpanInfo>>,
}

impl RecordingSink {
    /// A new recording sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The spans recorded so far.
    #[must_use]
    pub fn spans(&self) -> Vec<SpanInfo> {
        self.spans.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl TraceSink for RecordingSink {
    fn on_start(&self, _info: &SpanInfo) {}

    fn on_end(&self, info: &SpanInfo) {
        if let Ok(mut spans) = self.spans.lock() {
            spans.push(*info);
        }
    }
}

/// A timed region.
///
/// The span ends when it is dropped.
#[derive(Debug)]
pub struct Span {
    name: &'static str,
    id: u64,
    start: Instant,
    arg0: i64,
    arg1: i64,
    active: bool,
}

impl Span {
    /// Attach an integer argument.
    #[must_use]
    pub fn with_arg0(mut self, arg: i64) -> Self {
        self.arg0 = arg;
        self
    }

    /// Attach another integer argument.
    #[must_use]
    pub fn with_arg1(mut self, arg: i64) -> Self {
        self.arg1 = arg;
        self
    }

    /// The span's name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let info = SpanInfo {
            name: self.name,
            id: self.id,
            duration_ns: self.start.elapsed().as_nanos() as u64,
            arg0: self.arg0,
            arg1: self.arg1,
        };
        if let Some(sink) = current_sink() {
            sink.on_end(&info);
        }
    }
}

/// Start a span.
///
/// When tracing is disabled this is a no-op that costs one atomic load.
#[must_use]
pub fn span(name: &'static str) -> Span {
    if !is_enabled() {
        return Span {
            name,
            id: 0,
            start: Instant::now(),
            arg0: 0,
            arg1: 0,
            active: false,
        };
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let info = SpanInfo {
        name,
        id,
        duration_ns: 0,
        arg0: 0,
        arg1: 0,
    };
    if let Some(sink) = current_sink() {
        sink.on_start(&info);
    }
    Span {
        name,
        id,
        start: Instant::now(),
        arg0: 0,
        arg1: 0,
        active: true,
    }
}

/// Measure a closure inside a span.
pub fn instrument<T, F: FnOnce() -> T>(name: &'static str, f: F) -> T {
    let _span = span(name);
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the tests that swap the global sink.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn spans_are_recorded_when_enabled() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sink = Arc::new(RecordingSink::new());
        set_sink(Some(sink.clone()));
        assert!(is_enabled());
        {
            let _s = span("work").with_arg0(7);
        }
        let spans = sink.spans();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "work");
        assert_eq!(spans[0].arg0, 7);
        set_sink(None);
        assert!(!is_enabled());
    }

    #[test]
    fn spans_are_free_when_disabled() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        set_sink(None);
        let s = span("nothing");
        assert!(!s.active);
        drop(s);
    }

    #[test]
    fn instrument_measures_a_closure() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sink = Arc::new(RecordingSink::new());
        set_sink(Some(sink.clone()));
        let value = instrument("compute", || 42);
        assert_eq!(value, 42);
        assert_eq!(sink.spans().len(), 1);
        set_sink(None);
    }
}
