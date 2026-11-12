//! The Vimo project file format.
//!
//! # Layout
//!
//! ```text
//! magic    "VIMO"      4 bytes
//! version  u32         little endian
//! flags    u32         reserved, must be 0
//! sections...          repeated until end of file
//!
//! section := tag u8 | length u32 | payload[length]
//! ```
//!
//! Sections are self-describing and may appear in any order. A reader that
//! does not understand a tag skips its payload, which is what makes it
//! possible to add new sections without breaking older builds.
//!
//! # Version history
//!
//! | Version | Change |
//! |---------|--------|
//! | 1 | Initial format. Clip opacity stored as a `u8` percentage, no clip speed, no transitions, effect parameters were floats only. |
//! | 2 | Opacity became `f32` in `0..=1`, clips gained `speed` and `forward`, transitions were added. |
//! | 3 | Effect parameters became typed, clips gained a transform, keyframes gained bezier easing. |
//!
//! Older files are migrated forward by [`crate::migrate::migrate`].

pub mod reader;
pub mod writer;

use vimo_foundation::color::Rgba8;
use vimo_foundation::error::{malformed, ErrorKind, VimoError, VimoResult};
use vimo_foundation::math::{Rect, Vec2};
use vimo_foundation::time::{Rational, Time, TimeRange, Timebase};
use vimo_manifest::{EffectRegistry, EffectTypeId, ParamValue};

use crate::anim::{AnimTrack, Interpolation, Keyframe};
use crate::model::{
    Asset, AssetKind, BlendMode, Clip, EffectId, EffectNode, EffectOwner, Track, TrackKind,
    Transform, Transition, TransitionKind,
};
use crate::project::Project;

use reader::Reader;
use writer::Writer;

/// File magic.
pub const MAGIC: [u8; 4] = *b"VIMO";
/// The format version this build writes.
pub const CURRENT_VERSION: u32 = 3;
/// The oldest version this build can read.
pub const OLDEST_SUPPORTED_VERSION: u32 = 1;

const SECTION_META: u8 = 1;
const SECTION_ASSETS: u8 = 2;
const SECTION_TRACKS: u8 = 3;
const SECTION_CLIPS: u8 = 4;
const SECTION_EFFECTS: u8 = 5;
const SECTION_TRANSITIONS: u8 = 6;

const PT_FLOAT: u8 = 0;
const PT_INT: u8 = 1;
const PT_BOOL: u8 = 2;
const PT_COLOR: u8 = 3;
const PT_VEC2: u8 = 4;
const PT_VEC3: u8 = 5;
const PT_ENUM: u8 = 6;

const INTERP_LINEAR: u8 = 0;
const INTERP_HOLD: u8 = 1;
const INTERP_BEZIER: u8 = 2;

/// What happened while loading a project.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadReport {
    /// The version the file was written with.
    pub source_version: u32,
    /// The version the file was migrated to, when a migration ran.
    pub migrated_to: Option<u32>,
    /// Effect ids referenced by the file that are not in the registry.
    pub dropped_effects: Vec<String>,
    /// Number of parameter values coerced to their declared type.
    pub coerced_params: usize,
    /// Number of dead entities skipped.
    pub skipped_dead: usize,
}

