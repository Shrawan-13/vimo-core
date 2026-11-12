//! Stable entity identifiers.
//!
//! Ids are monotonically increasing `u64`s. They are never reused, so an id
//! captured by the UI stays valid (and detectable as stale) even after the
//! underlying entity is deleted.

/// A stable, never-reused entity identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct EntityId(u64);

impl EntityId {
    /// The invalid / placeholder id.
    pub const INVALID: EntityId = EntityId(0);

    /// Construct from a raw value.
    #[must_use]
    pub const fn from_raw(v: u64) -> Self {
        EntityId(v)
    }

    /// The raw value.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Whether this is the placeholder id.
    #[must_use]
    pub const fn is_invalid(self) -> bool {
        self.0 == 0
    }
}

/// Hands out monotonically increasing [`EntityId`]s.
#[derive(Debug, Clone, Default)]
pub struct IdAllocator {
    next: u64,
}

impl IdAllocator {
    /// Create an allocator whose first issued id is `1`.
    #[must_use]
    pub fn new() -> Self {
        IdAllocator { next: 1 }
    }

    /// Issue a fresh id.
    pub fn allocate(&mut self) -> EntityId {
        let id = EntityId(self.next);
        self.next += 1;
        id
    }

    /// The id that will be issued next (for testing / debug output).
    #[must_use]
    pub fn peek(&self) -> EntityId {
        EntityId(self.next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_monotonic() {
        let mut a = IdAllocator::new();
        let x = a.allocate();
        let y = a.allocate();
        assert!(y > x);
        assert_ne!(x, y);
        assert!(!x.is_invalid());
        assert!(EntityId::INVALID.is_invalid());
    }
}
