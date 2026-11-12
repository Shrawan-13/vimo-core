//! Closed error taxonomy for Vimo Core.
//!
//! The core never panics on bad user data, bad media, or platform failures. Every
//! fallible operation returns [`VimoResult`]. Errors are small, `Copy`-ish and
//! carry a `kind` discriminant (cheap to match) plus an optional static message.

use core::fmt;

/// Machine-readable classification of a failure.
///
/// The FFI layer maps kinds onto stable numeric codes so that the Kotlin/Java
/// side never has to parse strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The requested entity does not exist (or was already removed).
    NotFound,
    /// The supplied argument was structurally invalid.
    InvalidArgument,
    /// The operation is valid but the current state forbids it.
    InvalidState,
    /// Underlying media container / codec reported a failure.
    Decode,
    /// Encoding or muxing failure.
    Encode,
    /// A platform surface, EGL context or GL object failed.
    Graphics,
    /// A decoder/encoder was not available on this device.
    Unsupported,
    /// Out of memory, or a configured memory cap was exceeded.
    OutOfMemory,
    /// Underlying I/O failed (file, socket, file descriptor).
    Io,
    /// Operation exceeded its deadline.
    Timeout,
    /// The operation was cancelled by the caller.
    Cancelled,
    /// Data did not match the expected format/version.
    Format,
    /// A resource was exhausted (pool empty, HW decoder limit reached).
    Exhausted,
    /// Filesystem / version-control style conflict.
    Conflict,
}

impl ErrorKind {
    /// Stable numeric code used by the FFI boundary. Never renumber existing kinds.
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            ErrorKind::NotFound => 1,
            ErrorKind::InvalidArgument => 2,
            ErrorKind::InvalidState => 3,
            ErrorKind::Decode => 4,
            ErrorKind::Encode => 5,
            ErrorKind::Graphics => 6,
            ErrorKind::Unsupported => 7,
            ErrorKind::OutOfMemory => 8,
            ErrorKind::Io => 9,
            ErrorKind::Timeout => 10,
            ErrorKind::Cancelled => 11,
            ErrorKind::Format => 12,
            ErrorKind::Exhausted => 13,
            ErrorKind::Conflict => 14,
        }
    }

    /// Human readable, allocation-free description.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "not found",
            ErrorKind::InvalidArgument => "invalid argument",
            ErrorKind::InvalidState => "invalid state",
            ErrorKind::Decode => "decode failure",
            ErrorKind::Encode => "encode failure",
            ErrorKind::Graphics => "graphics failure",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::OutOfMemory => "out of memory",
            ErrorKind::Io => "io failure",
            ErrorKind::Timeout => "timed out",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::Format => "malformed data",
            ErrorKind::Exhausted => "resource exhausted",
            ErrorKind::Conflict => "conflict",
        }
    }
}

/// A Vimo Core error: a [`ErrorKind`] plus optional context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VimoError {
    kind: ErrorKind,
    context: Option<&'static str>,
    detail: Option<String>,
}

impl VimoError {
    /// Build an error with only a kind.
    #[must_use]
    pub const fn new(kind: ErrorKind) -> Self {
        Self { kind, context: None, detail: None }
    }

    /// Build an error with a static context string (no allocation).
    #[must_use]
    pub const fn ctx(kind: ErrorKind, context: &'static str) -> Self {
        Self { kind, context: Some(context), detail: None }
    }

    /// Build an error with a kind, static context and an owned detail message.
    #[must_use]
    pub fn detailed(kind: ErrorKind, context: &'static str, detail: impl Into<String>) -> Self {
        Self { kind, context: Some(context), detail: Some(detail.into()) }
    }

    /// The error classification.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Optional static context, e.g. `"decode frame"`.
    #[must_use]
    pub const fn context(&self) -> Option<&'static str> {
        self.context
    }

    /// Optional owned detail message.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// Prepend a static context frame, preserving the kind and detail.
    #[must_use]
    pub fn with_context(mut self, context: &'static str) -> Self {
        self.context = Some(match self.context {
            Some(prev) => prev, // keep the innermost context; it is more specific
            None => context,
        });
        self
    }
}

