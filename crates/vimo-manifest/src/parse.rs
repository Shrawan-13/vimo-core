//! Manifest parsing.
//!
//! The parser is deliberately strict about *semantics* (duplicate ids, unknown
//! types, out-of-range enum defaults) and lenient about *syntax* (unknown
//! fields are ignored so that a newer manifest still loads on an older app).
//!
//! It never panics: every failure is reported as a [`VimoError`] that names the
//! offending effect and parameter.

use std::io::Read;

use serde::Deserialize;
use serde_json::Value;

use crate::effect::{EffectCapabilities, EffectDesc, EffectRegistry};
use crate::param::{ParamDesc, ParamType, ParamValue};
use vimo_foundation::color::Rgba8;
use vimo_foundation::error::{malformed, ErrorKind, VimoError, VimoResult};
use vimo_foundation::math::{Vec2, Vec3};

/// Upper bound on the number of effects in one manifest.
pub const MAX_EFFECTS: usize = 4096;
/// Upper bound on the number of parameters per effect.
pub const MAX_PARAMS_PER_EFFECT: usize = 256;

#[derive(Debug, Deserialize)]
struct RawManifest {
    #[serde(default)]
    manifest_version: u32,
    #[serde(default)]
    effects: Vec<RawEffect>,
}

#[derive(Debug, Deserialize)]
struct RawEffect {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default = "one")]
    version: u32,
    #[serde(default)]
    shader: Option<String>,
    #[serde(default)]
    requires_gpu: bool,
    #[serde(default = "yes")]
    preview_scalable: bool,
    #[serde(default)]
    temporal: bool,
    #[serde(default)]
    params: Vec<RawParam>,
}

#[derive(Debug, Deserialize)]
struct RawParam {
    name: String,
    #[serde(rename = "type")]
    ptype: String,
    #[serde(default)]
    default: Option<Value>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    step: Option<f64>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    options: Option<Vec<String>>,
    #[serde(default)]
    animatable: Option<bool>,
}

fn one() -> u32 {
    1
}
fn yes() -> bool {
    true
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// A parsed manifest file.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    /// Manifest schema version declared by the file.
    pub manifest_version: u32,
    /// Effect definitions in file order.
    pub effects: Vec<EffectDesc>,
}

impl Manifest {
    /// Parse a manifest from a JSON string.
    pub fn parse_str(json: &str) -> VimoResult<Self> {
        let value: Value = serde_json::from_str(json).map_err(|e| {
            VimoError::detailed(ErrorKind::Format, "manifest", format!("invalid JSON: {e}"))
        })?;
        Self::from_value(value)
    }

    /// Parse a manifest from JSON bytes.
    pub fn parse_bytes(bytes: &[u8]) -> VimoResult<Self> {
        let value: Value = serde_json::from_slice(bytes).map_err(|e| {
            VimoError::detailed(ErrorKind::Format, "manifest", format!("invalid JSON: {e}"))
        })?;
        Self::from_value(value)
    }

    /// Parse a manifest from any reader.
    pub fn parse_reader<R: Read>(mut reader: R) -> VimoResult<Self> {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).map_err(|e| {
            VimoError::detailed(ErrorKind::Io, "manifest", format!("read failed: {e}"))
        })?;
        Self::parse_bytes(&buf)
    }

    /// Parse from an already decoded JSON value.
    ///
    /// Accepts either an object with an `"effects"` array or a bare array of
    /// effect objects.
    pub fn from_value(value: Value) -> VimoResult<Self> {
        let raw = match value {
            Value::Object(_) => serde_json::from_value::<RawManifest>(value).map_err(|e| {
                VimoError::detailed(ErrorKind::Format, "manifest", format!("invalid manifest: {e}"))
            })?,
            Value::Array(items) => RawManifest {
                manifest_version: 1,
                effects: items
                    .into_iter()
                    .map(|item| {
                        serde_json::from_value::<RawEffect>(item).map_err(|e| {
                            VimoError::detailed(
                                ErrorKind::Format,
                                "manifest",
                                format!("invalid effect entry: {e}"),
                            )
                        })
                    })
                    .collect::<VimoResult<Vec<_>>>()?,
            },
            other => {
                return Err(VimoError::detailed(
                    ErrorKind::Format,
                    "manifest",
                    format!("expected an object or array, found {}", type_name(&other)),
                ))
            }
        };
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawManifest) -> VimoResult<Self> {
        if raw.effects.len() > MAX_EFFECTS {
            return Err(VimoError::detailed(
                ErrorKind::InvalidArgument,
                "manifest",
                format!("too many effects: {} > {MAX_EFFECTS}", raw.effects.len()),
            ));
        }
        let mut effects = Vec::with_capacity(raw.effects.len());
        for eff in &raw.effects {
            effects.push(convert_effect(eff)?);
        }
        Ok(Manifest { manifest_version: raw.manifest_version, effects })
    }

    /// Build an interning registry from this manifest.
    pub fn into_registry(self) -> VimoResult<EffectRegistry> {
        let mut registry = EffectRegistry::new();
        for desc in self.effects {
            registry.register(desc)?;
        }
        Ok(registry)
    }
}