/// Serialise a project.
///
/// `version` selects the on-disk layout; use [`CURRENT_VERSION`] unless you are
/// producing a file for an older build.
pub fn write(project: &Project, registry: &EffectRegistry, version: u32) -> VimoResult<Vec<u8>> {
    if version == 0 || version > CURRENT_VERSION {
        return Err(VimoError::detailed(
            ErrorKind::InvalidArgument,
            "codec",
            format!("cannot write format version {version}"),
        ));
    }

    let mut out = Writer::with_capacity(4096);
    out.bytes(&MAGIC);
    out.u32(version);
    out.u32(0); // flags

    // --- meta -----------------------------------------------------------
    let mut meta = Writer::new();
    meta.str(&project.meta.name);
    meta.i64(project.meta.timebase.fps().num());
    meta.i64(project.meta.timebase.fps().den());
    meta.u32(project.meta.width);
    meta.u32(project.meta.height);
    meta.bytes(&project.meta.background.to_u32().to_le_bytes());
    meta.i64(project.meta.duration.as_nanos());
    meta.bool(project.meta.timebase.is_drop_frame());
    out.section(SECTION_META, meta.as_slice());

    // --- assets ---------------------------------------------------------
    let mut assets = Writer::new();
    assets.u32(project.assets.len() as u32);
    for a in &project.assets {
        assets.str(&a.source_id);
        assets.str(&a.name);
        assets.u8(match a.kind {
            AssetKind::Video => 0,
            AssetKind::Audio => 1,
            AssetKind::Image => 2,
            AssetKind::Generated => 3,
        });
        assets.i64(a.duration.as_nanos());
        assets.i64(a.timebase.fps().num());
        assets.i64(a.timebase.fps().den());
        assets.u32(a.width);
        assets.u32(a.height);
        assets.bool(a.has_audio);
        assets.u32(a.rotation);
        assets.bytes(&a.label_color.to_u32().to_le_bytes());
        assets.bool(a.variable_frame_rate);
        assets.bool(a.hdr);
        assets.bool(a.alive);
    }
    out.section(SECTION_ASSETS, assets.as_slice());

    // --- tracks ---------------------------------------------------------
    let mut tracks = Writer::new();
    tracks.u32(project.tracks.len() as u32);
    for t in &project.tracks {
        tracks.str(&t.name);
        tracks.u8(match t.kind {
            TrackKind::Video => 0,
            TrackKind::Audio => 1,
        });
        tracks.bool(t.enabled);
        tracks.bool(t.locked);
        tracks.bool(t.muted);
        tracks.bool(t.solo);
        tracks.f32(t.volume);
        tracks.f32(t.height);
        tracks.u64(t.revision);
        tracks.bool(t.alive);
        tracks.u32(t.clips.len() as u32);
        for c in &t.clips {
            tracks.u32(*c);
        }
        tracks.u32(t.effects.len() as u32);
        for e in &t.effects {
            tracks.u32(*e);
        }
    }
    out.section(SECTION_TRACKS, tracks.as_slice());

    // --- clips ----------------------------------------------------------
    let mut clips = Writer::new();
    clips.u32(project.clips.len() as u32);
    for c in &project.clips {
        clips.u32(c.asset);
        clips.i64(c.timeline.start.as_nanos());
        clips.i64(c.timeline.end.as_nanos());
        clips.i64(c.source_in.as_nanos());
        if version == 1 {
            // v1 stored opacity as an integer percentage.
            clips.u8((c.opacity.clamp(0.0, 1.0) * 100.0).round() as u8);
        } else {
            clips.f32(c.opacity);
        }
        clips.u8(c.blend.as_u8());
        if version >= 2 {
            clips.f32(c.speed);
            clips.bool(c.forward);
        }
        if version >= 3 {
            write_transform(&mut clips, &c.transform);
        }
        clips.bool(c.enabled);
        clips.bytes(&c.label_color.to_u32().to_le_bytes());
        clips.f32(c.volume);
        clips.bool(c.muted);
        clips.u64(c.revision);
        clips.bool(c.alive);
        clips.u32(c.effects.len() as u32);
        for e in &c.effects {
            clips.u32(*e);
        }
    }
    out.section(SECTION_CLIPS, clips.as_slice());

    // --- effects --------------------------------------------------------
    let mut effects = Writer::new();
    effects.u32(project.effects.len() as u32);
    for e in &project.effects {
        let type_id = registry
            .get(e.effect_type)
            .map(|d| d.id.clone())
            .unwrap_or_default();
        effects.str(&type_id);
        effects.bool(e.enabled);
        effects.u64(e.revision);
        effects.bool(e.alive);
        write_params(&mut effects, &e.params, version);
        effects.u32(e.animation.len() as u32);
        for track in &e.animation {
            effects.u16(track.param);
            effects.u32(track.keys.len() as u32);
            for k in &track.keys {
                effects.i64(k.time.as_nanos());
                write_param_value(&mut effects, k.value, version);
                write_interpolation(&mut effects, k.interpolation);
            }
        }
    }
    out.section(SECTION_EFFECTS, effects.as_slice());

    // --- transitions ----------------------------------------------------
    if version >= 2 {
        let mut tr = Writer::new();
        tr.u32(project.transitions.len() as u32);
        for t in &project.transitions {
            tr.u8(t.kind.as_u8());
            tr.u32(t.track);
            tr.u32(t.from_clip);
            tr.u32(t.to_clip);
            tr.i64(t.duration.as_nanos());
            tr.bool(t.alive);
            write_params(&mut tr, &t.params, version);
        }
        out.section(SECTION_TRANSITIONS, tr.as_slice());
    }

    Ok(out.finish())
}

