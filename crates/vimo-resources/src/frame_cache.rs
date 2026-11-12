//! The decoded-frame cache.
//!
//! The cache is the single biggest memory consumer in an editor, so it is
//! bounded by **bytes**, not by entry count, and it evicts in least-recently-
//! used order.
//!
//! Keys are `(source, frame index)`. The source is an interned `u64` so that
//! a lookup never allocates, and the frame index is derived from the
//! presentation timestamp using the source's timebase.

use vimo_foundation::map::FxHashMap;
use vimo_foundation::time::Time;

/// The key of a cached frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameKey {
    /// The interned source id.
    pub source: u64,
    /// The frame index within the source.
    pub frame: i64,
}

impl FrameKey {
    /// A key.
    #[must_use]
    pub const fn new(source: u64, frame: i64) -> Self {
        FrameKey { source, frame }
    }
}

/// A cached frame and its cost.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// The frame index (part of the key, kept for convenience).
    pub frame: i64,
    /// The number of bytes the frame occupies.
    pub bytes: usize,
    /// The last time the frame was used.
    pub last_used: u64,
    /// The presentation timestamp of the frame.
    pub pts: Time,
    /// The opaque handle to the decoded data (a texture id, a buffer index...).
    pub handle: u64,
}

/// Statistics about the cache.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Number of live entries.
    pub entries: usize,
    /// Bytes held by the cache.
    pub bytes: usize,
    /// Number of lookups that hit.
    pub hits: u64,
    /// Number of lookups that missed.
    pub misses: u64,
    /// Number of entries evicted.
    pub evictions: u64,
}

impl CacheStats {
    /// The hit rate in `0..=1`.
    #[must_use]
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }
}

/// A bounded, LRU cache of decoded frames.
#[derive(Debug)]
pub struct FrameCache {
    entries: FxHashMap<FrameKey, CacheEntry>,
    /// Least-recently-used order; the front is the coldest.
    lru: Vec<FrameKey>,
    bytes: usize,
    cap_bytes: usize,
    clock: u64,
    stats: CacheStats,
}

impl Default for FrameCache {
    fn default() -> Self {
        FrameCache::new(64 * 1024 * 1024)
    }
}

impl FrameCache {
    /// A cache holding at most `cap_bytes` bytes.
    #[must_use]
    pub fn new(cap_bytes: usize) -> Self {
        FrameCache {
            entries: FxHashMap::default(),
            lru: Vec::new(),
            bytes: 0,
            cap_bytes,
            clock: 0,
            stats: CacheStats::default(),
        }
    }

    /// The byte cap.
    #[must_use]
    pub fn cap_bytes(&self) -> usize {
        self.cap_bytes
    }

    /// Change the byte cap, trimming immediately if the cache is over it.
    pub fn set_cap_bytes(&mut self, cap: usize) {
        self.cap_bytes = cap;
        self.enforce_cap();
    }

    /// Statistics.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// The number of live entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The bytes held.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Look up a frame, marking it as recently used.
    pub fn get(&mut self, key: FrameKey) -> Option<&CacheEntry> {
        self.clock += 1;
        let clock = self.clock;
        if !self.entries.contains_key(&key) {
            self.stats.misses += 1;
            return None;
        }
        self.touch(key);
        let entry = self.entries.get_mut(&key)?;
        entry.last_used = clock;
        self.stats.hits += 1;
        Some(entry)
    }

    /// Look up a frame without changing its recency.
    #[must_use]
    pub fn peek(&self, key: FrameKey) -> Option<&CacheEntry> {
        self.entries.get(&key)
    }

    /// Whether a frame is cached.
    #[must_use]
    pub fn contains(&self, key: FrameKey) -> bool {
        self.entries.contains_key(&key)
    }

