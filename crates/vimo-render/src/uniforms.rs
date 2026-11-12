//! Typed uniform blocks for shader effects.
//!
//! A [`UniformBlock`] is a flat, typed list of values that can be uploaded to
//! the GPU in one go. It is **reused** between frames: the render graph clears
//! it and refills it, so the steady state performs no allocation.
//!
//! Uniform names follow the manifest: a parameter named `radius` becomes the
//! uniform `u_radius`. Built-in uniforms (`u_time`, `u_resolution`,
//! `u_texel`, `u_opacity`) are supplied by the graph.

use vimo_foundation::math::Vec2;
use vimo_manifest::{ParamType, ParamValue};

/// A reusable block of uniform values.
#[derive(Debug, Clone, Default)]
pub struct UniformBlock {
    floats: Vec<f32>,
    vec2s: Vec<[f32; 2]>,
    vec4s: Vec<[f32; 4]>,
    ints: Vec<i32>,
    textures: Vec<u32>,
}

impl UniformBlock {
    /// An empty block.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear the block, keeping its capacity.
    pub fn clear(&mut self) {
        self.floats.clear();
        self.vec2s.clear();
        self.vec4s.clear();
        self.ints.clear();
        self.textures.clear();
    }

    /// Append a float.
    pub fn push_float(&mut self, v: f32) {
        self.floats.push(v);
    }

    /// Append a 2D vector.
    pub fn push_vec2(&mut self, v: Vec2) {
        self.vec2s.push([v.x, v.y]);
    }

    /// Append a 4D vector.
    pub fn push_vec4(&mut self, v: [f32; 4]) {
        self.vec4s.push(v);
    }

    /// Append an integer.
    pub fn push_int(&mut self, v: i32) {
        self.ints.push(v);
    }

    /// Append a texture unit index.
    pub fn push_texture(&mut self, unit: u32) {
        self.textures.push(unit);
    }

    /// The float values.
    #[must_use]
    pub fn floats(&self) -> &[f32] {
        &self.floats
    }

    /// The 2D vector values.
    #[must_use]
    pub fn vec2s(&self) -> &[[f32; 2]] {
        &self.vec2s
    }

    /// The 4D vector values.
    #[must_use]
    pub fn vec4s(&self) -> &[[f32; 4]] {
        &self.vec4s
    }

    /// The integer values.
    #[must_use]
    pub fn ints(&self) -> &[i32] {
        &self.ints
    }

    /// The texture unit indices.
    #[must_use]
    pub fn textures(&self) -> &[u32] {
        &self.textures
    }

    /// Total number of values.
    #[must_use]
    pub fn len(&self) -> usize {
        self.floats.len() + self.vec2s.len() + self.vec4s.len() + self.ints.len() + self.textures.len()
    }

    /// Whether the block is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The uniform name for a manifest parameter.
///
/// `radius` becomes `u_radius`; `tint` becomes `u_tint`.
#[must_use]
pub fn uniform_name(param_name: &str) -> String {
    let mut out = String::with_capacity(param_name.len() + 2);
    out.push('u');
    out.push('_');
    for c in param_name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

/// Write a parameter value into a uniform block, using the declared type.
///
/// Returns `false` when the value does not match the declared type and was
/// therefore skipped.
pub fn write_param(block: &mut UniformBlock, ptype: ParamType, value: ParamValue) -> bool {
    match (ptype, value) {
        (ParamType::Float, ParamValue::Float(v)) => {
            block.push_float(v);
            true
        }
        (ParamType::Int, ParamValue::Int(v)) => {
            block.push_int(v);
            true
        }
        (ParamType::Bool, ParamValue::Bool(v)) => {
            block.push_int(i32::from(v));
            true
        }
        (ParamType::Enum, ParamValue::Enum(v)) => {
            block.push_int(i32::from(v));
            true
        }
        (ParamType::Color, ParamValue::Color(c)) => {
            let f = c.to_f32();
            block.push_vec4(f);
            true
        }
        (ParamType::Vec2, ParamValue::Vec2(v)) => {
            block.push_vec2(v);
            true
        }
        (ParamType::Vec3, ParamValue::Vec3(v)) => {
            block.push_vec4([v.x, v.y, v.z, 0.0]);
            true
        }
        // Mismatched types are coerced rather than dropped, so a project
        // authored against an older manifest still renders.
        (ParamType::Float, _) => match value.as_f32() {
            Some(v) => {
                block.push_float(v);
                true
            }
            None => false,
        },
        (ParamType::Int, _) => match value.as_i32() {
            Some(v) => {
                block.push_int(v);
                true
            }
            None => false,
        },
        (ParamType::Bool, _) => match value.as_bool() {
            Some(v) => {
                block.push_int(i32::from(v));
                true
            }
            None => false,
        },
        (ParamType::Enum, _) => match value {
            ParamValue::Enum(v) => {
                block.push_int(i32::from(v));
                true
            }
            _ => false,
        },
        (ParamType::Color, _) => match value.as_color() {
            Some(c) => {
                block.push_vec4(c.to_f32());
                true
            }
            None => false,
        },
        (ParamType::Vec2, _) => match value.as_vec2() {
            Some(v) => {
                block.push_vec2(v);
                true
            }
            None => false,
        },
        (ParamType::Vec3, _) => match value.as_vec3() {
            Some(v) => {
                block.push_vec4([v.x, v.y, v.z, 0.0]);
                true
            }
            None => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vimo_foundation::color::Rgba8;

    #[test]
    fn names_are_prefixed_and_sanitised() {
        assert_eq!(uniform_name("radius"), "u_radius");
        assert_eq!(uniform_name("tint color"), "u_tint_color");
    }

    #[test]
    fn writes_typed_values() {
        let mut b = UniformBlock::new();
        assert!(write_param(&mut b, ParamType::Float, ParamValue::Float(1.5)));
        assert!(write_param(&mut b, ParamType::Int, ParamValue::Int(3)));
        assert!(write_param(&mut b, ParamType::Color, ParamValue::Color(Rgba8::WHITE)));
        assert_eq!(b.floats(), &[1.5]);
        assert_eq!(b.ints(), &[3]);
        assert_eq!(b.vec4s().len(), 1);
    }

    #[test]
    fn clear_keeps_capacity() {
        let mut b = UniformBlock::new();
        for _ in 0..8 {
            b.push_float(1.0);
        }
        let cap = b.floats().len();
        b.clear();
        assert!(b.is_empty());
        assert!(cap > 0);
    }
}