fn write_transform(w: &mut Writer, t: &Transform) {
    w.f32(t.position.x);
    w.f32(t.position.y);
    w.f32(t.scale.x);
    w.f32(t.scale.y);
    w.f32(t.rotation);
    w.f32(t.anchor.x);
    w.f32(t.anchor.y);
    w.f32(t.crop.x);
    w.f32(t.crop.y);
    w.f32(t.crop.w);
    w.f32(t.crop.h);
}

fn read_transform(r: &mut Reader) -> VimoResult<Transform> {
    Ok(Transform {
        position: Vec2::new(r.f32()?, r.f32()?),
        scale: Vec2::new(r.f32()?, r.f32()?),
        rotation: r.f32()?,
        anchor: Vec2::new(r.f32()?, r.f32()?),
        crop: Rect::new(r.f32()?, r.f32()?, r.f32()?, r.f32()?),
    })
}

fn param_tag(v: ParamValue) -> u8 {
    match v {
        ParamValue::Float(_) => PT_FLOAT,
        ParamValue::Int(_) => PT_INT,
        ParamValue::Bool(_) => PT_BOOL,
        ParamValue::Color(_) => PT_COLOR,
        ParamValue::Vec2(_) => PT_VEC2,
        ParamValue::Vec3(_) => PT_VEC3,
        ParamValue::Enum(_) => PT_ENUM,
    }
}

fn write_param_value(w: &mut Writer, v: ParamValue, version: u32) {
    if version < 3 {
        w.f32(v.as_f32().unwrap_or(0.0));
        return;
    }
    w.u8(param_tag(v));
    match v {
        ParamValue::Float(f) => w.f32(f),
        ParamValue::Int(i) => w.i32(i),
        ParamValue::Bool(b) => w.bool(b),
        ParamValue::Color(c) => w.bytes(&c.to_u32().to_le_bytes()),
        ParamValue::Vec2(v) => {
            w.f32(v.x);
            w.f32(v.y);
        }
        ParamValue::Vec3(v) => {
            w.f32(v.x);
            w.f32(v.y);
            w.f32(v.z);
        }
        ParamValue::Enum(e) => w.u16(e),
    }
}

fn read_param_value(r: &mut Reader, version: u32) -> VimoResult<ParamValue> {
    if version < 3 {
        return Ok(ParamValue::Float(r.f32()?));
    }
    let tag = r.u8()?;
    Ok(match tag {
        PT_FLOAT => ParamValue::Float(r.f32()?),
        PT_INT => ParamValue::Int(r.i32()?),
        PT_BOOL => ParamValue::Bool(r.bool()?),
        PT_COLOR => {
            let b = r.bytes(4)?;
            ParamValue::Color(Rgba8::from_u32(u32::from_le_bytes([
                b[0], b[1], b[2], b[3],
            ])))
        }
        PT_VEC2 => ParamValue::Vec2(Vec2::new(r.f32()?, r.f32()?)),
        PT_VEC3 => ParamValue::Vec3(vimo_foundation::math::Vec3::new(
            r.f32()?,
            r.f32()?,
            r.f32()?,
        )),
        PT_ENUM => ParamValue::Enum(r.u16()?),
        other => {
            return Err(VimoError::detailed(
                ErrorKind::Format,
                "codec",
                format!("unknown parameter type tag {other}"),
            ))
        }
    })
}

