//! Typed effect parameters.
//!
//! A parameter's *type* is fixed by the effect's manifest entry; a project only
//! stores the value. Keeping the value enum small and `Copy` means evaluating
//! animated parameters never allocates, which is what makes per-frame effect
//! evaluation cheap.

use vimo_foundation::color::Rgba8;
use vimo_foundation::math::{Vec2, Vec3};

/// The declared type of a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamType {
    /// 32-bit float.
    Float,
    /// 32-bit signed integer.
    Int,
    /// Boolean.
    Bool,
    /// RGBA colour.
    Color,
    /// 2D vector.
    Vec2,
    /// 3D vector.
    Vec3,
    /// Enumeration; the value is an index into the declared option list.
    Enum,
}

impl ParamType {
    /// Parse the manifest's textual type name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "float" | "f32" | "number" => Some(ParamType::Float),
            "int" | "i32" | "integer" => Some(ParamType::Int),
            "bool" | "boolean" => Some(ParamType::Bool),
            "color" | "colour" => Some(ParamType::Color),
            "vec2" => Some(ParamType::Vec2),
            "vec3" => Some(ParamType::Vec3),
            "enum" => Some(ParamType::Enum),
            _ => None,
        }
    }

    /// The canonical textual name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ParamType::Float => "float",
            ParamType::Int => "int",
            ParamType::Bool => "bool",
            ParamType::Color => "color",
            ParamType::Vec2 => "vec2",
            ParamType::Vec3 => "vec3",
            ParamType::Enum => "enum",
        }
    }

    /// Whether values of this type can be keyframed.
    #[must_use]
    pub const fn is_animatable(self) -> bool {
        !matches!(self, ParamType::Bool | ParamType::Enum)
    }
}

/// A parameter value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamValue {
    /// Float value.
    Float(f32),
    /// Integer value.
    Int(i32),
    /// Boolean value.
    Bool(bool),
    /// Colour value.
    Color(Rgba8),
    /// 2D vector value.
    Vec2(Vec2),
    /// 3D vector value.
    Vec3(Vec3),
    /// Enumeration index.
    Enum(u16),
}

impl ParamValue {
    /// The type this value belongs to.
    #[must_use]
    pub const fn param_type(&self) -> ParamType {
        match self {
            ParamValue::Float(_) => ParamType::Float,
            ParamValue::Int(_) => ParamType::Int,
            ParamValue::Bool(_) => ParamType::Bool,
            ParamValue::Color(_) => ParamType::Color,
            ParamValue::Vec2(_) => ParamType::Vec2,
            ParamValue::Vec3(_) => ParamType::Vec3,
            ParamValue::Enum(_) => ParamType::Enum,
        }
    }

    /// Numeric coercion for animation and uniform binding.
    ///
    /// Booleans map to `0.0`/`1.0`, enum indices to their numeric value,
    /// colours to their luminance-free red channel is *not* used — colours are
    /// never coerced to a scalar, they return `None`.
    #[must_use]
    pub fn as_f32(&self) -> Option<f32> {
        match *self {
            ParamValue::Float(v) => Some(v),
            ParamValue::Int(v) => Some(v as f32),
            ParamValue::Bool(v) => Some(if v { 1.0 } else { 0.0 }),
            ParamValue::Enum(v) => Some(v as f32),
            ParamValue::Color(_) | ParamValue::Vec2(_) | ParamValue::Vec3(_) => None,
        }
    }