fn convert_effect(raw: &RawEffect) -> VimoResult<EffectDesc> {
    if raw.id.trim().is_empty() {
        return Err(malformed("manifest: effect id must not be empty"));
    }
    if raw.params.len() > MAX_PARAMS_PER_EFFECT {
        return Err(VimoError::detailed(
            ErrorKind::InvalidArgument,
            "manifest",
            format!("effect `{}` has too many params", raw.id),
        ));
    }

    let mut params = Vec::with_capacity(raw.params.len());
    let mut param_index = std::collections::HashMap::with_capacity(raw.params.len());

    for p in &raw.params {
        let ptype = ParamType::parse(&p.ptype).ok_or_else(|| {
            VimoError::detailed(
                ErrorKind::InvalidArgument,
                "manifest",
                format!("effect `{}`: unknown param type `{}`", raw.id, p.ptype),
            )
        })?;
        let name = p.name.trim().to_ascii_lowercase();
        if name.is_empty() {
            return Err(VimoError::detailed(
                ErrorKind::InvalidArgument,
                "manifest",
                format!("effect `{}`: param name must not be empty", raw.id),
            ));
        }
        if param_index.contains_key(&name) {
            return Err(VimoError::detailed(
                ErrorKind::Conflict,
                "manifest",
                format!("effect `{}`: duplicate param `{}`", raw.id, name),
            ));
        }

        let options = p.options.clone().unwrap_or_default();
        if ptype == ParamType::Enum && options.is_empty() {
            return Err(VimoError::detailed(
                ErrorKind::InvalidArgument,
                "manifest",
                format!("effect `{}`: enum param `{}` has no options", raw.id, name),
            ));
        }

        let default = match &p.default {
            Some(v) => convert_value(&raw.id, &name, ptype, &options, v)?,
            None => default_for_type(ptype),
        };

        let animatable = p.animatable.unwrap_or_else(|| ptype.is_animatable());

        params.push(ParamDesc {
            name: name.clone(),
            ptype,
            default,
            min: p.min.map(|v| v as f32),
            max: p.max.map(|v| v as f32),
            step: p.step.map(|v| v as f32),
            label: p.label.clone().unwrap_or_else(|| name.clone()),
            animatable,
            options,
        });
        param_index.insert(name, (params.len() - 1) as u16);
    }

    Ok(EffectDesc {
        id: raw.id.clone(),
        name: raw.name.clone().unwrap_or_else(|| raw.id.clone()),
        category: raw.category.clone().unwrap_or_else(|| "general".to_string()),
        version: raw.version,
        shader: raw.shader.clone(),
        params,
        param_index,
        capabilities: EffectCapabilities {
            requires_gpu: raw.requires_gpu,
            preview_scalable: raw.preview_scalable,
            temporal: raw.temporal,
        },
    })
}

fn default_for_type(ptype: ParamType) -> ParamValue {
    match ptype {
        ParamType::Float => ParamValue::Float(0.0),
        ParamType::Int => ParamValue::Int(0),
        ParamType::Bool => ParamValue::Bool(false),
        ParamType::Color => ParamValue::Color(Rgba8::BLACK),
        ParamType::Vec2 => ParamValue::Vec2(Vec2::ZERO),
        ParamType::Vec3 => ParamValue::Vec3(Vec3::ZERO),
        ParamType::Enum => ParamValue::Enum(0),
    }
}