fn write_params(w: &mut Writer, params: &vimo_manifest::ParamStore, version: u32) {
    w.u32(params.len() as u32);
    for (index, value) in params.iter_overridden() {
        w.u16(index);
        write_param_value(w, value, version);
    }
}

fn read_params(
    r: &mut Reader,
    version: u32,
    registry: &EffectRegistry,
    effect_type: Option<EffectTypeId>,
) -> VimoResult<(vimo_manifest::ParamStore, usize)> {
    let count = r.u32()?;
    let desc = effect_type.and_then(|t| registry.get(t));
    let mut values: Vec<ParamValue> = desc
        .map(|d| d.params.iter().map(|p| p.default).collect())
        .unwrap_or_default();
    for _ in 0..count {
        let index = r.u16()?;
        let value = read_param_value(r, version)?;
        if values.len() <= index as usize {
            values.resize(index as usize + 1, ParamValue::Float(0.0));
        }
        values[index as usize] = value;
    }
    // Everything present in the file is an explicit override of the default.
    let mut store = vimo_manifest::ParamStore::from_values(values);
    for i in 0..store.len() {
        store.set(
            i as u16,
            store.get(i as u16).unwrap_or(ParamValue::Float(0.0)),
        );
    }
    Ok((store, 0))
}

fn write_interpolation(w: &mut Writer, i: Interpolation) {
    match i {
        Interpolation::Linear => w.u8(INTERP_LINEAR),
        Interpolation::Hold => w.u8(INTERP_HOLD),
        Interpolation::Bezier { x1, y1, x2, y2 } => {
            w.u8(INTERP_BEZIER);
            w.f32(x1);
            w.f32(y1);
            w.f32(x2);
            w.f32(y2);
        }
    }
}

fn read_interpolation(r: &mut Reader) -> VimoResult<Interpolation> {
    Ok(match r.u8()? {
        INTERP_LINEAR => Interpolation::Linear,
        INTERP_HOLD => Interpolation::Hold,
        INTERP_BEZIER => Interpolation::Bezier {
            x1: r.f32()?,
            y1: r.f32()?,
            x2: r.f32()?,
            y2: r.f32()?,
        },
        other => {
            return Err(VimoError::detailed(
                ErrorKind::Format,
                "codec",
                format!("unknown interpolation tag {other}"),
            ))
        }
    })
}

