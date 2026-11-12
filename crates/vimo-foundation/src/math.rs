//! Small, allocation-free linear algebra for the render graph.
//!
//! Matrices are **column-major** (`m[col * 4 + row]`) to match OpenGL/GLES
//! uniform upload directly, so no transposition happens on the hot path.
//! Everything is `f32` and `Copy`.

use core::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

/// 2-component vector.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
}

impl Vec2 {
    /// Zero vector.
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    /// Unit X axis.
    pub const ONE: Vec2 = Vec2 { x: 1.0, y: 1.0 };

    /// Construct.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Vec2 { x, y }
    }

    /// Dot product.
    #[must_use]
    pub fn dot(self, o: Vec2) -> f32 {
        self.x * o.x + self.y * o.y
    }

    /// Length.
    #[must_use]
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// Inverse length (0 for a zero vector).
    #[must_use]
    pub fn inverse_length(self) -> f32 {
        let l = self.length();
        if l > f32::EPSILON {
            1.0 / l
        } else {
            0.0
        }
    }

    /// Normalize.
    #[must_use]
    pub fn normalized(self) -> Vec2 {
        let l = self.length();
        if l > f32::EPSILON {
            Vec2::new(self.x / l, self.y / l)
        } else {
            Vec2::ZERO
        }
    }

    /// Component-wise minimum.
    #[must_use]
    pub fn min(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.min(o.x), self.y.min(o.y))
    }

    /// Component-wise maximum.
    #[must_use]
    pub fn max(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.max(o.x), self.y.max(o.y))
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}
impl Mul<Vec2> for Vec2 {
    type Output = Vec2;
    fn mul(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x * o.x, self.y * o.y)
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}
impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        *self = *self - o;
    }
}
impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, s: f32) {
        *self = *self * s;
    }
}
impl Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, s: f32) -> Vec2 {
        Vec2::new(self.x / s, self.y / s)
    }
}

/// 3-component vector.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
}

impl Vec3 {
    /// Zero vector.
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    /// One vector.
    pub const ONE: Vec3 = Vec3 {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };

    /// Construct.
    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// Build from a 2D vector and a z component.
    #[must_use]
    pub const fn from_vec2(v: Vec2, z: f32) -> Vec3 {
        Vec3 { x: v.x, y: v.y, z }
    }

    /// Downcast to 2D, dropping z.
    #[must_use]
    pub const fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    /// Dot product.
    #[must_use]
    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    /// Cross product.
    #[must_use]
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    /// Length.
    #[must_use]
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// Normalize.
    #[must_use]
    pub fn normalized(self) -> Vec3 {
        let l = self.length();
        if l > f32::EPSILON {
            Vec3::new(self.x / l, self.y / l, self.z / l)
        } else {
            Vec3::ZERO
        }
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// 4-component vector.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec4 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
    /// W component.
    pub w: f32,
}

impl Vec4 {
    /// Zero vector.
    pub const ZERO: Vec4 = Vec4 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 0.0,
    };
    /// One vector.
    pub const ONE: Vec4 = Vec4 {
        x: 1.0,
        y: 1.0,
        z: 1.0,
        w: 1.0,
    };

    /// Construct.
    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Vec4 {
        Vec4 { x, y, z, w }
    }

    /// Build from a 3D vector and a w component.
    #[must_use]
    pub const fn from_vec3(v: Vec3, w: f32) -> Vec4 {
        Vec4 {
            x: v.x,
            y: v.y,
            z: v.z,
            w,
        }
    }

    /// Build from a 2D vector and z/w components.
    #[must_use]
    pub const fn from_vec2(v: Vec2, z: f32, w: f32) -> Vec4 {
        Vec4 {
            x: v.x,
            y: v.y,
            z,
            w,
        }
    }

    /// Swizzle into a 3D vector.
    #[must_use]
    pub const fn xyz(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }

    /// Swizzle into a 2D vector.
    #[must_use]
    pub const fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    /// Dot product.
    #[must_use]
    pub fn dot(self, o: Vec4) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }

    /// Component-wise clamp.
    #[must_use]
    pub fn clamp(self, lo: f32, hi: f32) -> Vec4 {
        Vec4::new(
            self.x.clamp(lo, hi),
            self.y.clamp(lo, hi),
            self.z.clamp(lo, hi),
            self.w.clamp(lo, hi),
        )
    }
}

impl Add for Vec4 {
    type Output = Vec4;
    fn add(self, o: Vec4) -> Vec4 {
        Vec4::new(self.x + o.x, self.y + o.y, self.z + o.z, self.w + o.w)
    }
}
impl Sub for Vec4 {
    type Output = Vec4;
    fn sub(self, o: Vec4) -> Vec4 {
        Vec4::new(self.x - o.x, self.y - o.y, self.z - o.z, self.w - o.w)
    }
}
impl Mul<f32> for Vec4 {
    type Output = Vec4;
    fn mul(self, s: f32) -> Vec4 {
        Vec4::new(self.x * s, self.y * s, self.z * s, self.w * s)
    }
}

