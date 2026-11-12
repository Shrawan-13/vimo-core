//! Storage containers tuned for the Vimo hot path.
//!
//! * [`SlotMap`] — a generational arena. O(1) insert/lookup/remove with stable
//!   handles and no allocation on removal. The project model lives in one.
//! * [`Arena`] — a plain index arena for transient, per-frame data.
//! * [`DenseMap`] — a `u32`→`u32` map with O(1) lookups and contiguous storage,
//!   used to translate entity ids into dense array indices in the render graph.

use core::fmt;
use core::marker::PhantomData;

/// A generational handle into a [`SlotMap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Handle<T> {
    index: u32,
    generation: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    /// Index part of the handle.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Generation part of the handle.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl<T> fmt::Display for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}v{}", self.index, self.generation)
    }
}

#[derive(Debug)]
struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// A generational arena mapping handles to values.
///
/// Removal bumps the generation counter of the freed slot, so a stale handle is
/// detected rather than silently addressing a recycled slot.
#[derive(Debug)]
pub struct SlotMap<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Default for SlotMap<T> {
    fn default() -> Self {
        SlotMap { slots: Vec::new(), free: Vec::new(), len: 0 }
    }
}

impl<T> SlotMap<T> {
    /// Create an empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an empty map with reserved capacity.
    #[must_use]
    pub fn with_capacity(cap: usize) -> Self {
        SlotMap { slots: Vec::with_capacity(cap), free: Vec::new(), len: 0 }
    }

    /// Insert a value, returning its handle.
    pub fn insert(&mut self, value: T) -> Handle<T> {
        self.len += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.value = Some(value);
            Handle { index, generation: slot.generation, _marker: PhantomData }
        } else {
            let index = u32::try_from(self.slots.len()).expect("SlotMap exceeded u32::MAX slots");
            self.slots.push(Slot { generation: 0, value: Some(value) });
            Handle { index, generation: 0, _marker: PhantomData }
        }
    }

    /// Get a value by handle.
    #[must_use]
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        let slot = self.slots.get(handle.index as usize)?;
        if slot.generation != handle.generation {
            return None;
        }
        slot.value.as_ref()
    }

    /// Get a mutable value by handle.
    #[must_use]
    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        if slot.generation != handle.generation {
            return None;
        }
        slot.value.as_mut()
    }

    /// Remove a value, invalidating its handle.
    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        if slot.generation != handle.generation {
            return None;
        }
        let value = slot.value.take();
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(handle.index);
        self.len -= 1;
        value
    }

    /// Whether the handle still refers to a live value.
    #[must_use]
    pub fn contains(&self, handle: Handle<T>) -> bool {
        self.get(handle).is_some()
    }

    /// Number of live values.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Total number of slots ever allocated (including free ones).
    #[must_use]
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Iterate over all live values.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.slots.iter().filter_map(|s| s.value.as_ref())
    }

    /// Iterate over all live values mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.slots.iter_mut().filter_map(|s| s.value.as_mut())
    }

    /// Iterate over `(handle, value)` pairs.
    pub fn entries(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.slots.iter().enumerate().filter_map(|(i, s)| {
            s.value.as_ref().map(|v| {
                (
                    Handle { index: i as u32, generation: s.generation, _marker: PhantomData },
                    v,
                )
            })
        })
    }
}

/// A non-generational arena of values addressed by `u32` index.
#[derive(Debug, Clone)]
pub struct Arena<T> {
    items: Vec<T>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena { items: Vec::new() }
    }
}

impl<T> Arena<T> {
    /// Create an empty arena.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a value, returning its index.
    pub fn push(&mut self, value: T) -> u32 {
        let idx = u32::try_from(self.items.len()).expect("Arena exceeded u32::MAX items");
        self.items.push(value);
        idx
    }

    /// Get by index.
    #[must_use]
    pub fn get(&self, index: u32) -> Option<&T> {
        self.items.get(index as usize)
    }

    /// Get mutably by index.
    #[must_use]
    pub fn get_mut(&mut self, index: u32) -> Option<&mut T> {
        self.items.get_mut(index as usize)
    }

    /// Remove by index, replacing with `Default` to keep indices dense.
    pub fn remove(&mut self, index: u32) -> Option<T>
    where
        T: Default,
    {
        let i = index as usize;
        if i >= self.items.len() {
            return None;
        }
        Some(core::mem::take(&mut self.items[i]))
    }

