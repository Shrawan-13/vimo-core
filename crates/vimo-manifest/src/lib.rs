//! # vimo-manifest
//!
//! Parsing and validation of **effect manifests** — the JSON documents that
//! describe which effects exist, what parameters they take and what those
//! parameters default to.
//!
//! ```
//! use vimo_manifest::Manifest;
//!
//! let m = Manifest::parse_str(r#"{
//!   "effects": [{
//!     "id": "com.vimo.effect.gain",
//!     "params": [{"name": "gain", "type": "float", "default": 1.0}]
//!   }]
//! }"#).unwrap();
//!
//! let registry = m.into_registry().unwrap();
//! let id = registry.lookup("com.vimo.effect.gain").unwrap();
//! assert_eq!(registry.get(id).unwrap().param_count(), 1);
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod effect;
pub mod param;
pub mod parse;

pub use effect::{EffectCapabilities, EffectDesc, EffectRegistry, EffectTypeId, ParamStore};
pub use param::{ParamDesc, ParamType, ParamValue};
pub use parse::{parse_hex_color, Manifest, MAX_EFFECTS, MAX_PARAMS_PER_EFFECT};