    /// Integer coercion.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        match *self {
            ParamValue::Int(v) => Some(v),
            ParamValue::Float(v) => Some(v.round() as i32),
            ParamValue::Bool(v) => Some(i32::from(v)),
            ParamValue::Enum(v) => Some(i32::from(v)),
            ParamValue::Color(_) | ParamValue::Vec2(_) | ParamValue::Vec3(_) => None,
        }
    }

    /// Boolean coercion.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            ParamValue::Bool(v) => Some(v),
            _ => None,
        }
    }

    /// Colour accessor.
    #[must_use]
    pub fn as_color(&self) -> Option<Rgba8> {
        match *self {
            ParamValue::Color(c) => Some(c),
            _ => None,
        }
    }

    /// 2D vector accessor.
    #[must_use]
    pub fn as_vec2(&self) -> Option<Vec2> {
        match *self {
            ParamValue::Vec2(v) => Some(v),
            _ => None,
        }
    }

    /// 3D vector accessor.
    #[must_use]
    pub fn as_vec3(&self) -> Option<Vec3> {
        match *self {
            ParamValue::Vec3(v) => Some(v),
            _ => None,
        }
    }
}

/// A parameter description from a manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDesc {
    /// Parameter name (unique within the effect).
    pub name: String,
    /// Declared type.
    pub ptype: ParamType,
    /// Default value.
    pub default: ParamValue,
    /// Optional minimum (float params).
    pub min: Option<f32>,
    /// Optional maximum (float params).
    pub max: Option<f32>,
    /// Optional step for UI editing.
    pub step: Option<f32>,
    /// Human readable label.
    pub label: String,
    /// Whether the parameter may be keyframed.
    pub animatable: bool,
    /// Enum option names (empty for non-enum types).
    pub options: Vec<String>,
}

impl ParamDesc {
    /// Clamp a float value into the declared range.
    #[must_use]
    pub fn clamp_f32(&self, v: f32) -> f32 {
        let mut v = v;
        if let Some(lo) = self.min {
            v = v.max(lo);
        }
        if let Some(hi) = self.max {
            v = v.min(hi);
        }
        v
    }

    /// Coerce an arbitrary value into this parameter's declared type.
    ///
    /// Used when loading a project whose parameter values were authored against
    /// a different manifest revision. Numeric values are additionally clamped
    /// into the declared range, so a project that stored an out-of-range value
    /// (e.g. before a range was tightened) still loads.
    #[must_use]
    pub fn coerce(&self, value: ParamValue) -> ParamValue {
        match self.ptype {
            ParamType::Float => match value.as_f32() {
                Some(v) => ParamValue::Float(self.clamp_f32(v)),
                None => self.default,
            },
            ParamType::Int => match value.as_i32() {
                Some(v) => ParamValue::Int(v),
                None => self.default,
            },
            ParamType::Bool => match value.as_bool() {
                Some(v) => ParamValue::Bool(v),
                None => self.default,
            },
            ParamType::Color => match value.as_color() {
                Some(v) => ParamValue::Color(v),
                None => self.default,
            },
            ParamType::Vec2 => match value.as_vec2() {
                Some(v) => ParamValue::Vec2(v),
                None => self.default,
            },
            ParamType::Vec3 => match value.as_vec3() {
                Some(v) => ParamValue::Vec3(v),
                None => self.default,
            },
            ParamType::Enum => match value {
                ParamValue::Enum(v) => ParamValue::Enum(v),
                _ => self.default,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerce_clamps_floats() {
        let d = ParamDesc {
            name: "x".into(),
            ptype: ParamType::Float,
            default: ParamValue::Float(1.0),
            min: Some(0.0),
            max: Some(2.0),
            step: None,
            label: "x".into(),
            animatable: true,
            options: Vec::new(),
        };
        assert_eq!(d.coerce(ParamValue::Float(5.0)), ParamValue::Float(2.0));
        assert_eq!(d.coerce(ParamValue::Int(1)), ParamValue::Float(1.0));
    }

    #[test]
    fn type_names_roundtrip() {
        for t in [
            ParamType::Float,
            ParamType::Int,
            ParamType::Bool,
            ParamType::Color,
            ParamType::Vec2,
            ParamType::Vec3,
            ParamType::Enum,
        ] {
            assert_eq!(ParamType::parse(t.as_str()), Some(t));
        }
    }
}
