//! The Vimo project data model.
//!
//! Everything is stored in **flat vectors** indexed by `u32`. The project graph
//! is a DAG of indices rather than a tree of pointers, which means:
//!
//! * iteration is cache friendly and branch predictable,
//! * cloning a project for undo is a handful of `memcpy`s,
//! * the render graph can translate entity ids into dense array indices with a
//!   single lookup table.
//!
//! Ids are *indices*, so they must be re-mapped whenever the backing vector
//! changes shape. To keep that safe, mutations that change the shape of a
//! vector go through [`crate::command::Command`], which knows how to keep every
//! cross-reference consistent.

use vimo_foundation::color::Rgba8;
use vimo_foundation::math::{Vec2, Vec3};
use vimo_foundation::time::{Time, TimeRange, Timebase};

/// Index of an asset inside [`crate::project::Project::assets`].
pub type AssetId = u32;
/// Index of a track inside [`crate::project::Project::tracks`].
pub type TrackId = u32;
/// Index of a clip inside [`crate::project::Project::clips`].
pub type ClipId = u32;
/// Index of an effect node inside [`crate::project::Project::effects`].
pub type EffectId = u32;
/// Index of a transition inside [`crate::project::Project::transitions`].
pub type TransitionId = u32;

/// The kind of media an asset holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetKind {
    /// Video (with optional audio).
    Video,
    /// Audio only.
    Audio,
    /// Still image.
    Image,
    /// Generated / synthetic source.
    Generated,
}

/// A media source referenced by clips.
#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    /// Opaque source id used by the platform layer to open the media.
    pub source_id: String,
    /// Human readable name.
    pub name: String,
    /// What kind of media this is.
    pub kind: AssetKind,
    /// Duration of the source media.
    pub duration: Time,
    /// Frame rate of the source.
    pub timebase: Timebase,
    /// Pixel width (0 for audio).
    pub width: u32,
    /// Pixel height (0 for audio).
    pub height: u32,
    /// Whether the source carries an audio track.
    pub has_audio: bool,
    /// Display rotation in degrees (0/90/180/270).
    pub rotation: u32,
    /// Label colour in the UI.
    pub label_color: Rgba8,
    /// Whether the source is variable frame rate.
    pub variable_frame_rate: bool,
    /// Whether the source uses a colour space other than sRGB (e.g. HDR).
    pub hdr: bool,
    /// Whether the asset is still alive.
    ///
    /// Entities are never removed from the backing vectors; deleting one flips
    /// this flag. Ids therefore stay valid forever, which keeps every
    /// cross-reference (track → clip → effect) sound without index fix-ups.
    pub alive: bool,
}

impl Asset {
    /// A minimal video asset.
    #[must_use]
    pub fn video(source_id: impl Into<String>, duration: Time) -> Self {
        Asset {
            source_id: source_id.into(),
            name: String::new(),
            kind: AssetKind::Video,
            duration,
            timebase: Timebase::from_fps(30, 1),
            width: 1920,
            height: 1080,
            has_audio: true,
            rotation: 0,
            label_color: Rgba8::rgb(70, 120, 220),
            variable_frame_rate: false,
            hdr: false,
            alive: true,
        }
    }
}

/// Whether a track carries pictures or sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackKind {
    /// Video track.
    Video,
    /// Audio track.
    Audio,
}

/// A blend mode used when a clip or layer is composited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum BlendMode {
    /// Source over destination.
    #[default]
    Normal,
    /// `src + dst`.
    Add,
    /// `1 - (1-src)(1-dst)`.
    Screen,
    /// `src * dst`.
    Multiply,
    /// Hard light.
    Overlay,
    /// `2*src*dst` for `dst <= 0.5`.
    HardLight,
    /// Soft light.
    SoftLight,
    /// `max(src, dst)`.
    Lighten,
    /// `min(src, dst)`.
    Darken,
    /// Colour dodge.
    ColorDodge,
    /// Colour burn.
    ColorBurn,
    /// `src + dst` with saturation clamp.
    LinearDodge,
    /// `src + dst - 1`.
    LinearBurn,
    /// Absolute difference.
    Difference,
    /// Exclusion.
    Exclusion,
    /// Hue (non-separable).
    Hue,
    /// Saturation (non-separable).
    Saturation,
    /// Colour (non-separable).
    Color,
    /// Luminosity (non-separable).
    Luminosity,
}


