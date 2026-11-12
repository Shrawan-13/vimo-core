//! Effect descriptors and the interning registry.

use std::collections::HashMap;

use crate::param::{ParamDesc, ParamType, ParamValue};
use vimo_foundation::error::{malformed, VimoError, VimoResult};

/// An interned handle to an effect type inside an [`EffectRegistry`].
///
/// Interning turns the string effect id (which appears once per effect
/// instance in a project) into a `u32` index, so the render graph can look up
/// parameter layouts with a single bounds-checked array access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct EffectTypeId(u32);

impl EffectTypeId {
    /// The raw index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }

    /// Construct from a raw index.
    #[must_use]
    pub const fn from_index(i: u32) -> Self {
        EffectTypeId(i)
    }
}

/// Execution requirements an effect declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EffectCapabilities {
    /// The effect requires a GPU shader; it cannot run on the CPU fallback path.
    pub requires_gpu: bool,
    /// The effect is safe to evaluate at reduced preview resolution.
    pub preview_scalable: bool,
    /// The effect needs the previous frame (temporal effects such as motion blur).
    pub temporal: bool,
}

/// A fully validated effect definition.
#[derive(Debug, Clone)]
pub struct EffectDesc {
    /// Globally unique string id, e.g. `com.vimo.effect.blur`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Category for the UI (`blur`, `color`, `transform`, ...).
    pub category: String,
    /// Manifest revision of this effect.
    pub version: u32,
    /// GLSL fragment shader source or asset path.
    pub shader: Option<String>,
    /// Parameter descriptors in declaration order.
    pub params: Vec<ParamDesc>,
    /// Lowercased parameter name → index, for O(1) lookup.
    pub(crate) param_index: HashMap<String, u16>,
    /// Declared capabilities.
    pub capabilities: EffectCapabilities,
}

impl EffectDesc {
    /// Build an effect descriptor, validating parameter names.
    ///
    /// The name → index map is derived here so that lookups stay O(1).
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        category: impl Into<String>,
        version: u32,
        shader: Option<String>,
        params: Vec<ParamDesc>,
        capabilities: EffectCapabilities,
    ) -> VimoResult<Self> {
        let mut param_index = HashMap::with_capacity(params.len());
        for (i, p) in params.iter().enumerate() {
            let key = p.name.to_ascii_lowercase();
            if param_index.insert(key, i as u16).is_some() {
                return Err(VimoError::detailed(
                    vimo_foundation::ErrorKind::Conflict,
                    "effect descriptor",
                    format!("duplicate param `{}`", p.name),
                ));
            }
        }
        Ok(EffectDesc {
            id: id.into(),
            name: name.into(),
            category: category.into(),
            version,
            shader,
            params,
            param_index,
            capabilities,
        })
    }

    /// Number of declared parameters.
    #[must_use]
    pub fn param_count(&self) -> usize {
        self.params.len()
    }

    /// Look up a parameter index by name (case-insensitive).
    #[must_use]
    pub fn param_index(&self, name: &str) -> Option<u16> {
        self.param_index.get(&name.to_ascii_lowercase()).copied()
    }

    /// Get a parameter descriptor by index.
    #[must_use]
    pub fn param(&self, index: u16) -> Option<&ParamDesc> {
        self.params.get(index as usize)
    }

    /// The default value of a parameter.
    #[must_use]
    pub fn default_value(&self, index: u16) -> ParamValue {
        self.params
            .get(index as usize)
            .map(|p| p.default)
            .unwrap_or(ParamValue::Float(0.0))
    }

    /// Build the parameter value store for a new instance of this effect.
    #[must_use]
    pub fn new_param_store(&self) -> ParamStore {
        let values: Vec<ParamValue> = self.params.iter().map(|p| p.default).collect();
        ParamStore::from_values(values)
    }
}

/// A registry of effect definitions, indexed by [`EffectTypeId`].
#[derive(Debug, Clone, Default)]
pub struct EffectRegistry {
    effects: Vec<EffectDesc>,
    by_id: HashMap<String, u32>,
}

impl EffectRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an effect, returning its interned id.
    ///
    /// Fails if the id is already registered.
    pub fn register(&mut self, desc: EffectDesc) -> VimoResult<EffectTypeId> {
        let key = desc.id.to_ascii_lowercase();
        if self.by_id.contains_key(&key) {
            return Err(VimoError::detailed(
                vimo_foundation::ErrorKind::Conflict,
                "register effect",
                format!("duplicate effect id `{}`", desc.id),
            ));
        }
        let id = EffectTypeId(u32::try_from(self.effects.len()).map_err(|_| {
            malformed("register effect: too many effect types")
        })?);
        self.by_id.insert(key, id.0);
        self.effects.push(desc);
        Ok(id)
    }

    /// Look up an effect type by its string id (case-insensitive).
    #[must_use]
    pub fn lookup(&self, id: &str) -> Option<EffectTypeId> {
        self.by_id.get(&id.to_ascii_lowercase()).copied().map(EffectTypeId)
    }

    /// Get an effect descriptor by interned id.
    #[must_use]
    pub fn get(&self, id: EffectTypeId) -> Option<&EffectDesc> {
        self.effects.get(id.0 as usize)
    }

    /// Number of registered effects.
    #[must_use]
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// Iterate over `(id, descriptor)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (EffectTypeId, &EffectDesc)> {
        self.effects
            .iter()
            .enumerate()
            .map(|(i, e)| (EffectTypeId(i as u32), e))
    }

    /// Find the index of an animatable parameter by name.
    #[must_use]
    pub fn animatable_param(&self, effect: EffectTypeId, name: &str) -> Option<u16> {
        let desc = self.get(effect)?;
        let idx = desc.param_index(name)?;
        let p = desc.param(idx)?;
        if p.ptype.is_animatable() {
            Some(idx)
        } else {
            None
        }
    }
}