/// Axis-aligned rectangle in normalized 0..1 space.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// Construct.
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }

    /// Full-frame rect.
    pub const FULL: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };

    /// Right edge.
    #[must_use]
    pub fn right(self) -> f32 {
        self.x + self.w
    }

    /// Bottom edge.
    #[must_use]
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    /// Whether the rect has positive area.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }
}

/// 3x3 matrix, column-major.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat3 {
    /// Column-major storage: `m[col * 3 + row]`.
    pub data: [f32; 9],
}

impl Default for Mat3 {
    fn default() -> Self {
        Mat3::IDENTITY
    }
}

impl Mat3 {
    /// Identity matrix.
    pub const IDENTITY: Mat3 = Mat3 {
        data: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };

    /// Construct from column-major data.
    #[must_use]
    pub const fn from_array(data: [f32; 9]) -> Self {
        Mat3 { data }
    }

    /// Read an element (column, row).
    #[must_use]
    pub fn get(&self, col: usize, row: usize) -> f32 {
        self.data[col * 3 + row]
    }

    /// Write an element (column, row).
    pub fn set(&mut self, col: usize, row: usize, v: f32) {
        self.data[col * 3 + row] = v;
    }

    /// Matrix multiplication.
    #[must_use]
    pub fn mul(&self, o: &Mat3) -> Mat3 {
        let mut out = [0.0f32; 9];
        for c in 0..3 {
            for r in 0..3 {
                let mut sum = 0.0;
                for k in 0..3 {
                    sum += self.data[k * 3 + r] * o.data[c * 3 + k];
                }
                out[c * 3 + r] = sum;
            }
        }
        Mat3 { data: out }
    }

    /// Multiply a column vector.
    #[must_use]
    pub fn transform_vec3(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.data[0] * v.x + self.data[3] * v.y + self.data[6] * v.z,
            self.data[1] * v.x + self.data[4] * v.y + self.data[7] * v.z,
            self.data[2] * v.x + self.data[5] * v.y + self.data[8] * v.z,
        )
    }

    /// Transpose.
    #[must_use]
    pub fn transposed(&self) -> Mat3 {
        let mut out = [0.0f32; 9];
        for c in 0..3 {
            for r in 0..3 {
                out[r * 3 + c] = self.data[c * 3 + r];
            }
        }
        Mat3 { data: out }
    }

    /// Determinant.
    #[must_use]
    pub fn determinant(&self) -> f32 {
        let m = &self.data;
        m[0] * (m[4] * m[8] - m[7] * m[5]) - m[3] * (m[1] * m[8] - m[7] * m[2])
            + m[6] * (m[1] * m[5] - m[4] * m[2])
    }

    /// Inverse, or `None` when singular.
    #[must_use]
    pub fn inverse(&self) -> Option<Mat3> {
        let det = self.determinant();
        if det.abs() < 1e-20 {
            return None;
        }
        let inv_det = 1.0 / det;
        let m = &self.data;
        let mut out = [0.0f32; 9];
        out[0] = (m[4] * m[8] - m[7] * m[5]) * inv_det;
        out[1] = (m[7] * m[2] - m[1] * m[8]) * inv_det;
        out[2] = (m[1] * m[5] - m[4] * m[2]) * inv_det;
        out[3] = (m[6] * m[5] - m[3] * m[8]) * inv_det;
        out[4] = (m[0] * m[8] - m[6] * m[2]) * inv_det;
        out[5] = (m[3] * m[2] - m[0] * m[5]) * inv_det;
        out[6] = (m[3] * m[7] - m[6] * m[4]) * inv_det;
        out[7] = (m[6] * m[1] - m[0] * m[7]) * inv_det;
        out[8] = (m[0] * m[4] - m[3] * m[1]) * inv_det;
        Some(Mat3 { data: out })
    }

    /// Build a 2D affine transform from scale, rotation (radians) and translation.
    #[must_use]
    pub fn affine_2d(scale: Vec2, rotation_rad: f32, translation: Vec2) -> Mat3 {
        let (s, c) = rotation_rad.sin_cos();
        let (sx, sy) = (scale.x, scale.y);
        // column-major: [ sx*c, sx*s, 0,  -sy*s, sy*c, 0,  tx, ty, 1 ]
        Mat3 {
            data: [
                sx * c,
                sx * s,
                0.0, //
                -sy * s,
                sy * c,
                0.0, //
                translation.x,
                translation.y,
                1.0,
            ],
        }
    }
}

/// 4x4 matrix, column-major (OpenGL layout).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    /// Column-major storage: `m[col * 4 + row]`.
    pub data: [f32; 16],
}

impl Default for Mat4 {
    fn default() -> Self {
        Mat4::IDENTITY
    }
}