impl BlendMode {
    /// All blend modes, in a stable order.
    pub const ALL: &'static [BlendMode] = &[
        BlendMode::Normal,
        BlendMode::Add,
        BlendMode::Screen,
        BlendMode::Multiply,
        BlendMode::Overlay,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Lighten,
        BlendMode::Darken,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::LinearDodge,
        BlendMode::LinearBurn,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    /// Whether the mode can be expressed with plain GL blend factors.
    ///
    /// The four non-separable modes need a shader pass.
    #[must_use]
    pub const fn is_separable(&self) -> bool {
        !matches!(
            self,
            BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity
        )
    }

    /// The GL blend factors `(src_rgb, dst_rgb, src_alpha, dst_alpha)`.
    ///
    /// `None` for non-separable modes, which require a shader.
    #[must_use]
    pub const fn gl_factors(&self) -> Option<(BlendFactor, BlendFactor, BlendFactor, BlendFactor)> {
        use BlendFactor::*;
        Some(match self {
            BlendMode::Normal => (One, OneMinusSrcAlpha, One, OneMinusSrcAlpha),
            BlendMode::Add => (One, One, One, One),
            BlendMode::Screen => (One, OneMinusSrcColor, One, OneMinusSrcAlpha),
            BlendMode::Multiply => (DstColor, Zero, Zero, One),
            BlendMode::Overlay => (Two, OneMinusTwoSrcAlpha, One, OneMinusSrcAlpha),
            BlendMode::HardLight => (Two, OneMinusTwoSrcAlpha, One, OneMinusSrcAlpha),
            BlendMode::SoftLight => (Two, OneMinusTwoSrcAlpha, One, OneMinusSrcAlpha),
            BlendMode::Lighten => (Max, Max, One, OneMinusSrcAlpha),
            BlendMode::Darken => (Min, Min, One, OneMinusSrcAlpha),
            BlendMode::ColorDodge => (One, OneMinusSrcColor, One, OneMinusSrcAlpha),
            BlendMode::ColorBurn => (One, OneMinusSrcColor, One, OneMinusSrcAlpha),
            BlendMode::LinearDodge => (One, One, One, One),
            BlendMode::LinearBurn => (One, One, One, One),
            BlendMode::Difference => (One, OneMinusSrcColor, One, OneMinusSrcAlpha),
            BlendMode::Exclusion => (One, OneMinusSrcColor, One, OneMinusSrcAlpha),
            BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => {
                return None
            }
        })
    }

    /// Decode from the on-disk representation.
    #[must_use]
    pub const fn from_u8(v: u8) -> BlendMode {
        match v {
            1 => BlendMode::Add,
            2 => BlendMode::Screen,
            3 => BlendMode::Multiply,
            4 => BlendMode::Overlay,
            5 => BlendMode::HardLight,
            6 => BlendMode::SoftLight,
            7 => BlendMode::Lighten,
            8 => BlendMode::Darken,
            9 => BlendMode::ColorDodge,
            10 => BlendMode::ColorBurn,
            11 => BlendMode::LinearDodge,
            12 => BlendMode::LinearBurn,
            13 => BlendMode::Difference,
            14 => BlendMode::Exclusion,
            15 => BlendMode::Hue,
            16 => BlendMode::Saturation,
            17 => BlendMode::Color,
            18 => BlendMode::Luminosity,
            _ => BlendMode::Normal,
        }
    }

    /// The on-disk representation.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            BlendMode::Normal => 0,
            BlendMode::Add => 1,
            BlendMode::Screen => 2,
            BlendMode::Multiply => 3,
            BlendMode::Overlay => 4,
            BlendMode::HardLight => 5,
            BlendMode::SoftLight => 6,
            BlendMode::Lighten => 7,
            BlendMode::Darken => 8,
            BlendMode::ColorDodge => 9,
            BlendMode::ColorBurn => 10,
            BlendMode::LinearDodge => 11,
            BlendMode::LinearBurn => 12,
            BlendMode::Difference => 13,
            BlendMode::Exclusion => 14,
            BlendMode::Hue => 15,
            BlendMode::Saturation => 16,
            BlendMode::Color => 17,
            BlendMode::Luminosity => 18,
        }
    }
}