impl fmt::Display for VimoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind.as_str())?;
        if let Some(ctx) = self.context {
            write!(f, " [{ctx}")?;
            if let Some(detail) = &self.detail {
                write!(f, ": {detail}")?;
            }
            write!(f, "]")?;
        } else if let Some(detail) = &self.detail {
            write!(f, ": {detail}")?;
        }
        Ok(())
    }
}

impl std::error::Error for VimoError {}

impl From<std::io::Error> for VimoError {
    fn from(e: std::io::Error) -> Self {
        let kind = match e.kind() {
            std::io::ErrorKind::NotFound => ErrorKind::NotFound,
            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => ErrorKind::InvalidArgument,
            std::io::ErrorKind::UnexpectedEof => ErrorKind::Format,
            std::io::ErrorKind::TimedOut => ErrorKind::Timeout,
            std::io::ErrorKind::Interrupted => ErrorKind::Cancelled,
            _ => ErrorKind::Io,
        };
        VimoError::detailed(kind, "io", e.to_string())
    }
}

/// Convenience alias used throughout the core.
pub type VimoResult<T> = Result<T, VimoError>;

/// Construct a [`VimoError`] with [`ErrorKind::InvalidArgument`].
#[must_use]
pub fn invalid_argument(context: &'static str) -> VimoError {
    VimoError::ctx(ErrorKind::InvalidArgument, context)
}

/// Construct a [`VimoError`] with [`ErrorKind::NotFound`].
#[must_use]
pub fn not_found(context: &'static str) -> VimoError {
    VimoError::ctx(ErrorKind::NotFound, context)
}

/// Construct a [`VimoError`] with [`ErrorKind::InvalidState`].
#[must_use]
pub fn invalid_state(context: &'static str) -> VimoError {
    VimoError::ctx(ErrorKind::InvalidState, context)
}

/// Construct a [`VimoError`] with [`ErrorKind::Unsupported`].
#[must_use]
pub fn unsupported(context: &'static str) -> VimoError {
    VimoError::ctx(ErrorKind::Unsupported, context)
}

/// Construct a [`VimoError`] with [`ErrorKind::Format`].
#[must_use]
pub fn malformed(context: &'static str) -> VimoError {
    VimoError::ctx(ErrorKind::Format, context)
}

/// Construct a [`VimoError`] with [`ErrorKind::Exhausted`].
#[must_use]
pub fn exhausted(context: &'static str) -> VimoError {
    VimoError::ctx(ErrorKind::Exhausted, context)
}

/// Construct a [`VimoError`] with [`ErrorKind::Cancelled`].
#[must_use]
pub fn cancelled(context: &'static str) -> VimoError {
    VimoError::ctx(ErrorKind::Cancelled, context)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_stable() {
        let kinds = [
            ErrorKind::NotFound,
            ErrorKind::InvalidArgument,
            ErrorKind::InvalidState,
            ErrorKind::Decode,
            ErrorKind::Encode,
            ErrorKind::Graphics,
            ErrorKind::Unsupported,
            ErrorKind::OutOfMemory,
            ErrorKind::Io,
            ErrorKind::Timeout,
            ErrorKind::Cancelled,
            ErrorKind::Format,
            ErrorKind::Exhausted,
            ErrorKind::Conflict,
        ];
        let mut seen = [false; 32];
        for k in kinds {
            let c = k.code();
            assert!(c > 0 && c < 32);
            assert!(!seen[c as usize], "duplicate code for {k:?}");
            seen[c as usize] = true;
        }
    }

    #[test]
    fn display_is_readable() {
        let e = VimoError::detailed(ErrorKind::Decode, "frame", "bad h264 slice");
        assert_eq!(e.to_string(), "decode failure [frame: bad h264 slice]");
    }
}
