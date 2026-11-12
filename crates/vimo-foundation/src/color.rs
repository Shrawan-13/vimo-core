//! Colour types and colour-space conversion.

/// 8-bit per channel RGBA, non-premultiplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgba8 {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha.
    pub a: u8,
}

impl Rgba8 {
    /// Opaque black.
    pub const BLACK: Rgba8 = Rgba8 {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    /// Opaque white.
    pub const WHITE: Rgba8 = Rgba8 {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    /// Fully transparent.
    pub const TRANSPARENT: Rgba8 = Rgba8 {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// Construct.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba8 { r, g, b, a }
    }

    /// Construct from opaque RGB.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba8 { r, g, b, a: 255 }
    }

    /// Pack into a `u32` as `0xAABBGGRR` (little-endian RGBA, GL friendly).
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        (self.a as u32) << 24 | (self.b as u32) << 16 | (self.g as u32) << 8 | self.r as u32
    }

    /// Unpack from `0xAABBGGRR`.
    #[must_use]
    pub const fn from_u32(v: u32) -> Self {
        Rgba8 {
            r: (v & 0xff) as u8,
            g: ((v >> 8) & 0xff) as u8,
            b: ((v >> 16) & 0xff) as u8,
            a: ((v >> 24) & 0xff) as u8,
        }
    }

    /// Linear (scene-referred) float components.
    #[must_use]
    pub fn to_linear(self) -> [f32; 4] {
        [
            srgb_to_linear(self.r),
            srgb_to_linear(self.g),
            srgb_to_linear(self.b),
            self.a as f32 / 255.0,
        ]
    }

    /// Straight (un-premultiplied) float components in sRGB.
    #[must_use]
    pub fn to_f32(self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }
}

/// Convert an 8-bit sRGB channel to linear light.
#[must_use]
pub fn srgb_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Convert a linear-light channel to 8-bit sRGB.
#[must_use]
pub fn linear_to_srgb(v: f32) -> u8 {
    let c = v.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5) as u8
}

/// Straight-alpha RGBA float colour.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RgbaF32 {
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
    /// Alpha.
    pub a: f32,
}

impl RgbaF32 {
    /// Transparent black.
    pub const TRANSPARENT: RgbaF32 = RgbaF32 {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    /// Opaque white.
    pub const WHITE: RgbaF32 = RgbaF32 {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    /// Construct.
    #[must_use]
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        RgbaF32 { r, g, b, a }
    }

    /// Convert to 8-bit sRGB.
    #[must_use]
    pub fn to_rgba8(self) -> Rgba8 {
        Rgba8 {
            r: linear_to_srgb(self.r),
            g: linear_to_srgb(self.g),
            b: linear_to_srgb(self.b),
            a: (self.a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        }
    }

    /// Convert from 8-bit sRGB (treated as linear for display purposes).
    #[must_use]
    pub fn from_rgba8(c: Rgba8) -> Self {
        RgbaF32 {
            r: c.r as f32 / 255.0,
            g: c.g as f32 / 255.0,
            b: c.b as f32 / 255.0,
            a: c.a as f32 / 255.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u32_packing_roundtrip() {
        let c = Rgba8::new(10, 20, 30, 40);
        assert_eq!(Rgba8::from_u32(c.to_u32()), c);
    }

    #[test]
    fn srgb_linear_roundtrip() {
        for v in 0..=255u8 {
            assert_eq!(linear_to_srgb(srgb_to_linear(v)), v, "value {v}");
        }
    }
}
