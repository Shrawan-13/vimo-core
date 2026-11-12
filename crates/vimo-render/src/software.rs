//! A CPU reference rasteriser.
//!
//! This is the **reference implementation** of the frame plan. It is used for
//! golden-frame tests on a desktop and as the fallback path on devices without
//! a usable GPU.
//!
//! It is deliberately simple and slow: correctness and clarity matter more
//! than speed here. The GPU path in the Android layer is the fast path; this
//! one defines what "correct" means.

use vimo_foundation::color::Rgba8;
use vimo_foundation::math::Rect;
use vimo_project::model::BlendMode;

use crate::plan::{DrawStep, FramePlan, TextureRef};

/// An RGBA8 image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameBuffer {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major pixels.
    pub pixels: Vec<Rgba8>,
}

impl FrameBuffer {
    /// A new, transparent framebuffer.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        FrameBuffer {
            width,
            height,
            pixels: vec![Rgba8::TRANSPARENT; (width as usize) * (height as usize)],
        }
    }

    /// A new framebuffer filled with a colour.
    #[must_use]
    pub fn filled(width: u32, height: u32, color: Rgba8) -> Self {
        FrameBuffer {
            width,
            height,
            pixels: vec![color; (width as usize) * (height as usize)],
        }
    }

    /// Get a pixel.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgba8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(self.pixels[(y as usize) * (self.width as usize) + x as usize])
    }

    /// Set a pixel.
    pub fn set_pixel(&mut self, x: u32, y: u32, c: Rgba8) {
        if x < self.width && y < self.height {
            let idx = (y as usize) * (self.width as usize) + x as usize;
            self.pixels[idx] = c;
        }
    }
}

/// A borrowed view of a decoded picture.
#[derive(Debug, Clone, Copy)]
pub struct FrameView<'a> {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major RGBA8 pixels.
    pub pixels: &'a [Rgba8],
}