/// Parse a project from bytes.
///
/// `registry` is used to resolve effect type ids (the file stores the string
/// id) and to coerce parameter values to their declared types.
pub fn read(bytes: &[u8], registry: &EffectRegistry) -> VimoResult<(Project, LoadReport)> {
    let mut r = Reader::new(bytes);
    if r.bytes(4)? != MAGIC {
        return Err(malformed("project: bad magic"));
    }
    let version = r.u32()?;
    let _flags = r.u32()?;
    if version < OLDEST_SUPPORTED_VERSION {
        return Err(VimoError::detailed(
            ErrorKind::Unsupported,
            "project",
            format!("format version {version} is too old"),
        ));
    }
    if version > CURRENT_VERSION {
        return Err(VimoError::detailed(
            ErrorKind::Unsupported,
            "project",
            format!(
                "format version {version} is newer than this build supports ({CURRENT_VERSION})"
            ),
        ));
    }

    // Collect the sections first so that they may appear in any order.
    let mut sections: Vec<(u8, &[u8])> = Vec::new();
    while !r.is_empty() {
        let tag = r.u8()?;
        let len = r.u32()? as usize;
        let payload = r.bytes(len)?;
        sections.push((tag, payload));
    }

    let mut project = Project::new();
    let mut report = LoadReport {
        source_version: version,
        ..Default::default()
    };

    // --- meta -----------------------------------------------------------
    for (tag, payload) in &sections {
        if *tag != SECTION_META {
            continue;
        }
        let mut m = Reader::new(payload);
        let name = m.str()?;
        let num = m.i64()?;
        let den = m.i64()?;
        let width = m.u32()?;
        let height = m.u32()?;
        let bg = m.bytes(4)?;
        let background = Rgba8::from_u32(u32::from_le_bytes([bg[0], bg[1], bg[2], bg[3]]));
        let duration = Time::from_nanos(m.i64()?);
        let drop_frame = m.bool()?;
        let fps = Rational::new(num, den).unwrap_or(Rational::ONE);
        project.meta = crate::model::ProjectMeta {
            name,
            timebase: Timebase::with_drop_frame(fps, drop_frame),
            width,
            height,
            background,
            duration,
        };
    }

    // --- assets ---------------------------------------------------------
    for (tag, payload) in &sections {
        if *tag != SECTION_ASSETS {
            continue;
        }
        let mut a = Reader::new(payload);
        let count = a.u32()?;
        for _ in 0..count {
            let source_id = a.str()?;
            let name = a.str()?;
            let kind = match a.u8()? {
                1 => AssetKind::Audio,
                2 => AssetKind::Image,
                3 => AssetKind::Generated,
                _ => AssetKind::Video,
            };
            let duration = Time::from_nanos(a.i64()?);
            let num = a.i64()?;
            let den = a.i64()?;
            let width = a.u32()?;
            let height = a.u32()?;
            let has_audio = a.bool()?;
            let rotation = a.u32()?;
            let lc = a.bytes(4)?;
            let label_color = Rgba8::from_u32(u32::from_le_bytes([lc[0], lc[1], lc[2], lc[3]]));
            let variable_frame_rate = a.bool()?;
            let hdr = a.bool()?;
            let alive = a.bool()?;
            project.assets.push(Asset {
                source_id,
                name,
                kind,
                duration,
                timebase: Timebase::with_drop_frame(
                    Rational::new(num, den).unwrap_or(Rational::ONE),
                    false,
                ),
                width,
                height,
                has_audio,
                rotation,
                label_color,
                variable_frame_rate,
                hdr,
                alive,
            });
        }
    }

    // --- effects (resolved first so clips can reference them) -----------
    let mut effect_map: Vec<Option<EffectId>> = Vec::new();
    for (tag, payload) in &sections {
        if *tag != SECTION_EFFECTS {
            continue;
        }
        let mut e = Reader::new(payload);
        let count = e.u32()?;
        for _ in 0..count {
            let type_id_str = e.str()?;
            let enabled = e.bool()?;
            let revision = e.u64()?;
            let alive = e.bool()?;
            let resolved = registry.lookup(&type_id_str);
            if resolved.is_none() && !type_id_str.is_empty() {
                report.dropped_effects.push(type_id_str.clone());
            }
            let effect_type = resolved.unwrap_or(EffectTypeId::from_index(u32::MAX));
            let (params, coerced) = read_params(&mut e, version, registry, resolved)?;
            report.coerced_params += coerced;
            let anim_count = e.u32()?;
            let mut animation = Vec::with_capacity(anim_count as usize);
            for _ in 0..anim_count {
                let param = e.u16()?;
                let key_count = e.u32()?;
                let mut keys = Vec::with_capacity(key_count as usize);
                for _ in 0..key_count {
                    let time = Time::from_nanos(e.i64()?);
                    let value = read_param_value(&mut e, version)?;
                    let interpolation = read_interpolation(&mut e)?;
                    keys.push(Keyframe {
                        time,
                        value,
                        interpolation,
                    });
                }
                animation.push(AnimTrack { param, keys });
            }
            if resolved.is_none() {
                effect_map.push(None);
                continue;
            }
            let id = project.effects.len() as EffectId;
            project.effects.push(EffectNode {
                effect_type,
                params,
                enabled,
                animation,
                revision,
                alive,
            });
            effect_map.push(Some(id));
        }
    }

    // --- tracks ---------------------------------------------------------
    for (tag, payload) in &sections {
        if *tag != SECTION_TRACKS {
            continue;
        }
        let mut t = Reader::new(payload);
        let count = t.u32()?;
        for _ in 0..count {
            let name = t.str()?;
            let kind = match t.u8()? {
                1 => TrackKind::Audio,
                _ => TrackKind::Video,
            };
            let enabled = t.bool()?;
            let locked = t.bool()?;
            let muted = t.bool()?;
            let solo = t.bool()?;
            let volume = t.f32()?;
            let height = t.f32()?;
            let revision = t.u64()?;
            let alive = t.bool()?;
            let clip_ids = t.u32_vec()?;
            let effect_ids = t.u32_vec()?;
            project.tracks.push(Track {
                name,
                kind,
                enabled,
                locked,
                muted,
                solo,
                volume,
                clips: clip_ids,
                effects: effect_ids
                    .into_iter()
                    .filter_map(|e| effect_map.get(e as usize).copied().flatten())
                    .collect(),
                height,
                revision,
                alive,
            });
        }
    }

    // --- clips ----------------------------------------------------------
    for (tag, payload) in &sections {
        if *tag != SECTION_CLIPS {
            continue;
        }
        let mut c = Reader::new(payload);
        let count = c.u32()?;
        for _ in 0..count {
            let asset = c.u32()?;
            let start = Time::from_nanos(c.i64()?);
            let end = Time::from_nanos(c.i64()?);
            let source_in = Time::from_nanos(c.i64()?);
            let opacity = if version == 1 {
                c.u8()? as f32 / 100.0
            } else {
                c.f32()?
            };
            let blend = BlendMode::from_u8(c.u8()?);
            let (speed, forward) = if version >= 2 {
                (c.f32()?, c.bool()?)
            } else {
                (1.0, true)
            };
            let transform = if version >= 3 {
                read_transform(&mut c)?
            } else {
                Transform::default()
            };
            let enabled = c.bool()?;
            let lc = c.bytes(4)?;
            let label_color = Rgba8::from_u32(u32::from_le_bytes([lc[0], lc[1], lc[2], lc[3]]));
            let volume = c.f32()?;
            let muted = c.bool()?;
            let revision = c.u64()?;
            let alive = c.bool()?;
            let effect_ids = c.u32_vec()?;
            project.clips.push(Clip {
                asset,
                timeline: TimeRange::new(start, end),
                source_in,
                speed,
                forward,
                opacity,
                blend,
                transform,
                enabled,
                effects: effect_ids
                    .into_iter()
                    .filter_map(|e| effect_map.get(e as usize).copied().flatten())
                    .collect(),
                label_color,
                volume,
                muted,
                revision,
                alive,
            });
        }
    }

    // --- transitions ----------------------------------------------------
    for (tag, payload) in &sections {
        if *tag != SECTION_TRANSITIONS {
            continue;
        }
        let mut t = Reader::new(payload);
        let count = t.u32()?;
        for _ in 0..count {
            let kind = TransitionKind::from_u8(t.u8()?);
            let track = t.u32()?;
            let from_clip = t.u32()?;
            let to_clip = t.u32()?;
            let duration = Time::from_nanos(t.i64()?);
            let alive = t.bool()?;
            let (params, coerced) = read_params(&mut t, version, registry, None)?;
            report.coerced_params += coerced;
            project.transitions.push(Transition {
                kind,
                track,
                from_clip,
                to_clip,
                duration,
                params,
                alive,
            });
        }
    }

    // --- migrations -----------------------------------------------------
    let report = crate::migrate::migrate(&mut project, version, registry, report)?;

    project.revision = 1;
    Ok((project, report))
}