/// Per-instance parameter values for one effect.
///
/// Values are stored densely (one slot per declared parameter) so that
/// evaluating a parameter in the render loop is a plain array read with no
/// hashing and no allocation. The *override* bitset records which values differ
/// from the manifest default, which keeps project files small and makes
/// "reset to default" trivial.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamStore {
    values: Box<[ParamValue]>,
    overridden: Box<[u64]>,
}

impl ParamStore {
    /// Build a store from a dense value list (no overrides).
    #[must_use]
    pub fn from_values(values: Vec<ParamValue>) -> Self {
        let words = values.len().div_ceil(64).max(1);
        ParamStore { values: values.into_boxed_slice(), overridden: vec![0u64; words].into_boxed_slice() }
    }

    /// Number of parameter slots.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the store has no slots.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Read a parameter value.
    #[must_use]
    pub fn get(&self, index: u16) -> Option<ParamValue> {
        self.values.get(index as usize).copied()
    }

    /// Write a parameter value.
    pub fn set(&mut self, index: u16, value: ParamValue) -> bool {
        let Some(slot) = self.values.get_mut(index as usize) else {
            return false;
        };
        *slot = value;
        let word = index as usize / 64;
        let bit = index as u64 % 64;
        if let Some(w) = self.overridden.get_mut(word) {
            *w |= 1u64 << bit;
        }
        true
    }

    /// Whether a parameter has been explicitly overridden.
    #[must_use]
    pub fn is_overridden(&self, index: u16) -> bool {
        let word = index as usize / 64;
        let bit = index as u64 % 64;
        self.overridden.get(word).is_some_and(|w| (w >> bit) & 1 == 1)
    }

    /// Clear the override flag for a parameter (the value stays as-is).
    pub fn clear_override(&mut self, index: u16) {
        let word = index as usize / 64;
        let bit = index as u64 % 64;
        if let Some(w) = self.overridden.get_mut(word) {
            *w &= !(1u64 << bit);
        }
    }

    /// Iterate over overridden `(index, value)` pairs, in index order.
    pub fn iter_overridden(&self) -> impl Iterator<Item = (u16, ParamValue)> + '_ {
        let values = &self.values;
        self.overridden
            .iter()
            .enumerate()
            .flat_map(move |(w, &bits)| {
                let mut bit = 0usize;
                std::iter::from_fn(move || {
                    if bit >= 64 {
                        return None;
                    }
                    let cur = bit;
                    bit += 1;
                    if (bits >> cur) & 1 == 1 {
                        Some((w, cur))
                    } else {
                        None
                    }
                })
            })
            .filter_map(move |(w, b)| {
                let idx = (w * 64 + b) as u16;
                values.get(idx as usize).map(|v| (idx, *v))
            })
    }

    /// Reset a parameter to the manifest default.
    pub fn reset(&mut self, desc: &EffectDesc, index: u16) {
        self.set(index, desc.default_value(index));
        self.clear_override(index);
    }
}

/// Helper used by the parser to build a `ParamDesc` with a numeric range.
#[must_use]
pub fn make_float_desc(
    name: &str,
    default: f32,
    min: Option<f32>,
    max: Option<f32>,
    step: Option<f32>,
) -> ParamDesc {
    ParamDesc {
        name: name.to_ascii_lowercase(),
        ptype: ParamType::Float,
        default: ParamValue::Float(default),
        min,
        max,
        step,
        label: name.to_ascii_lowercase(),
        animatable: true,
        options: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param::ParamDesc;

    fn desc(id: &str) -> EffectDesc {
        EffectDesc {
            id: id.into(),
            name: "Test".into(),
            category: "test".into(),
            version: 1,
            shader: None,
            params: vec![ParamDesc {
                name: "radius".into(),
                ptype: ParamType::Float,
                default: ParamValue::Float(1.0),
                min: None,
                max: None,
                step: None,
                label: "Radius".into(),
                animatable: true,
                options: Vec::new(),
            }],
            param_index: HashMap::from([("radius".to_string(), 0u16)]),
            capabilities: EffectCapabilities::default(),
        }
    }

    #[test]
    fn registry_interns_ids() {
        let mut r = EffectRegistry::new();
        let a = r.register(desc("com.vimo.test")).unwrap();
        let b = r.register(desc("com.vimo.other")).unwrap();
        assert_ne!(a, b);
        assert_eq!(r.lookup("COM.VIMO.TEST"), Some(a));
        assert!(r.register(desc("com.vimo.test")).is_err());
    }

    #[test]
    fn param_store_tracks_overrides() {
        let d = desc("x");
        let mut s = d.new_param_store();
        assert!(!s.is_overridden(0));
        s.set(0, ParamValue::Float(5.0));
        assert!(s.is_overridden(0));
        assert_eq!(s.get(0), Some(ParamValue::Float(5.0)));
        s.reset(&d, 0);
        assert!(!s.is_overridden(0));
        assert_eq!(s.get(0), Some(ParamValue::Float(1.0)));
    }
}