fn convert_value(
    effect: &str,
    param: &str,
    ptype: ParamType,
    options: &[String],
    v: &Value,
) -> VimoResult<ParamValue> {
    let bad = |what: &str| {
        VimoError::detailed(
            ErrorKind::InvalidArgument,
            "manifest",
            format!("effect `{effect}`: param `{param}` {what}"),
        )
    };
    match ptype {
        ParamType::Float => {
            let n = v.as_f64().ok_or_else(|| bad("expects a number"))?;
            Ok(ParamValue::Float(n as f32))
        }
        ParamType::Int => {
            let n = v.as_i64().ok_or_else(|| bad("expects an integer"))?;
            Ok(ParamValue::Int(n as i32))
        }
        ParamType::Bool => {
            let b = v.as_bool().ok_or_else(|| bad("expects a boolean"))?;
            Ok(ParamValue::Bool(b))
        }
        ParamType::Color => parse_color(v).map(ParamValue::Color).ok_or_else(|| bad("expects a colour")),
        ParamType::Vec2 => {
            let arr = v.as_array().ok_or_else(|| bad("expects a 2-element array"))?;
            if arr.len() != 2 {
                return Err(bad("expects a 2-element array"));
            }
            let x = arr[0].as_f64().ok_or_else(|| bad("expects numeric components"))?;
            let y = arr[1].as_f64().ok_or_else(|| bad("expects numeric components"))?;
            Ok(ParamValue::Vec2(Vec2::new(x as f32, y as f32)))
        }
        ParamType::Vec3 => {
            let arr = v.as_array().ok_or_else(|| bad("expects a 3-element array"))?;
            if arr.len() != 3 {
                return Err(bad("expects a 3-element array"));
            }
            let mut c = [0.0f32; 3];
            for (i, item) in arr.iter().enumerate() {
                c[i] = item.as_f64().ok_or_else(|| bad("expects numeric components"))? as f32;
            }
            Ok(ParamValue::Vec3(Vec3::new(c[0], c[1], c[2])))
        }
        ParamType::Enum => {
            if let Some(idx) = v.as_u64() {
                let idx = u16::try_from(idx).map_err(|_| bad("enum index out of range"))?;
                if (idx as usize) >= options.len() {
                    return Err(bad("enum index out of range"));
                }
                return Ok(ParamValue::Enum(idx));
            }
            if let Some(s) = v.as_str() {
                let idx = options
                    .iter()
                    .position(|o| o == s)
                    .ok_or_else(|| bad("references an unknown option"))?;
                return Ok(ParamValue::Enum(idx as u16));
            }
            Err(bad("expects an enum index or option name"))
        }
    }
}

/// Parse `#rrggbb`, `#rrggbbaa`, `#rgb`, or a `[r, g, b, a]` array of 0..255.
#[must_use]
pub fn parse_color(v: &Value) -> Option<Rgba8> {
    if let Some(arr) = v.as_array() {
        if arr.len() == 3 || arr.len() == 4 {
            let mut c = [0u8, 0, 0, 255];
            for (i, item) in arr.iter().enumerate() {
                let n = item.as_f64()?;
                if !(0.0..=255.0).contains(&n) {
                    return None;
                }
                c[i] = n.round() as u8;
            }
            return Some(Rgba8::new(c[0], c[1], c[2], c[3]));
        }
        return None;
    }
    let s = v.as_str()?;
    parse_hex_color(s)
}