/// A GL blend factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendFactor {
    /// `GL_ZERO`.
    Zero,
    /// `GL_ONE`.
    One,
    /// `GL_SRC_COLOR`.
    SrcColor,
    /// `GL_ONE_MINUS_SRC_COLOR`.
    OneMinusSrcColor,
    /// `GL_DST_COLOR`.
    DstColor,
    /// `GL_ONE_MINUS_DST_COLOR`.
    OneMinusDstColor,
    /// `GL_SRC_ALPHA`.
    SrcAlpha,
    /// `GL_ONE_MINUS_SRC_ALPHA`.
    OneMinusSrcAlpha,
    /// `GL_DST_ALPHA`.
    DstAlpha,
    /// `GL_ONE_MINUS_DST_ALPHA`.
    OneMinusDstAlpha,
    /// `GL_TWO`.
    Two,
    /// `GL_ONE_MINUS_TWO_SRC_ALPHA`.
    OneMinusTwoSrcAlpha,
    /// `GL_MAX`.
    Max,
    /// `GL_MIN`.
    Min,
}

/// A 2D affine transform applied to a clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Normalised translation (0..1 of the frame).
    pub position: Vec2,
    /// Scale factor.
    pub scale: Vec2,
    /// Rotation in radians.
    pub rotation: f32,
    /// Normalised anchor point.
    pub anchor: Vec2,
    /// Crop rectangle in normalised source space.
    pub crop: vimo_foundation::math::Rect,
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            position: Vec2::ZERO,
            scale: Vec2::ONE,
            rotation: 0.0,
            anchor: Vec2::new(0.5, 0.5),
            crop: vimo_foundation::math::Rect::FULL,
        }
    }
}

impl Transform {
    /// Build the 3x3 matrix for this transform, in normalised frame space.
    ///
    /// The anchor is the pivot: scaling and rotation happen around it, and the
    /// anchor itself is kept at the centre of the frame.
    #[must_use]
    pub fn to_matrix(&self) -> vimo_foundation::math::Mat3 {
        let (sx, sy) = (self.scale.x, self.scale.y);
        let (cx, cy) = (self.anchor.x, self.anchor.y);
        let (px, py) = (self.position.x, self.position.y);

        // T(-anchor) * R * S * T(anchor) * T(position)
        let rot = vimo_foundation::math::Mat3::affine_2d(
            Vec2::new(sx, sy),
            self.rotation,
            Vec2::ZERO,
        );
        // Pivot compensation: after scaling around the anchor the anchor drifts.
        let pivot = vimo_foundation::math::Mat3::affine_2d(
            Vec2::ONE,
            0.0,
            Vec2::new(cx - cx * sx, cy - cy * sy),
        );
        let offset = vimo_foundation::math::Mat3::affine_2d(
            Vec2::ONE,
            0.0,
            Vec2::new(px, py),
        );
        offset.mul(&pivot).mul(&rot)
    }
}

/// A clip placed on the timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// The asset this clip plays.
    pub asset: AssetId,
    /// Where the clip sits on the timeline.
    pub timeline: TimeRange,
    /// In point within the source media.
    pub source_in: Time,
    /// Playback rate (1.0 = real time).
    pub speed: f32,
    /// Whether the clip plays forwards.
    pub forward: bool,
    /// Opacity in `0..=1`.
    pub opacity: f32,
    /// How the clip composites with the layers below.
    pub blend: BlendMode,
    /// 2D transform.
    pub transform: Transform,
    /// Whether the clip is rendered.
    pub enabled: bool,
    /// Effect nodes applied to the clip.
    pub effects: Vec<EffectId>,
    /// Label colour in the UI.
    pub label_color: Rgba8,
    /// Gain applied to the clip's audio (`1.0` = unity).
    pub volume: f32,
    /// Whether the clip is muted.
    pub muted: bool,
    /// Monotonic revision counter, bumped whenever this clip changes.
    ///
    /// The render graph compares the revision it last drew against the current
    /// one to decide whether the layer must be re-evaluated. Because the
    /// revision is part of the data a command captures, undo restores the
    /// previous revision exactly, which keeps dirty tracking sound.
    pub revision: u64,
    /// Whether the clip is still alive.
    pub alive: bool,
}

