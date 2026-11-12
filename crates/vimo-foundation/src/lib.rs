//! # vimo-foundation
//!
//! Dependency-free primitives shared by every Vimo Core crate.
//!
//! Design goals, in priority order:
//!
//! 1. **No allocation on the hot path.** Everything here either borrows or owns
//!    storage inline ([`container::ArrayVec`], [`container::SmallVec`]).
//! 2. **Exact arithmetic for time.** Timeline positions are stored as signed
//!    nanoseconds ([`time::Time`]) so that a 3-hour 120 fps project never drifts
//!    by a single microsecond. Frame numbers are derived, never stored.
//! 3. **Total, predictable errors.** [`error::VimoError`] is a closed enum; no
//!    `Box<dyn Error>` allocation in the hot path, no panics.
//!
//! This crate has **zero** external dependencies and forbids `unsafe`.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod color;
pub mod container;
pub mod error;
pub mod hash;
pub mod id;
pub mod math;
pub mod map;
pub mod time;
pub mod trace;

pub use error::{ErrorKind, VimoError, VimoResult};
pub use id::{EntityId, IdAllocator};
pub use math::{Mat3, Mat4, Rect, Vec2, Vec3, Vec4};
pub use time::{Rational, Time, Timebase};