/// Parse `#rgb`, `#rrggbb` or `#rrggbbaa`.
#[must_use]
pub fn parse_hex_color(s: &str) -> Option<Rgba8> {
    let s = s.trim();
    let hex = s.strip_prefix('#')?;
    let nibble = |c: char| c.to_digit(16).map(|v| v as u8);
    let chars: Vec<char> = hex.chars().collect();
    match chars.len() {
        3 => {
            let mut out = [0u8; 4];
            out[3] = 255;
            for (i, c) in chars.iter().enumerate() {
                out[i] = nibble(*c)? * 17;
            }
            Some(Rgba8::new(out[0], out[1], out[2], out[3]))
        }
        6 => {
            let mut out = [0u8; 4];
            out[3] = 255;
            for i in 0..3 {
                out[i] = nibble(chars[i * 2])? * 16 + nibble(chars[i * 2 + 1])?;
            }
            Some(Rgba8::new(out[0], out[1], out[2], out[3]))
        }
        8 => {
            let mut out = [0u8; 4];
            for i in 0..4 {
                out[i] = nibble(chars[i * 2])? * 16 + nibble(chars[i * 2 + 1])?;
            }
            Some(Rgba8::new(out[0], out[1], out[2], out[3]))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r##"{
      "manifest_version": 1,
      "effects": [
        {
          "id": "com.vimo.effect.blur",
          "name": "Blur",
          "category": "blur",
          "shader": "shaders/blur.frag",
          "params": [
            {"name": "radius", "type": "float", "default": 4.0, "min": 0.0, "max": 32.0},
            {"name": "taps", "type": "int", "default": 9},
            {"name": "on", "type": "bool", "default": true},
            {"name": "tint", "type": "color", "default": "#ff8800ff"},
            {"name": "offset", "type": "vec2", "default": [0.25, 0.5]},
            {"name": "q", "type": "enum", "default": "high", "options": ["low", "high"]}
          ]
        }
      ]
    }"##;

    #[test]
    fn parses_a_well_formed_manifest() {
        let m = Manifest::parse_str(GOOD).unwrap();
        assert_eq!(m.effects.len(), 1);
        let e = &m.effects[0];
        assert_eq!(e.params.len(), 6);
        assert_eq!(e.param_index("radius"), Some(0));
        assert_eq!(e.param_index("RADIUS"), Some(0));
        assert_eq!(e.params[4].default, ParamValue::Vec2(Vec2::new(0.25, 0.5)));
        assert_eq!(e.params[5].default, ParamValue::Enum(1));
    }

    #[test]
    fn rejects_duplicate_ids() {
        let json = r#"{"effects":[{"id":"a"},{"id":"a"}]}"#;
        let m = Manifest::parse_str(json).unwrap();
        assert!(m.into_registry().is_err());
    }

    #[test]
    fn rejects_unknown_type() {
        let json = r#"{"effects":[{"id":"a","params":[{"name":"x","type":"weird"}]}]}"#;
        let err = Manifest::parse_str(json).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidArgument);
    }

    #[test]
    fn rejects_enum_without_options() {
        let json = r#"{"effects":[{"id":"a","params":[{"name":"x","type":"enum","default":0}]}]}"#;
        assert!(Manifest::parse_str(json).is_err());
    }

    #[test]
    fn rejects_duplicate_params() {
        let json = r#"{"effects":[{"id":"a","params":[{"name":"x","type":"float"},{"name":"X","type":"float"}]}]}"#;
        assert!(Manifest::parse_str(json).is_err());
    }

    #[test]
    fn rejects_out_of_range_enum_default() {
        let json = r#"{"effects":[{"id":"a","params":[{"name":"x","type":"enum","default":9,"options":["a"]}]}]}"#;
        assert!(Manifest::parse_str(json).is_err());
    }

    #[test]
    fn rejects_malformed_json_without_panicking() {
        assert!(Manifest::parse_str("{").is_err());
        assert!(Manifest::parse_str("").is_err());
        assert!(Manifest::parse_str("\"a string\"").is_err());
        assert!(Manifest::parse_str("42").is_err());
        assert!(Manifest::parse_str("null").is_err());
        // A bare array is a valid (if unusual) manifest: an empty effect list.
        let empty = Manifest::parse_str("[]").unwrap();
        assert!(empty.effects.is_empty());
    }

    #[test]
    fn accepts_a_bare_array_of_effects() {
        let m = Manifest::parse_str(r#"[{"id":"a"},{"id":"b"}]"#).unwrap();
        assert_eq!(m.effects.len(), 2);
    }

    #[test]
    fn parses_hex_colors() {
        assert_eq!(parse_hex_color("#ff0000"), Some(Rgba8::new(255, 0, 0, 255)));
        assert_eq!(parse_hex_color("#00ff0080"), Some(Rgba8::new(0, 255, 0, 128)));
        assert_eq!(parse_hex_color("#f00"), Some(Rgba8::new(255, 0, 0, 255)));
        assert_eq!(parse_hex_color("zzz"), None);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = r#"{"manifest_version":2,"future_field":true,"effects":[{"id":"a","new_thing":1}]}"#;
        let m = Manifest::parse_str(json).unwrap();
        assert_eq!(m.manifest_version, 2);
        assert_eq!(m.effects.len(), 1);
    }
}