impl Clip {
    /// A clip covering `range` of the timeline, starting at `source_in`.
    #[must_use]
    pub fn new(asset: AssetId, range: TimeRange, source_in: Time) -> Self {
        Clip {
            asset,
            timeline: range,
            source_in,
            speed: 1.0,
            forward: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            transform: Transform::default(),
            enabled: true,
            effects: Vec::new(),
            label_color: Rgba8::rgb(70, 120, 220),
            volume: 1.0,
            muted: false,
            revision: 0,
            alive: true,
        }
    }

    /// Duration of the clip on the timeline.
    #[must_use]
    pub fn duration(&self) -> Time {
        self.timeline.duration()
    }

    /// The source-media range this clip plays.
    #[must_use]
    pub fn source_range(&self) -> TimeRange {
        let dur = self.duration();
        let speed = if self.speed.abs() < f32::EPSILON { 1.0 } else { self.speed };
        TimeRange::new(self.source_in, self.source_in + Time::from_secs_f64(dur.as_secs_f64() * speed as f64))
    }

    /// Trim the clip to `range`, keeping the picture in sync.
    ///
    /// Trimming the **head** of a clip must advance the source in-point by the
    /// same amount, otherwise the picture jumps to a different part of the
    /// source. Trimming the tail simply shortens the clip. The result is
    /// clamped to `source_duration` so that a clip can never run past the end
    /// of its media.
    #[must_use]
    pub fn trimmed(&self, range: TimeRange, source_duration: Time) -> Clip {
        let mut clip = self.clone();
        let old_start = clip.timeline.start;
        let new_start = range.start.max(old_start);
        let new_end = range.end.max(new_start);

        // How much of the head was removed, in source time.
        let head = new_start - old_start;
        if head > Time::ZERO {
            let speed = if clip.speed.abs() < f32::EPSILON { 1.0 } else { clip.speed };
            let advance = Time::from_secs_f64(head.as_secs_f64() * speed as f64);
            clip.source_in += advance;
        }

        // The clip may not run past the end of the source.
        let speed = if clip.speed.abs() < f32::EPSILON { 1.0 } else { clip.speed };
        let max_duration = Time::from_secs_f64(source_duration.as_secs_f64() / speed as f64);
        let duration = (new_end - new_start).min(max_duration);
        clip.timeline = TimeRange::new(new_start, new_start + duration);
        clip
    }

    /// Split the clip at `at`, returning the second half.
    ///
    /// The second half continues where the first one stopped, so the picture
    /// does not jump across the cut.
    #[must_use]
    pub fn split_at(&self, at: Time) -> Option<Clip> {
        if at <= self.timeline.start || at >= self.timeline.end {
            return None;
        }
        let speed = if self.speed.abs() < f32::EPSILON { 1.0 } else { self.speed };
        let mut second = self.clone();
        second.timeline = TimeRange::new(at, self.timeline.end);
        second.source_in = self.source_in + Time::from_secs_f64((at - self.timeline.start).as_secs_f64() * speed as f64);
        Some(second)
    }
}

/// A timeline track.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// Display name.
    pub name: String,
    /// Video or audio.
    pub kind: TrackKind,
    /// Whether the track contributes to the render.
    pub enabled: bool,
    /// Whether the track rejects edits.
    pub locked: bool,
    /// Whether audio is muted.
    pub muted: bool,
    /// Whether this track plays alone.
    pub solo: bool,
    /// Track gain.
    pub volume: f32,
    /// Clips in timeline order.
    pub clips: Vec<ClipId>,
    /// Track level effect nodes.
    pub effects: Vec<EffectId>,
    /// UI height hint.
    pub height: f32,
    /// Monotonic revision counter, bumped whenever this track changes.
    pub revision: u64,
    /// Whether the track is still alive.
    pub alive: bool,
}

impl Track {
    /// A new track.
    #[must_use]
    pub fn new(name: impl Into<String>, kind: TrackKind) -> Self {
        Track {
            name: name.into(),
            kind,
            enabled: true,
            locked: false,
            muted: false,
            solo: false,
            volume: 1.0,
            clips: Vec::new(),
            effects: Vec::new(),
            height: 60.0,
            revision: 0,
            alive: true,
        }
    }
}