impl<'a> FrameView<'a> {
    /// Sample a pixel with nearest-neighbour filtering.
    #[must_use]
    pub fn sample_nearest(&self, u: f32, v: f32) -> [f32; 4] {
        if self.width == 0 || self.height == 0 {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let x = ((u.clamp(0.0, 1.0) * self.width as f32) as usize).min(self.width as usize - 1);
        let y = ((v.clamp(0.0, 1.0) * self.height as f32) as usize).min(self.height as usize - 1);
        let idx = y * self.width as usize + x;
        self.pixels.get(idx).map(|c| c.to_f32()).unwrap_or([0.0; 4])
    }

    /// Sample a pixel with bilinear filtering.
    #[must_use]
    pub fn sample_bilinear(&self, u: f32, v: f32) -> [f32; 4] {
        if self.width == 0 || self.height == 0 {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let fx = u.clamp(0.0, 1.0) * self.width as f32 - 0.5;
        let fy = v.clamp(0.0, 1.0) * self.height as f32 - 0.5;
        let x0 = fx.floor();
        let y0 = fy.floor();
        let tx = fx - x0;
        let ty = fy - y0;
        let x0i = (x0.max(0.0) as usize).min(self.width as usize - 1);
        let y0i = (y0.max(0.0) as usize).min(self.height as usize - 1);
        let x1i = (x0i as i32 + 1).clamp(0, self.width as i32 - 1) as usize;
        let y1i = (y0i as i32 + 1).clamp(0, self.height as i32 - 1) as usize;
        let w = self.width as usize;
        let p = |x: usize, y: usize| -> [f32; 4] {
            self.pixels
                .get(y * w + x)
                .map(|c| c.to_f32())
                .unwrap_or([0.0; 4])
        };
        let c00 = p(x0i, y0i);
        let c10 = p(x1i, y0i);
        let c01 = p(x0i, y1i);
        let c11 = p(x1i, y1i);
        let mut out = [0.0f32; 4];
        for i in 0..4 {
            let top = c00[i] * (1.0 - tx) + c10[i] * tx;
            let bottom = c01[i] * (1.0 - tx) + c11[i] * tx;
            out[i] = top * (1.0 - ty) + bottom * ty;
        }
        out
    }
}

/// Supplies decoded pictures for texture references.
pub trait FrameProvider {
    /// Return the picture for a texture request.
    fn frame_for(&self, request: &TextureRef) -> Option<FrameView<'_>>;
}

/// Rasterise a frame plan into `target`.
///
/// Effects are **not** applied by the reference rasteriser; shader effects are
/// the platform layer's responsibility. The reference implementation is used
/// to validate compositing, transforms and blend modes.
pub fn execute(plan: &FramePlan, target: &mut FrameBuffer, provider: &dyn FrameProvider) {
    for step in &plan.steps {
        match step {
            DrawStep::Clear { color } => {
                target.pixels.fill(*color);
            }
            DrawStep::Composite {
                source,
                transform,
                src_rect,
                dst_rect,
                blend,
                opacity,
                ..
            } => {
                let Some(view) = provider.frame_for(source) else {
                    continue;
                };
                let Some(inverse) = transform.inverse() else {
                    continue;
                };
                draw_quad(
                    target, view, &inverse, *src_rect, *dst_rect, *blend, *opacity,
                );
            }
            DrawStep::Transition {
                from_source,
                to_source,
                transform,
                src_rect,
                dst_rect,
                kind,
                progress,
                ..
            } => {
                let Some(from) = provider.frame_for(from_source) else {
                    continue;
                };
                let Some(to) = provider.frame_for(to_source) else {
                    continue;
                };
                let Some(inverse) = transform.inverse() else {
                    continue;
                };
                draw_transition(
                    target, from, to, &inverse, *src_rect, *dst_rect, *kind, *progress,
                );
            }
        }
    }
}

/// Draw a transformed, blended quad.
#[allow(clippy::too_many_arguments)]
fn draw_quad(
    target: &mut FrameBuffer,
    view: FrameView<'_>,
    inverse: &vimo_foundation::math::Mat3,
    src_rect: Rect,
    dst_rect: Rect,
    blend: BlendMode,
    opacity: f32,
) {
    let x0 = (dst_rect.x * target.width as f32).round() as i64;
    let x1 = ((dst_rect.x + dst_rect.w) * target.width as f32).round() as i64;
    let y0 = (dst_rect.y * target.height as f32).round() as i64;
    let y1 = ((dst_rect.y + dst_rect.h) * target.height as f32).round() as i64;

    for py in y0.max(0)..y1.min(target.height as i64) {
        for px in x0.max(0)..x1.min(target.width as i64) {
            let u = (px as f32 + 0.5) / target.width as f32;
            let v = (py as f32 + 0.5) / target.height as f32;
            let p = inverse.transform_vec3(vimo_foundation::math::Vec3::new(u, v, 1.0));
            // Map the layer-space position into the source rectangle.
            let su = src_rect.x + (p.x - src_rect.x) * src_rect.w;
            let sv = src_rect.y + (p.y - src_rect.y) * src_rect.h;
            if su < 0.0 || su > 1.0 || sv < 0.0 || sv > 1.0 {
                continue;
            }
            let color = view.sample_bilinear(su, sv);
            let dst = target
                .pixel(px as u32, py as u32)
                .map(|c| c.to_f32())
                .unwrap_or([0.0; 4]);
            let blended = blend_pixel(blend, color, dst, opacity);
            target.set_pixel(
                px as u32,
                py as u32,
                Rgba8::new(
                    to_u8(blended[0]),
                    to_u8(blended[1]),
                    to_u8(blended[2]),
                    to_u8(blended[3]),
                ),
            );
        }
    }
}

/// Draw a transition between two pictures.
#[allow(clippy::too_many_arguments)]
fn draw_transition(
    target: &mut FrameBuffer,
    from: FrameView<'_>,
    to: FrameView<'_>,
    inverse: &vimo_foundation::math::Mat3,
    src_rect: Rect,
    dst_rect: Rect,
    kind: vimo_project::model::TransitionKind,
    progress: f32,
) {
    let x0 = (dst_rect.x * target.width as f32).round() as i64;
    let x1 = ((dst_rect.x + dst_rect.w) * target.width as f32).round() as i64;
    let y0 = (dst_rect.y * target.height as f32).round() as i64;
    let y1 = ((dst_rect.y + dst_rect.h) * target.height as f32).round() as i64;

    for py in y0.max(0)..y1.min(target.height as i64) {
        for px in x0.max(0)..x1.min(target.width as i64) {
            let u = (px as f32 + 0.5) / target.width as f32;
            let v = (py as f32 + 0.5) / target.height as f32;
            let p = inverse.transform_vec3(vimo_foundation::math::Vec3::new(u, v, 1.0));
            let su = src_rect.x + (p.x - src_rect.x) * src_rect.w;
            let sv = src_rect.y + (p.y - src_rect.y) * src_rect.h;
            if su < 0.0 || su > 1.0 || sv < 0.0 || sv > 1.0 {
                continue;
            }
            let a = from.sample_bilinear(su, sv);
            let b = to.sample_bilinear(su, sv);
            let color = match kind {
                vimo_project::model::TransitionKind::DipToBlack => {
                    let m = 1.0 - (2.0 * progress - 1.0).abs();
                    let mut c = [0.0f32; 4];
                    for i in 0..4 {
                        c[i] = a[i] * (1.0 - m) + b[i] * m;
                    }
                    c
                }
                _ => {
                    let mut c = [0.0f32; 4];
                    for i in 0..4 {
                        c[i] = a[i] * (1.0 - progress) + b[i] * progress;
                    }
                    c
                }
            };
            let dst = target
                .pixel(px as u32, py as u32)
                .map(|c| c.to_f32())
                .unwrap_or([0.0; 4]);
            let blended = blend_pixel(BlendMode::Normal, color, dst, 1.0);
            target.set_pixel(
                px as u32,
                py as u32,
                Rgba8::new(
                    to_u8(blended[0]),
                    to_u8(blended[1]),
                    to_u8(blended[2]),
                    to_u8(blended[3]),
                ),
            );
        }
    }
}

/// Blend a source colour over a destination colour.
#[must_use]
pub fn blend_pixel(blend: BlendMode, src: [f32; 4], dst: [f32; 4], opacity: f32) -> [f32; 4] {
    let a = (src[3] * opacity).clamp(0.0, 1.0);
    let mut out = [0.0f32; 4];
    for i in 0..3 {
        let s = src[i];
        let d = dst[i];
        out[i] = match blend {
            BlendMode::Normal => s * a + d * (1.0 - a),
            BlendMode::Add => (s * a + d).min(1.0),
            BlendMode::Screen => (s + d - s * d).min(1.0),
            BlendMode::Multiply => (s * d).min(1.0),
            BlendMode::Overlay => {
                if d <= 0.5 {
                    (2.0 * s * d).min(1.0)
                } else {
                    (1.0 - 2.0 * (1.0 - s) * (1.0 - d)).max(0.0)
                }
            }
            BlendMode::HardLight => {
                if s <= 0.5 {
                    (2.0 * s * d).min(1.0)
                } else {
                    (1.0 - 2.0 * (1.0 - s) * (1.0 - d)).max(0.0)
                }
            }
            BlendMode::SoftLight => soft_light(s, d),
            BlendMode::Lighten => s.max(d),
            BlendMode::Darken => s.min(d),
            BlendMode::ColorDodge => {
                if s >= 1.0 {
                    1.0
                } else {
                    (d / (1.0 - s)).min(1.0)
                }
            }
            BlendMode::ColorBurn => {
                if s <= 0.0 {
                    0.0
                } else {
                    (1.0 - (1.0 - d) / s).max(0.0)
                }
            }
            BlendMode::LinearDodge => (s + d).min(1.0),
            BlendMode::LinearBurn => (s + d - 1.0).max(0.0),
            BlendMode::Difference => (s - d).abs(),
            BlendMode::Exclusion => (s + d - 2.0 * s * d).max(0.0),
            // Non-separable modes need a shader; fall back to "over".
            _ => s * a + d * (1.0 - a),
        };
    }
    out[3] = a + dst[3] * (1.0 - a);
    out
}

fn soft_light(s: f32, d: f32) -> f32 {
    if s <= 0.5 {
        d - (1.0 - 2.0 * s) * d * (1.0 - d)
    } else {
        let g = if d <= 0.25 {
            ((16.0 * d - 12.0) * d + 4.0) * d
        } else {
            d.sqrt()
        };
        d + (2.0 * s - 1.0) * (g - d)
    }
}

#[inline]
fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// The result of comparing two framebuffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffStats {
    /// Number of pixels that differ.
    pub differing_pixels: usize,
    /// The largest per-channel difference.
    pub max_channel_delta: u8,
}

/// Compare two framebuffers of the same size.
#[must_use]
pub fn diff(a: &FrameBuffer, b: &FrameBuffer) -> DiffStats {
    if a.width != b.width || a.height != b.height {
        return DiffStats {
            differing_pixels: a.pixels.len().max(b.pixels.len()),
            max_channel_delta: 255,
        };
    }
    let mut differing = 0usize;
    let mut max_delta = 0u8;
    for (pa, pb) in a.pixels.iter().zip(b.pixels.iter()) {
        let d =
            pa.r.abs_diff(pb.r)
                .max(pa.g.abs_diff(pb.g))
                .max(pa.b.abs_diff(pb.b))
                .max(pa.a.abs_diff(pb.a));
        if d != 0 {
            differing += 1;
            max_delta = max_delta.max(d);
        }
    }
    DiffStats {
        differing_pixels: differing,
        max_channel_delta: max_delta,
    }
}

/// Write a framebuffer as a binary PPM (`P6`).
///
/// PPM keeps the golden files dependency free and diffable.
pub fn write_ppm(fb: &FrameBuffer, path: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;
    let mut out = Vec::with_capacity(fb.pixels.len() * 3 + 32);
    out.extend_from_slice(format!("P6\n{} {}\n255\n", fb.width, fb.height).as_bytes());
    for p in &fb.pixels {
        out.push(p.r);
        out.push(p.g);
        out.push(p.b);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(path)?;
    file.write_all(&out)?;
    Ok(())
}

/// Read a binary PPM (`P6`) file.
pub fn read_ppm(path: &std::path::Path) -> std::io::Result<FrameBuffer> {
    let data = std::fs::read(path)?;
    let mut pos = 0usize;
    let magic = next_token(&data, &mut pos)
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    if magic != "P6" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "not a P6 PPM",
        ));
    }
    let width: u32 = next_token(&data, &mut pos)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    let height: u32 = next_token(&data, &mut pos)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    let _maxval = next_token(&data, &mut pos);
    // Exactly one whitespace byte separates the header from the data.
    if pos < data.len() {
        pos += 1;
    }
    let count = (width as usize) * (height as usize);
    if data.len() < pos + count * 3 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "truncated PPM",
        ));
    }
    let mut pixels = Vec::with_capacity(count);
    for i in 0..count {
        let o = pos + i * 3;
        pixels.push(Rgba8::new(data[o], data[o + 1], data[o + 2], 255));
    }
    Ok(FrameBuffer {
        width,
        height,
        pixels,
    })
}