    /// Number of items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the arena is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Iterate over items.
    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.items.iter()
    }

    /// Iterate mutably.
    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, T> {
        self.items.iter_mut()
    }

    /// Access the underlying slice.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.items
    }
}

/// A dense `u32` → `u32` mapping with contiguous storage.
///
/// Insertion order is preserved so that render order matches project order
/// without a separate sort.
#[derive(Debug, Clone, Default)]
pub struct DenseMap {
    keys: Vec<u32>,
    values: Vec<u32>,
    lookup: Vec<u32>,
}

const DENSE_EMPTY: u32 = u32::MAX;

impl DenseMap {
    /// Create an empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_capacity(&mut self, additional: usize) {
        let needed = self.keys.len() + additional;
        let mut cap = if self.lookup.is_empty() { 64 } else { self.lookup.len() };
        // Keep the load factor below 0.7 so probe chains stay short.
        while cap * 7 < needed * 10 {
            cap *= 2;
        }
        if cap > self.lookup.len() {
            self.rehash(cap);
        }
    }

    fn rehash(&mut self, cap: usize) {
        let cap = cap.next_power_of_two().max(64);
        self.lookup.clear();
        self.lookup.resize(cap, DENSE_EMPTY);
        for (i, key) in self.keys.iter().enumerate() {
            let mut slot = (*key as usize) & (cap - 1);
            while self.lookup[slot] != DENSE_EMPTY {
                slot = (slot + 1) & (cap - 1);
            }
            self.lookup[slot] = i as u32;
        }
    }

    /// Insert or update a key.
    pub fn insert(&mut self, key: u32, value: u32) {
        self.ensure_capacity(1);
        let mut slot = (key as usize) & (self.lookup.len() - 1);
        loop {
            let entry = self.lookup[slot];
            if entry == DENSE_EMPTY {
                self.keys.push(key);
                self.values.push(value);
                self.lookup[slot] = (self.keys.len() - 1) as u32;
                return;
            }
            if self.keys[entry as usize] == key {
                self.values[entry as usize] = value;
                return;
            }
            slot = (slot + 1) & (self.lookup.len() - 1);
        }
    }

    /// Look up a key.
    #[must_use]
    pub fn get(&self, key: u32) -> Option<u32> {
        if self.lookup.is_empty() {
            return None;
        }
        let mut slot = (key as usize) & (self.lookup.len() - 1);
        loop {
            let entry = self.lookup[slot];
            if entry == DENSE_EMPTY {
                return None;
            }
            if self.keys[entry as usize] == key {
                return Some(self.values[entry as usize]);
            }
            slot = (slot + 1) & (self.lookup.len() - 1);
        }
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Iterate over `(key, value)` pairs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.keys.iter().copied().zip(self.values.iter().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_map_handles_are_generational() {
        let mut m: SlotMap<u32> = SlotMap::new();
        let h = m.insert(7);
        assert_eq!(m.get(h), Some(&7));
        assert_eq!(m.remove(h), Some(7));
        assert_eq!(m.get(h), None);
        let h2 = m.insert(9);
        assert_eq!(
            h2.index(),
            h.index(),
            "the freed slot is reused to keep memory compact"
        );
        assert_ne!(h2.generation(), h.generation(), "generation must advance");
        assert_eq!(m.get(h2), Some(&9));
        assert_eq!(m.get(h), None, "the stale handle must not resolve");
    }

    #[test]
    fn slot_map_len_tracks_live_values() {
        let mut m: SlotMap<u32> = SlotMap::new();
        let a = m.insert(1);
        let b = m.insert(2);
        assert_eq!(m.len(), 2);
        m.remove(a);
        assert_eq!(m.len(), 1);
        assert!(m.contains(b));
    }

    #[test]
    fn dense_map_roundtrip() {
        let mut m = DenseMap::new();
        for i in 0..500u32 {
            m.insert(i * 7, i);
        }
        for i in 0..500u32 {
            assert_eq!(m.get(i * 7), Some(i));
        }
        assert_eq!(m.get(1), None);
        assert_eq!(m.len(), 500);
    }

    #[test]
    fn arena_remove_keeps_indices_dense() {
        let mut a: Arena<u32> = Arena::new();
        let i0 = a.push(10);
        let i1 = a.push(20);
        assert_eq!(a.remove(i0), Some(10));
        assert_eq!(a.get(i1), Some(&20));
    }
}