impl Mat4 {
    /// Identity matrix.
    pub const IDENTITY: Mat4 = Mat4 {
        data: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    /// Construct from column-major data.
    #[must_use]
    pub const fn from_array(data: [f32; 16]) -> Self {
        Mat4 { data }
    }

    /// Read an element (column, row).
    #[must_use]
    pub fn get(&self, col: usize, row: usize) -> f32 {
        self.data[col * 4 + row]
    }

    /// Write an element (column, row).
    pub fn set(&mut self, col: usize, row: usize, v: f32) {
        self.data[col * 4 + row] = v;
    }

    /// Multiply a homogeneous vector.
    #[must_use]
    pub fn transform_vec4(&self, v: Vec4) -> Vec4 {
        let mut out = [0.0f32; 4];
        for (r, out_r) in out.iter_mut().enumerate() {
            *out_r = self.data[r] * v.x
                + self.data[4 + r] * v.y
                + self.data[8 + r] * v.z
                + self.data[12 + r] * v.w;
        }
        Vec4::new(out[0], out[1], out[2], out[3])
    }

    /// Matrix multiplication.
    #[must_use]
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut out = [0.0f32; 16];
        for c in 0..4 {
            for r in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += self.data[k * 4 + r] * o.data[c * 4 + k];
                }
                out[c * 4 + r] = sum;
            }
        }
        Mat4 { data: out }
    }

    /// Transpose.
    #[must_use]
    pub fn transposed(&self) -> Mat4 {
        let mut out = [0.0f32; 16];
        for c in 0..4 {
            for r in 0..4 {
                out[r * 4 + c] = self.data[c * 4 + r];
            }
        }
        Mat4 { data: out }
    }

    /// Build an orthographic projection matrix (GL convention, depth -1..1).
    #[must_use]
    pub fn ortho(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Mat4 {
        let lr = 1.0 / (left - right);
        let bt = 1.0 / (bottom - top);
        let nf = 1.0 / (near - far);
        Mat4 {
            data: [
                -2.0 * lr,
                0.0,
                0.0,
                0.0, //
                0.0,
                -2.0 * bt,
                0.0,
                0.0, //
                0.0,
                0.0,
                2.0 * nf,
                0.0, //
                (left + right) * lr,
                (top + bottom) * bt,
                (far + near) * nf,
                1.0,
            ],
        }
    }

    /// Build a translation matrix.
    #[must_use]
    pub fn translation(t: Vec3) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        m.data[12] = t.x;
        m.data[13] = t.y;
        m.data[14] = t.z;
        m
    }

    /// Build a 2D scale matrix.
    #[must_use]
    pub fn scale_2d(s: Vec2) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        m.data[0] = s.x;
        m.data[5] = s.y;
        m
    }

    /// Build a rotation around Z.
    #[must_use]
    pub fn rotation_z(rad: f32) -> Mat4 {
        let (s, c) = rad.sin_cos();
        let mut m = Mat4::IDENTITY;
        m.data[0] = c;
        m.data[1] = s;
        m.data[4] = -s;
        m.data[5] = c;
        m
    }
}

/// Linear interpolation between two values.
#[inline]
#[must_use]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Hermite interpolation (smoothstep-like ease).
#[inline]
#[must_use]
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() < f32::EPSILON {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Clamp a value into `[0, 1]`.
#[inline]
#[must_use]
pub fn saturate(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Cubic bezier evaluation for a single dimension.
#[inline]
#[must_use]
pub fn cubic_bezier(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let u = 1.0 - t;
    let uu = u * u;
    let tt = t * t;
    uu * u * p0 + 3.0 * uu * t * p1 + 3.0 * u * tt * p2 + tt * t * p3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mat4_mul_is_associative_with_identity() {
        let m = Mat4::IDENTITY.mul(&Mat4::IDENTITY);
        assert_eq!(m, Mat4::IDENTITY);
    }

    #[test]
    fn mat3_inverse_roundtrip() {
        let m = Mat3::affine_2d(Vec2::new(2.0, 3.0), 0.7, Vec2::new(4.0, -1.0));
        let inv = m.inverse().unwrap();
        let id = m.mul(&inv);
        for i in 0..9 {
            let expected = if i % 4 == 0 { 1.0 } else { 0.0 };
            assert!(
                (id.data[i] - expected).abs() < 1e-4,
                "index {i}: {}",
                id.data[i]
            );
        }
    }

    #[test]
    fn ortho_maps_ndc() {
        // GL convention: bottom edge -> -1, top edge -> +1.
        let m = Mat4::ortho(0.0, 1920.0, 1080.0, 0.0, -1.0, 1.0);
        let p = m.transform_vec4(Vec4::new(0.0, 1080.0, 0.0, 1.0));
        assert!((p.x + 1.0).abs() < 1e-5);
        assert!((p.y + 1.0).abs() < 1e-5);
        let q = m.transform_vec4(Vec4::new(1920.0, 0.0, 0.0, 1.0));
        assert!((q.x - 1.0).abs() < 1e-5);
        assert!((q.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn bezier_endpoints() {
        assert!((cubic_bezier(0.0, 0.0, 1.0, 1.0, 0.0)).abs() < 1e-6);
        assert!((cubic_bezier(0.0, 0.0, 1.0, 1.0, 1.0) - 1.0).abs() < 1e-6);
    }
}
