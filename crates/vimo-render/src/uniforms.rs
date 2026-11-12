//! Typed uniform blocks for shader effects.
//!
//! A [`UniformBlock`] holds the values for one effect instance: the built-in
//! uniforms the engine always supplies, plus one value per manifest
//! parameter.
//!
//! The blocks are **reused** between frames: the render graph clears and
//! refills them, so the steady state performs no allocation. When a frame is
//! assembled, the values are copied into the frame plan's [`UniformArena`], so
//! a plan is self-contained and can be handed to a renderer on another
//! thread without any lifetime ties back to the graph.

use vimo_foundation::math::Vec2;
use vimo_manifest::{ParamType, ParamValue};

/// The built-in uniforms every Vimo shader may use.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BuiltinUniforms {
    /// `u_time`: the local time of the effect, in seconds.
    pub time: f32,
    /// `u_resolution`: the output resolution in pixels.
    pub resolution: [f32; 2],
    /// `u_texel`: the size of one texel in the output.
    pub texel: [f32; 2],
    /// `u_opacity`: the layer opacity.
    pub opacity: f32,
}

/// A reusable block of uniform values for one effect.
#[derive(Debug, Clone, Default)]
pub struct UniformBlock {
    /// The built-in uniforms.
    pub builtins: BuiltinUniforms,
    floats: Vec<f32>,
    vec2s: Vec<[f32; 2]>,
    vec4s: Vec<[f32; 4]>,
    ints: Vec<i32>,
}

impl UniformBlock {
    /// An empty block.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear the parameter values, keeping the built-ins and the capacity.
    pub fn clear(&mut self) {
        self.floats.clear();
        self.vec2s.clear();
        self.vec4s.clear();
        self.ints.clear();
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

    /// The number of parameter values.
    #[must_use]
    pub fn len(&self) -> usize {
        self.floats.len() + self.vec2s.len() + self.vec4s.len() + self.ints.len()
    }

    /// Whether the block holds no parameter values.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The uniform name for a manifest parameter.
///
/// `radius` becomes `u_radius`; `tint color` becomes `u_tint_color`.
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
            block.push_vec4(c.to_f32());
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

/// The location of a block's values inside a [`UniformArena`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UniformRange {
    /// The float values: `(offset, count)`.
    pub floats: (u32, u32),
    /// The 2D vector values: `(offset, count)`.
    pub vec2s: (u32, u32),
    /// The 4D vector values: `(offset, count)`.
    pub vec4s: (u32, u32),
    /// The integer values: `(offset, count)`.
    pub ints: (u32, u32),
}

impl UniformRange {
    /// Whether the range is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.floats.1 == 0 && self.vec2s.1 == 0 && self.vec4s.1 == 0 && self.ints.1 == 0
    }
}

/// A flat, reusable store of uniform values for a whole frame.
///
/// The arena keeps its capacity between frames, so assembling a frame does not
/// allocate once the buffers have grown to their steady-state size.
#[derive(Debug, Clone, Default)]
pub struct UniformArena {
    floats: Vec<f32>,
    vec2s: Vec<[f32; 2]>,
    vec4s: Vec<[f32; 4]>,
    ints: Vec<i32>,
}

impl UniformArena {
    /// An empty arena.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear the arena, keeping its capacity.
    pub fn clear(&mut self) {
        self.floats.clear();
        self.vec2s.clear();
        self.vec4s.clear();
        self.ints.clear();
    }

    /// Append a block's values, returning their location.
    pub fn push_block(&mut self, block: &UniformBlock) -> UniformRange {
        let float_offset = self.floats.len() as u32;
        let vec2_offset = self.vec2s.len() as u32;
        let vec4_offset = self.vec4s.len() as u32;
        let int_offset = self.ints.len() as u32;
        self.floats.extend_from_slice(block.floats());
        self.vec2s.extend_from_slice(block.vec2s());
        self.vec4s.extend_from_slice(block.vec4s());
        self.ints.extend_from_slice(block.ints());
        UniformRange {
            floats: (float_offset, block.floats().len() as u32),
            vec2s: (vec2_offset, block.vec2s().len() as u32),
            vec4s: (vec4_offset, block.vec4s().len() as u32),
            ints: (int_offset, block.ints().len() as u32),
        }
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

    /// The float values of a range.
    #[must_use]
    pub fn floats_at(&self, range: UniformRange) -> &[f32] {
        let (offset, count) = range.floats;
        &self.floats[offset as usize..(offset + count) as usize]
    }

    /// The 2D vector values of a range.
    #[must_use]
    pub fn vec2s_at(&self, range: UniformRange) -> &[[f32; 2]] {
        let (offset, count) = range.vec2s;
        &self.vec2s[offset as usize..(offset + count) as usize]
    }

    /// The 4D vector values of a range.
    #[must_use]
    pub fn vec4s_at(&self, range: UniformRange) -> &[[f32; 4]] {
        let (offset, count) = range.vec4s;
        &self.vec4s[offset as usize..(offset + count) as usize]
    }

    /// The integer values of a range.
    #[must_use]
    pub fn ints_at(&self, range: UniformRange) -> &[i32] {
        let (offset, count) = range.ints;
        &self.ints[offset as usize..(offset + count) as usize]
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
        assert!(write_param(
            &mut b,
            ParamType::Float,
            ParamValue::Float(1.5)
        ));
        assert!(write_param(&mut b, ParamType::Int, ParamValue::Int(3)));
        assert!(write_param(
            &mut b,
            ParamType::Color,
            ParamValue::Color(Rgba8::WHITE)
        ));
        assert_eq!(b.floats(), &[1.5]);
        assert_eq!(b.ints(), &[3]);
        assert_eq!(b.vec4s().len(), 1);
    }

    #[test]
    fn clear_keeps_the_builtins() {
        let mut b = UniformBlock::new();
        b.builtins.time = 1.5;
        b.push_float(1.0);
        b.clear();
        assert!(b.is_empty());
        assert_eq!(b.builtins.time, 1.5);
    }

    #[test]
    fn the_arena_records_ranges() {
        let mut arena = UniformArena::new();
        let mut a = UniformBlock::new();
        a.push_float(1.0);
        a.push_float(2.0);
        let mut b = UniformBlock::new();
        b.push_float(3.0);
        b.push_int(9);

        let ra = arena.push_block(&a);
        let rb = arena.push_block(&b);
        assert_eq!(arena.floats_at(ra), &[1.0, 2.0]);
        assert_eq!(arena.floats_at(rb), &[3.0]);
        assert_eq!(arena.ints_at(rb), &[9]);
        assert_eq!(arena.floats(), &[1.0, 2.0, 3.0]);
    }
}