/// Read the next whitespace-delimited token, skipping comments.
fn next_token(data: &[u8], pos: &mut usize) -> Option<String> {
    loop {
        while *pos < data.len() && data[*pos].is_ascii_whitespace() {
            *pos += 1;
        }
        if *pos < data.len() && data[*pos] == b'#' {
            while *pos < data.len() && data[*pos] != b'\n' {
                *pos += 1;
            }
            continue;
        }
        break;
    }
    let start = *pos;
    while *pos < data.len() && !data[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
    if start == *pos {
        return None;
    }
    Some(String::from_utf8_lossy(&data[start..*pos]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_blend_composites_over() {
        let out = blend_pixel(
            BlendMode::Normal,
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
            1.0,
        );
        assert_eq!(out[0], 1.0);
        assert_eq!(out[2], 0.0);
    }

    #[test]
    fn multiply_blend_darkens() {
        let out = blend_pixel(
            BlendMode::Multiply,
            [0.5, 0.5, 0.5, 1.0],
            [0.5, 0.5, 0.5, 1.0],
            1.0,
        );
        assert!((out[0] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn screen_blend_lightens() {
        let out = blend_pixel(
            BlendMode::Screen,
            [0.5, 0.5, 0.5, 1.0],
            [0.5, 0.5, 0.5, 1.0],
            1.0,
        );
        assert!((out[0] - 0.75).abs() < 1e-6);
    }

    #[test]
    fn ppm_roundtrip() {
        let fb = FrameBuffer::filled(4, 3, Rgba8::new(10, 20, 30, 255));
        let dir = std::env::temp_dir().join(format!("vimo-ppm-{}", std::process::id()));
        let path = dir.join("t.ppm");
        write_ppm(&fb, &path).unwrap();
        let back = read_ppm(&path).unwrap();
        assert_eq!(back.width, 4);
        assert_eq!(back.height, 3);
        assert_eq!(back.pixel(1, 1), Some(Rgba8::new(10, 20, 30, 255)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_detects_changes() {
        let a = FrameBuffer::filled(2, 2, Rgba8::new(0, 0, 0, 255));
        let mut b = a.clone();
        b.set_pixel(0, 0, Rgba8::new(255, 0, 0, 255));
        let d = diff(&a, &b);
        assert_eq!(d.differing_pixels, 1);
        assert_eq!(d.max_channel_delta, 255);
    }
}
