//! Hash map aliases tuned for Vimo.
//!
//! The core hashes only internally produced data (ids, revision counters,
//! content fingerprints), so SipHash's DoS resistance buys nothing and costs
//! measurable time in the render graph's dirty-tracking path.

use crate::hash::FxBuildHasher;
use std::collections::{HashMap, HashSet};

/// A `HashMap` using the Fx hasher.
pub type FxHashMap<K, V> = HashMap<K, V, FxBuildHasher>;

/// A `HashSet` using the Fx hasher.
pub type FxHashSet<T> = HashSet<T, FxBuildHasher>;

/// Create a [`FxHashMap`] with the given capacity.
#[must_use]
pub fn fx_map<K, V>() -> FxHashMap<K, V> {
    FxHashMap::default()
}

/// Create a [`FxHashSet`].
#[must_use]
pub fn fx_set<T>() -> FxHashSet<T> {
    FxHashSet::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_works() {
        let mut m = fx_map();
        m.insert(1u32, "a");
        assert_eq!(m.get(&1), Some(&"a"));
    }
}