/// The kind of a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransitionKind {
    /// Cross dissolve.
    CrossDissolve,
    /// Dip to black.
    DipToBlack,
    /// Wipe with a direction.
    Wipe,
    /// Slide.
    Slide,
    /// Push.
    Push,
    /// Luma dissolve.
    LumaDissolve,
    /// Zoom.
    Zoom,
}

impl TransitionKind {
    /// Decode from the on-disk representation.
    #[must_use]
    pub const fn from_u8(v: u8) -> TransitionKind {
        match v {
            1 => TransitionKind::DipToBlack,
            2 => TransitionKind::Wipe,
            3 => TransitionKind::Slide,
            4 => TransitionKind::Push,
            5 => TransitionKind::LumaDissolve,
            6 => TransitionKind::Zoom,
            _ => TransitionKind::CrossDissolve,
        }
    }

    /// The on-disk representation.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            TransitionKind::CrossDissolve => 0,
            TransitionKind::DipToBlack => 1,
            TransitionKind::Wipe => 2,
            TransitionKind::Slide => 3,
            TransitionKind::Push => 4,
            TransitionKind::LumaDissolve => 5,
            TransitionKind::Zoom => 6,
        }
    }
}

/// A transition between two adjacent clips on a track.
#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    /// Transition kind.
    pub kind: TransitionKind,
    /// The track the transition lives on.
    pub track: TrackId,
    /// The clip before the cut.
    pub from_clip: ClipId,
    /// The clip after the cut.
    pub to_clip: ClipId,
    /// Total duration of the transition.
    pub duration: Time,
    /// Transition parameters (e.g. wipe direction).
    pub params: vimo_manifest::ParamStore,
    /// Whether the transition is still alive.
    pub alive: bool,
}

impl Transition {
    /// A cross dissolve between two clips.
    #[must_use]
    pub fn cross_dissolve(track: TrackId, from_clip: ClipId, to_clip: ClipId, duration: Time) -> Self {
        Transition {
            kind: TransitionKind::CrossDissolve,
            track,
            from_clip,
            to_clip,
            duration,
            params: vimo_manifest::ParamStore::from_values(Vec::new()),
            alive: true,
        }
    }
}

/// Which entity an effect node is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectOwner {
    /// Attached to a clip.
    Clip(ClipId),
    /// Attached to a track.
    Track(TrackId),
    /// Attached to a transition.
    Transition(TransitionId),
}

/// An instantiated effect.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectNode {
    /// The effect type from the manifest registry.
    pub effect_type: vimo_manifest::EffectTypeId,
    /// Parameter values.
    pub params: vimo_manifest::ParamStore,
    /// Whether the effect is applied.
    pub enabled: bool,
    /// Animation tracks, one per animated parameter.
    pub animation: Vec<crate::anim::AnimTrack>,
    /// Monotonic revision counter, bumped whenever this effect changes.
    pub revision: u64,
    /// Whether the effect instance is still alive.
    pub alive: bool,
}

impl EffectNode {
    /// Create an effect node with default parameters.
    #[must_use]
    pub fn new(effect_type: vimo_manifest::EffectTypeId, desc: &vimo_manifest::EffectDesc) -> Self {
        EffectNode {
            effect_type,
            params: desc.new_param_store(),
            enabled: true,
            animation: Vec::new(),
            revision: 0,
            alive: true,
        }
    }
}

/// Project-level metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMeta {
    /// Project name.
    pub name: String,
    /// Timeline frame rate.
    pub timebase: Timebase,
    /// Canvas width in pixels.
    pub width: u32,
    /// Canvas height in pixels.
    pub height: u32,
    /// Background colour.
    pub background: Rgba8,
    /// Total timeline duration.
    pub duration: Time,
}

impl Default for ProjectMeta {
    fn default() -> Self {
        ProjectMeta {
            name: "Untitled".to_string(),
            timebase: Timebase::from_fps(30, 1),
            width: 1920,
            height: 1080,
            background: Rgba8::BLACK,
            duration: Time::ZERO,
        }
    }
}