    /// Insert a frame, evicting the coldest entries if needed.
    pub fn insert(&mut self, key: FrameKey, bytes: usize, handle: u64, pts: Time) {
        self.clock += 1;
        let clock = self.clock;
        if let Some(old) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
            self.lru.retain(|k| *k != key);
        }
        self.entries.insert(
            key,
            CacheEntry {
                frame: key.frame,
                bytes,
                last_used: clock,
                pts,
                handle,
            },
        );
        self.lru.push(key);
        self.bytes = self.bytes.saturating_add(bytes);
        self.enforce_cap();
    }

    /// Remove a frame.
    pub fn remove(&mut self, key: FrameKey) -> Option<CacheEntry> {
        let entry = self.entries.remove(&key)?;
        self.bytes = self.bytes.saturating_sub(entry.bytes);
        self.lru.retain(|k| *k != key);
        Some(entry)
    }

    /// Remove every frame of one source.
    pub fn remove_source(&mut self, source: u64) -> usize {
        let keys: Vec<FrameKey> = self
            .entries
            .keys()
            .copied()
            .filter(|k| k.source == source)
            .collect();
        let n = keys.len();
        for k in keys {
            self.remove(k);
        }
        n
    }

    /// Empty the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.bytes = 0;
    }

    /// Drop the least recently used entries until at most `target_bytes` are
    /// held.
    ///
    /// Returns the number of entries removed.
    pub fn trim_to(&mut self, target_bytes: usize) -> usize {
        let mut removed = 0;
        while self.bytes > target_bytes && !self.lru.is_empty() {
            let key = self.lru.remove(0);
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
                removed += 1;
                self.stats.evictions += 1;
            }
        }
        removed
    }

    /// Drop the coldest half of the cache.
    pub fn trim_half(&mut self) -> usize {
        let target = self.bytes / 2;
        self.trim_to(target)
    }

    /// The keys ordered from coldest to hottest.
    #[must_use]
    pub fn lru_order(&self) -> &[FrameKey] {
        &self.lru
    }

    fn enforce_cap(&mut self) {
        while self.bytes > self.cap_bytes && !self.lru.is_empty() {
            let key = self.lru.remove(0);
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
                self.stats.evictions += 1;
            }
        }
    }

    /// Move a key to the warm end of the LRU list.
    fn touch(&mut self, key: FrameKey) {
        if let Some(pos) = self.lru.iter().position(|k| *k == key) {
            let k = self.lru.remove(pos);
            self.lru.push(k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eviction_is_least_recently_used() {
        let mut c = FrameCache::new(100);
        c.insert(FrameKey::new(1, 0), 50, 1, Time::ZERO);
        c.insert(FrameKey::new(1, 1), 50, 2, Time::ZERO);
        assert_eq!(c.len(), 2);
        // Touch the first entry so the second becomes the coldest.
        assert!(c.get(FrameKey::new(1, 0)).is_some());
        c.insert(FrameKey::new(1, 2), 10, 3, Time::ZERO);
        assert!(!c.contains(FrameKey::new(1, 1)));
        assert!(c.contains(FrameKey::new(1, 0)));
    }

    #[test]
    fn the_cache_never_exceeds_its_cap() {
        let mut c = FrameCache::new(100);
        for i in 0..20 {
            c.insert(FrameKey::new(1, i), 30, i as u64, Time::ZERO);
        }
        assert!(c.bytes() <= 100, "bytes={}", c.bytes());
    }

    #[test]
    fn stats_track_hits_and_misses() {
        let mut c = FrameCache::new(1000);
        c.insert(FrameKey::new(1, 0), 10, 1, Time::ZERO);
        c.get(FrameKey::new(1, 0));
        c.get(FrameKey::new(1, 5));
        let s = c.stats();
        assert_eq!(s.hits, 1);
        assert_eq!(s.misses, 1);
        assert!((s.hit_rate() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn trimming_removes_the_coldest_first() {
        let mut c = FrameCache::new(1000);
        for i in 0..4 {
            c.insert(FrameKey::new(1, i), 100, i as u64, Time::ZERO);
        }
        let removed = c.trim_to(200);
        assert_eq!(removed, 2);
        assert_eq!(c.len(), 2);
    }
}
