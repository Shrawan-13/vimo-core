//! A JSON snapshot of a project.
//!
//! The UI needs to *read* the project to draw a timeline, but the engine's
//! contract is "commands in, events out". A snapshot is the read side of that
//! contract: the UI asks for one when it opens a project or after an edit,
//! and renders the timeline from it.
//!
//! The snapshot is deliberately a **read-only view**. It is JSON because the
//! Kotlin side can parse it without a native dependency, and because it is
//! easy to inspect while debugging.
//!
//! Snapshots are taken on demand (when the project changes), never per frame,
//! so the cost does not matter.

use serde_json::{Value, json};

use crate::model::{AssetKind, BlendMode, TransitionKind};
use crate::project::Project;

/// Build a JSON snapshot of the project.
#[must_use]
pub fn snapshot(project: &Project) -> Value {
    let meta = &project.meta;
    json!({
        "revision": project.revision,
        "meta": {
            "name": meta.name,
            "width": meta.width,
            "height": meta.height,
            "fpsNum": meta.timebase.fps().num(),
            "fpsDen": meta.timebase.fps().den(),
            "dropFrame": meta.timebase.is_drop_frame(),
            "durationNs": meta.duration.as_nanos(),
            "durationMs": meta.duration.as_millis(),
            "background": meta.background.to_u32(),
        },
        "assets": project.assets.iter().filter(|a| a.alive).map(|a| json!({
            "id": index_of_asset(project, a),
            "sourceId": a.source_id,
            "name": a.name,
            "kind": asset_kind_name(a.kind),
            "durationNs": a.duration.as_nanos(),
            "width": a.width,
            "height": a.height,
            "hasAudio": a.has_audio,
            "rotation": a.rotation,
            "variableFrameRate": a.variable_frame_rate,
            "hdr": a.hdr,
        })).collect::<Vec<_>>(),
        "tracks": project.tracks.iter().enumerate().filter(|(_, t)| t.alive).map(|(i, t)| json!({
            "id": i,
            "name": t.name,
            "kind": if t.kind == crate::model::TrackKind::Video { "video" } else { "audio" },
            "enabled": t.enabled,
            "locked": t.locked,
            "muted": t.muted,
            "solo": t.solo,
            "volume": t.volume,
            "effects": t.effects.iter().filter_map(|e| effect_json(project, *e)).collect::<Vec<_>>(),
            "clips": t.clips.iter().filter_map(|c| {
                let clip = project.clips.get(*c as usize)?;
                if !clip.alive { return None; }
                Some(clip_json(project, *c, clip))
            }).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "transitions": project.transitions.iter().enumerate().filter(|(_, t)| t.alive).map(|(i, t)| json!({
            "id": i,
            "kind": transition_kind_name(t.kind),
            "track": t.track,
            "fromClip": t.from_clip,
            "toClip": t.to_clip,
            "durationNs": t.duration.as_nanos(),
        })).collect::<Vec<_>>(),
    })
}

/// The index of an asset inside the project's asset vector.
fn index_of_asset(project: &Project, asset: &crate::model::Asset) -> u32 {
    project
        .assets
        .iter()
        .position(|a| std::ptr::eq(a, asset))
        .unwrap_or(0) as u32
}

/// A JSON view of one clip.
fn clip_json(project: &Project, id: u32, clip: &crate::model::Clip) -> Value {
    let tb = project.meta.timebase;
    json!({
        "id": id,
        "asset": clip.asset,
        "startNs": clip.timeline.start.as_nanos(),
        "endNs": clip.timeline.end.as_nanos(),
        "startFrame": clip.timeline.start.as_nanos() * tb.fps().num() / (tb.fps().den() * 1_000_000_000),
        "durationNs": clip.duration().as_nanos(),
        "sourceInNs": clip.source_in.as_nanos(),
        "speed": clip.speed,
        "forward": clip.forward,
        "opacity": clip.opacity,
        "blend": blend_name(clip.blend),
        "enabled": clip.enabled,
        "muted": clip.muted,
        "volume": clip.volume,
        "transform": {
            "positionX": clip.transform.position.x,
            "positionY": clip.transform.position.y,
            "scaleX": clip.transform.scale.x,
            "scaleY": clip.transform.scale.y,
            "rotation": clip.transform.rotation,
            "anchorX": clip.transform.anchor.x,
            "anchorY": clip.transform.anchor.y,
        },
        "effects": clip.effects.iter().filter_map(|e| effect_json(project, *e)).collect::<Vec<_>>(),
    })
}

/// A JSON view of one effect.
fn effect_json(project: &Project, id: u32) -> Option<Value> {
    let effect = project.effects.get(id as usize)?;
    if !effect.alive {
        return None;
    }
    Some(json!({
        "id": id,
        "type": effect.effect_type.index(),
        "enabled": effect.enabled,
        "params": effect.params.iter_overridden().map(|(index, value)| json!({
            "index": index,
            "value": param_value_json(value),
        })).collect::<Vec<_>>(),
        "keyframes": effect.animation.iter().map(|track| json!({
            "param": track.param,
            "keys": track.keys.iter().map(|k| json!({
                "timeNs": k.time.as_nanos(),
                "value": param_value_json(k.value),
                "interpolation": interpolation_name(k.interpolation),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    }))
}

/// A JSON view of a parameter value.
fn param_value_json(value: vimo_manifest::ParamValue) -> Value {
    use vimo_manifest::ParamValue;
    match value {
        ParamValue::Float(v) => json!({ "type": "float", "value": v }),
        ParamValue::Int(v) => json!({ "type": "int", "value": v }),
        ParamValue::Bool(v) => json!({ "type": "bool", "value": v }),
        ParamValue::Color(c) => json!({ "type": "color", "value": c.to_u32() }),
        ParamValue::Vec2(v) => json!({ "type": "vec2", "value": [v.x, v.y] }),
        ParamValue::Vec3(v) => json!({ "type": "vec3", "value": [v.x, v.y, v.z] }),
        ParamValue::Enum(v) => json!({ "type": "enum", "value": v }),
    }
}

/// The name of a blend mode.
#[must_use]
pub fn blend_name(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "normal",
        BlendMode::Add => "add",
        BlendMode::Screen => "screen",
        BlendMode::Multiply => "multiply",
        BlendMode::Overlay => "overlay",
        BlendMode::HardLight => "hardLight",
        BlendMode::SoftLight => "softLight",
        BlendMode::Lighten => "lighten",
        BlendMode::Darken => "darken",
        BlendMode::ColorDodge => "colorDodge",
        BlendMode::ColorBurn => "colorBurn",
        BlendMode::LinearDodge => "linearDodge",
        BlendMode::LinearBurn => "linearBurn",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
    }
}

/// The name of an asset kind.
#[must_use]
pub fn asset_kind_name(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Video => "video",
        AssetKind::Audio => "audio",
        AssetKind::Image => "image",
        AssetKind::Generated => "generated",
    }
}

/// The name of a transition kind.
#[must_use]
pub fn transition_kind_name(kind: TransitionKind) -> &'static str {
    match kind {
        TransitionKind::CrossDissolve => "crossDissolve",
        TransitionKind::DipToBlack => "dipToBlack",
        TransitionKind::Wipe => "wipe",
        TransitionKind::Slide => "slide",
        TransitionKind::Push => "push",
        TransitionKind::LumaDissolve => "lumaDissolve",
        TransitionKind::Zoom => "zoom",
    }
}

/// The name of an interpolation.
#[must_use]
pub fn interpolation_name(interpolation: crate::anim::Interpolation) -> &'static str {
    use crate::anim::Interpolation;
    match interpolation {
        Interpolation::Linear => "linear",
        Interpolation::Hold => "hold",
        Interpolation::Bezier { .. } => "bezier",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Asset, Clip, Track, TrackKind};
    use vimo_foundation::time::{Time, TimeRange};

    fn project() -> Project {
        let mut p = Project::new();
        p.assets.push(Asset::video("src-0", Time::from_secs(30)));
        let mut track = Track::new("V1", TrackKind::Video);
        track.clips.push(0);
        p.tracks.push(track);
        p.clips.push(Clip::new(
            0,
            TimeRange::new(Time::from_secs(1), Time::from_secs(3)),
            Time::ZERO,
        ));
        p
    }

    #[test]
    fn the_snapshot_describes_the_timeline() {
        let value = snapshot(&project());
        assert_eq!(value["tracks"].as_array().unwrap().len(), 1);
        let clip = &value["tracks"][0]["clips"][0];
        assert_eq!(clip["startNs"], 1_000_000_000i64);
        assert_eq!(clip["endNs"], 3_000_000_000i64);
        assert_eq!(clip["durationNs"], 2_000_000_000i64);
        assert_eq!(clip["asset"], 0);
    }

    #[test]
    fn the_snapshot_is_valid_json() {
        let text = serde_json::to_string(&snapshot(&project())).unwrap();
        assert!(text.starts_with('{'));
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["meta"]["width"], 1920);
    }

    #[test]
    fn dead_entities_are_not_in_the_snapshot() {
        let mut p = project();
        p.clips[0].alive = false;
        let value = snapshot(&p);
        assert_eq!(value["tracks"][0]["clips"].as_array().unwrap().len(), 0);
    }
}