/// A 3D vector used by effects (light direction, etc.).
pub type ColorVec3 = Vec3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_modes_roundtrip_through_u8() {
        for m in BlendMode::ALL {
            assert_eq!(BlendMode::from_u8(m.as_u8()), *m);
        }
    }

    #[test]
    fn non_separable_modes_need_a_shader() {
        assert!(!BlendMode::Hue.is_separable());
        assert!(BlendMode::Normal.is_separable());
        assert!(BlendMode::Normal.gl_factors().is_some());
        assert!(BlendMode::Luminosity.gl_factors().is_none());
    }

    #[test]
    fn trimming_the_head_advances_the_source() {
        let clip = Clip::new(
            0,
            vimo_foundation::time::TimeRange::new(
                vimo_foundation::time::Time::from_secs(5),
                vimo_foundation::time::Time::from_secs(10),
            ),
            vimo_foundation::time::Time::from_secs(2),
        );
        // Trim two seconds off the head: the source must advance by two
        // seconds, otherwise the picture jumps.
        let trimmed = clip.trimmed(
            vimo_foundation::time::TimeRange::new(
                vimo_foundation::time::Time::from_secs(7),
                vimo_foundation::time::Time::from_secs(10),
            ),
            vimo_foundation::time::Time::from_secs(30),
        );
        assert_eq!(trimmed.source_in, vimo_foundation::time::Time::from_secs(4));
        assert_eq!(trimmed.timeline.start, vimo_foundation::time::Time::from_secs(7));
        assert_eq!(trimmed.duration(), vimo_foundation::time::Time::from_secs(3));
    }

    #[test]
    fn trimming_the_tail_leaves_the_source_alone() {
        let clip = Clip::new(
            0,
            vimo_foundation::time::TimeRange::new(
                vimo_foundation::time::Time::from_secs(5),
                vimo_foundation::time::Time::from_secs(10),
            ),
            vimo_foundation::time::Time::from_secs(2),
        );
        let trimmed = clip.trimmed(
            vimo_foundation::time::TimeRange::new(
                vimo_foundation::time::Time::from_secs(5),
                vimo_foundation::time::Time::from_secs(8),
            ),
            vimo_foundation::time::Time::from_secs(30),
        );
        assert_eq!(trimmed.source_in, vimo_foundation::time::Time::from_secs(2));
        assert_eq!(trimmed.duration(), vimo_foundation::time::Time::from_secs(3));
    }

    #[test]
    fn trimming_cannot_run_past_the_source() {
        let clip = Clip::new(
            0,
            vimo_foundation::time::TimeRange::new(
                vimo_foundation::time::Time::from_secs(0),
                vimo_foundation::time::Time::from_secs(5),
            ),
            vimo_foundation::time::Time::from_secs(0),
        );
        let trimmed = clip.trimmed(
            vimo_foundation::time::TimeRange::new(
                vimo_foundation::time::Time::from_secs(0),
                vimo_foundation::time::Time::from_secs(60),
            ),
            vimo_foundation::time::Time::from_secs(10),
        );
        assert_eq!(trimmed.duration(), vimo_foundation::time::Time::from_secs(10));
    }

    #[test]
    fn splitting_continues_the_source() {
        let clip = Clip::new(
            0,
            vimo_foundation::time::TimeRange::new(
                vimo_foundation::time::Time::from_secs(5),
                vimo_foundation::time::Time::from_secs(10),
            ),
            vimo_foundation::time::Time::from_secs(2),
        );
        let second = clip
            .split_at(vimo_foundation::time::Time::from_secs(7))
            .expect("a split inside the clip");
        assert_eq!(second.source_in, vimo_foundation::time::Time::from_secs(4));
        assert_eq!(second.timeline.start, vimo_foundation::time::Time::from_secs(7));
        assert_eq!(second.duration(), vimo_foundation::time::Time::from_secs(3));
        assert!(clip.split_at(vimo_foundation::time::Time::from_secs(5)).is_none());
    }

    #[test]
    fn transform_pivots_around_the_anchor() {
        let t = Transform {
            position: Vec2::ZERO,
            scale: Vec2::new(2.0, 2.0),
            rotation: 0.0,
            anchor: Vec2::new(0.5, 0.5),
            crop: Default::default(),
        };
        let m = t.to_matrix();
        // The anchor must stay put when scaling around it.
        let p = m.transform_vec3(vimo_foundation::math::Vec3::new(0.5, 0.5, 1.0));
        assert!((p.x - 0.5).abs() < 1e-5, "x={}", p.x);
        assert!((p.y - 0.5).abs() < 1e-5, "y={}", p.y);
    }
}