/// Resolve the effect owner of an effect id inside a project.
#[must_use]
pub fn owner_of(project: &Project, effect: EffectId) -> Option<EffectOwner> {
    for (i, c) in project.clips.iter().enumerate() {
        if c.effects.contains(&effect) {
            return Some(EffectOwner::Clip(i as EffectId));
        }
    }
    for (i, t) in project.tracks.iter().enumerate() {
        if t.effects.contains(&effect) {
            return Some(EffectOwner::Track(i as EffectId));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Asset, TrackKind};

    fn registry() -> EffectRegistry {
        let mut r = EffectRegistry::new();
        let desc = vimo_manifest::EffectDesc::new(
            "com.vimo.test",
            "Test",
            "test",
            1,
            None,
            vec![vimo_manifest::ParamDesc {
                name: "gain".into(),
                ptype: vimo_manifest::ParamType::Float,
                default: ParamValue::Float(1.0),
                min: None,
                max: None,
                step: None,
                label: "Gain".into(),
                animatable: true,
                options: Vec::new(),
            }],
            Default::default(),
        )
        .unwrap();
        r.register(desc).unwrap();
        r
    }

    fn sample() -> Project {
        let mut p = Project::new();
        p.assets.push(Asset::video("src-0", Time::from_secs(10)));
        let mut t = Track::new("V1", TrackKind::Video);
        t.clips.push(0);
        p.tracks.push(t);
        p.clips.push(Clip::new(
            0,
            TimeRange::new(Time::from_secs(1), Time::from_secs(3)),
            Time::ZERO,
        ));
        p
    }

    #[test]
    fn roundtrip_current_version() {
        let reg = registry();
        let p = sample();
        let bytes = write(&p, &reg, CURRENT_VERSION).unwrap();
        let (q, report) = read(&bytes, &reg).unwrap();
        assert_eq!(report.source_version, CURRENT_VERSION);
        assert_eq!(q.clips.len(), p.clips.len());
        assert_eq!(q.clips[0].timeline, p.clips[0].timeline);
        assert_eq!(q.tracks[0].clips, p.tracks[0].clips);
        assert_eq!(q.meta.width, p.meta.width);
    }

    #[test]
    fn roundtrip_v1_migrates_forward() {
        let reg = registry();
        let mut p = sample();
        p.clips[0].opacity = 0.5;
        let bytes = write(&p, &reg, 1).unwrap();
        let (q, report) = read(&bytes, &reg).unwrap();
        assert_eq!(report.source_version, 1);
        assert_eq!(report.migrated_to, Some(CURRENT_VERSION));
        // v1 stored opacity as an integer percentage, so 0.5 round-trips as 0.5.
        assert!(
            (q.clips[0].opacity - 0.5).abs() < 0.011,
            "opacity={}",
            q.clips[0].opacity
        );
    }

    #[test]
    fn rejects_bad_magic() {
        let reg = registry();
        assert!(read(b"NOPE0000", &reg).is_err());
    }

    #[test]
    fn rejects_future_version() {
        let reg = registry();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&(CURRENT_VERSION + 1).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        assert!(read(&bytes, &reg).is_err());
    }

    #[test]
    fn truncated_file_is_an_error() {
        let reg = registry();
        let p = sample();
        let bytes = write(&p, &reg, CURRENT_VERSION).unwrap();
        for cut in [4usize, 8, 16, bytes.len() - 1] {
            assert!(
                read(&bytes[..cut], &reg).is_err(),
                "cut at {cut} should fail"
            );
        }
    }

    #[test]
    fn unknown_sections_are_skipped() {
        let reg = registry();
        let p = sample();
        let mut bytes = write(&p, &reg, CURRENT_VERSION).unwrap();
        // Append an unknown section with a garbage payload.
        bytes.push(200);
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3, 4, 5]);
        let (q, _) = read(&bytes, &reg).unwrap();
        assert_eq!(q.clips.len(), 1);
    }
}
