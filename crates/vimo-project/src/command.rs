//! The command system.
//!
//! Every mutation of a [`Project`] is expressed as a [`Command`]. Applying a
//! command returns its **inverse**, which is what the undo stack stores. This
//! "apply returns the inverse" shape means a command only has to describe the
//! forward transition; the reverse transition is derived automatically and can
//! never drift out of sync with it.
//!
//! ```
//! use vimo_project::{Command, Project};
//! use vimo_project::model::{Track, TrackKind};
//! use vimo_foundation::time::{Time, TimeRange};
//!
//! let mut project = Project::new();
//! project.tracks.push(Track::new("V1", TrackKind::Video));
//! let clip = vimo_project::model::Clip::new(
//!     0,
//!     TimeRange::new(Time::ZERO, Time::from_secs(2)),
//!     Time::ZERO,
//! );
//! let cmd = Command::add_clip(0, 0, clip);
//! let inverse = cmd.apply(&mut project).unwrap();
//! assert_eq!(project.clips.len(), 1);
//! inverse.apply(&mut project).unwrap();
//! assert!(!project.clips[0].alive);
//! ```

use vimo_foundation::error::{invalid_argument, invalid_state, VimoResult};
use vimo_foundation::time::Time;
use vimo_manifest::ParamValue;

use crate::anim::{AnimTrack, Interpolation, Keyframe};
use crate::model::{
    Asset, AssetId, Clip, ClipId, EffectId, EffectNode, EffectOwner, Track, TrackId, Transition,
    TransitionId,
};
use crate::project::Project;

/// A reversible mutation of a project.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Append a new asset.
    AddAsset {
        /// The asset to add.
        asset: Asset,
    },
    /// Mark an asset dead (it must not be referenced by a live clip).
    RemoveAsset {
        /// The asset index.
        index: AssetId,
    },
    /// Bring a removed asset back.
    RestoreAsset {
        /// The asset index.
        index: AssetId,
    },
    /// Append a new track.
    AddTrack {
        /// The track to add.
        track: Track,
    },
    /// Mark a track dead.
    RemoveTrack {
        /// The track index.
        index: TrackId,
    },
    /// Bring a removed track back.
    RestoreTrack {
        /// The track index.
        index: TrackId,
    },
    /// Append a clip and place it on a track.
    AddClip {
        /// The track the clip belongs to.
        track: TrackId,
        /// Where in the track's clip list the clip goes.
        list_index: usize,
        /// The clip data.
        clip: Clip,
    },
    /// Mark a clip dead and detach it from its track.
    RemoveClip {
        /// The track the clip belonged to.
        track: TrackId,
        /// Where the clip was in the track's clip list.
        list_index: usize,
        /// The clip index.
        clip: ClipId,
    },
    /// Bring a removed clip back.
    RestoreClip {
        /// The track the clip belongs to.
        track: TrackId,
        /// Where in the track's clip list the clip goes.
        list_index: usize,
        /// The clip index.
        clip: ClipId,
    },
    /// Replace a clip's contents.
    UpdateClip {
        /// The clip index.
        clip: ClipId,
        /// The state before the change.
        before: Clip,
        /// The state after the change.
        after: Clip,
    },
    /// Replace a track's contents.
    UpdateTrack {
        /// The track index.
        track: TrackId,
        /// The state before the change.
        before: Track,
        /// The state after the change.
        after: Track,
    },
    /// Append an effect and attach it to an owner.
    AddEffect {
        /// The clip, track or transition the effect attaches to.
        owner: EffectOwner,
        /// The effect instance.
        effect: EffectNode,
    },
    /// Mark an effect dead and detach it from its owner.
    RemoveEffect {
        /// The owner the effect was attached to.
        owner: EffectOwner,
        /// Where the effect was in the owner's effect list.
        list_index: usize,
        /// The effect index.
        effect: EffectId,
    },
    /// Bring a removed effect back.
    RestoreEffect {
        /// The owner the effect attaches to.
        owner: EffectOwner,
        /// Where in the owner's effect list the effect goes.
        list_index: usize,
        /// The effect index.
        effect: EffectId,
    },
    /// Replace an effect's contents.
    UpdateEffect {
        /// The effect index.
        effect: EffectId,
        /// The state before the change.
        before: EffectNode,
        /// The state after the change.
        after: EffectNode,
    },
    /// Change a single parameter value.
    SetParam {
        /// The effect index.
        effect: EffectId,
        /// The parameter index.
        param: u16,
        /// The value before the change.
        before: ParamValue,
        /// The value after the change.
        after: ParamValue,
    },
    /// Enable or disable an effect.
    SetEffectEnabled {
        /// The effect index.
        effect: EffectId,
        /// Enabled before the change.
        before: bool,
        /// Enabled after the change.
        after: bool,
    },
    /// Insert a keyframe into an animation track.
    AddKeyframe {
        /// The effect index.
        effect: EffectId,
        /// The animation track index.
        track: usize,
        /// The keyframe.
        key: Keyframe,
    },
    /// Remove a keyframe from an animation track.
    RemoveKeyframe {
        /// The effect index.
        effect: EffectId,
        /// The animation track index.
        track: usize,
        /// The index of the keyframe inside the track.
        index: usize,
        /// The keyframe that was removed.
        key: Keyframe,
    },
    /// Move a keyframe to another position inside its track.
    MoveKeyframe {
        /// The effect index.
        effect: EffectId,
        /// The animation track index.
        track: usize,
        /// The old index.
        from: usize,
        /// The new index.
        to: usize,
    },
    /// Change a keyframe's interpolation.
    SetKeyframeInterpolation {
        /// The effect index.
        effect: EffectId,
        /// The animation track index.
        track: usize,
        /// The keyframe index.
        index: usize,
        /// The interpolation before the change.
        before: Interpolation,
        /// The interpolation after the change.
        after: Interpolation,
    },
    /// Append a transition.
    AddTransition {
        /// The transition.
        transition: Transition,
    },
    /// Mark a transition dead.
    RemoveTransition {
        /// The transition index.
        index: TransitionId,
    },
    /// Bring a removed transition back.
    RestoreTransition {
        /// The transition index.
        index: TransitionId,
    },
    /// A sequence of commands applied as one unit.
    Batch(Vec<Command>),
}

impl Command {
    /// Build an `AddClip` command.
    #[must_use]
    pub fn add_clip(track: TrackId, list_index: usize, clip: Clip) -> Command {
        Command::AddClip {
            track,
            list_index,
            clip,
        }
    }

    /// Build an `UpdateClip` command from a mutation of a copy of the clip.
    ///
    /// The revision counter is bumped by [`Command::apply`], not by the
    /// mutation closure.
    #[must_use]
    pub fn edit_clip(clip: ClipId, before: Clip, mutate: impl FnOnce(&mut Clip)) -> Command {
        let mut after = before.clone();
        mutate(&mut after);
        Command::UpdateClip {
            clip,
            before,
            after,
        }
    }

    /// Build an `UpdateTrack` command from a mutation of a copy of the track.
    #[must_use]
    pub fn edit_track(track: TrackId, before: Track, mutate: impl FnOnce(&mut Track)) -> Command {
        let mut after = before.clone();
        mutate(&mut after);
        Command::UpdateTrack {
            track,
            before,
            after,
        }
    }

    /// Build an `UpdateEffect` command from a mutation of a copy of the effect.
    #[must_use]
    pub fn edit_effect(
        effect: EffectId,
        before: EffectNode,
        mutate: impl FnOnce(&mut EffectNode),
    ) -> Command {
        let mut after = before.clone();
        mutate(&mut after);
        Command::UpdateEffect {
            effect,
            before,
            after,
        }
    }

    /// The effect index this command touches, when it touches exactly one.
    #[must_use]
    pub fn effect_id(&self) -> Option<EffectId> {
        match self {
            Command::AddEffect { .. } => None,
            Command::RemoveEffect { effect, .. }
            | Command::RestoreEffect { effect, .. }
            | Command::UpdateEffect { effect, .. }
            | Command::SetParam { effect, .. }
            | Command::SetEffectEnabled { effect, .. }
            | Command::AddKeyframe { effect, .. }
            | Command::RemoveKeyframe { effect, .. }
            | Command::MoveKeyframe { effect, .. }
            | Command::SetKeyframeInterpolation { effect, .. } => Some(*effect),
            _ => None,
        }
    }

    /// A short, stable label for the command, suitable for an undo menu.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Command::AddAsset { .. } => "Add Media",
            Command::RemoveAsset { .. } | Command::RestoreAsset { .. } => "Remove Media",
            Command::AddTrack { .. } => "Add Track",
            Command::RemoveTrack { .. } | Command::RestoreTrack { .. } => "Remove Track",
            Command::AddClip { .. } => "Add Clip",
            Command::RemoveClip { .. } | Command::RestoreClip { .. } => "Delete Clip",
            Command::UpdateClip { .. } => "Edit Clip",
            Command::UpdateTrack { .. } => "Edit Track",
            Command::AddEffect { .. } => "Add Effect",
            Command::RemoveEffect { .. } | Command::RestoreEffect { .. } => "Remove Effect",
            Command::UpdateEffect { .. } => "Edit Effect",
            Command::SetParam { .. } => "Change Parameter",
            Command::SetEffectEnabled { .. } => "Toggle Effect",
            Command::AddKeyframe { .. } => "Add Keyframe",
            Command::RemoveKeyframe { .. } => "Remove Keyframe",
            Command::MoveKeyframe { .. } => "Move Keyframe",
            Command::SetKeyframeInterpolation { .. } => "Change Keyframe Easing",
            Command::AddTransition { .. } => "Add Transition",
            Command::RemoveTransition { .. } | Command::RestoreTransition { .. } => {
                "Remove Transition"
            }
            Command::Batch(_) => "Edit",
        }
    }

    /// Apply the command to `project`, returning the command that undoes it.
    pub fn apply(&self, project: &mut Project) -> VimoResult<Command> {
        let inverse = match self {
            Command::AddAsset { asset } => {
                project.assets.push(asset.clone());
                Command::RemoveAsset {
                    index: (project.assets.len() - 1) as AssetId,
                }
            }
            Command::RemoveAsset { index } => {
                if project.live_clips().any(|(_, c)| c.asset == *index) {
                    return Err(invalid_state("remove asset: still used by a clip"));
                }
                let asset = project
                    .assets
                    .get_mut(*index as usize)
                    .ok_or_else(|| invalid_argument("remove asset: index out of range"))?;
                asset.alive = false;
                Command::RestoreAsset { index: *index }
            }
            Command::RestoreAsset { index } => {
                let asset = project
                    .assets
                    .get_mut(*index as usize)
                    .ok_or_else(|| invalid_argument("restore asset: index out of range"))?;
                asset.alive = true;
                Command::RemoveAsset { index: *index }
            }
            Command::AddTrack { track } => {
                project.tracks.push(track.clone());
                Command::RemoveTrack {
                    index: (project.tracks.len() - 1) as TrackId,
                }
            }
            Command::RemoveTrack { index } => {
                let t = project
                    .tracks
                    .get_mut(*index as usize)
                    .ok_or_else(|| invalid_argument("remove track: index out of range"))?;
                t.alive = false;
                Command::RestoreTrack { index: *index }
            }
            Command::RestoreTrack { index } => {
                let t = project
                    .tracks
                    .get_mut(*index as usize)
                    .ok_or_else(|| invalid_argument("restore track: index out of range"))?;
                t.alive = true;
                Command::RemoveTrack { index: *index }
            }
            Command::AddClip {
                track,
                list_index,
                clip,
            } => {
                let t = project
                    .tracks
                    .get_mut(*track as usize)
                    .filter(|t| t.alive)
                    .ok_or_else(|| invalid_argument("add clip: unknown track"))?;
                let id = project.clips.len() as ClipId;
                let mut clip = clip.clone();
                clip.alive = true;
                project.clips.push(clip);
                let pos = (*list_index).min(t.clips.len());
                t.clips.insert(pos, id);
                t.revision = t.revision.wrapping_add(1);
                Command::RemoveClip {
                    track: *track,
                    list_index: pos,
                    clip: id,
                }
            }
            Command::RemoveClip {
                track,
                list_index,
                clip,
            } => {
                let t = project
                    .tracks
                    .get_mut(*track as usize)
                    .filter(|t| t.alive)
                    .ok_or_else(|| invalid_argument("remove clip: unknown track"))?;
                let pos = t
                    .clips
                    .iter()
                    .position(|c| c == clip)
                    .unwrap_or(*list_index);
                t.clips.remove(pos);
                t.revision = t.revision.wrapping_add(1);
                if let Some(c) = project.clips.get_mut(*clip as usize) {
                    c.alive = false;
                }
                Command::RestoreClip {
                    track: *track,
                    list_index: pos,
                    clip: *clip,
                }
            }
            Command::RestoreClip {
                track,
                list_index,
                clip,
            } => {
                let t = project
                    .tracks
                    .get_mut(*track as usize)
                    .filter(|t| t.alive)
                    .ok_or_else(|| invalid_argument("restore clip: unknown track"))?;
                if let Some(c) = project.clips.get_mut(*clip as usize) {
                    c.alive = true;
                }
                let pos = (*list_index).min(t.clips.len());
                if !t.clips.contains(clip) {
                    t.clips.insert(pos, *clip);
                }
                t.revision = t.revision.wrapping_add(1);
                Command::RemoveClip {
                    track: *track,
                    list_index: pos,
                    clip: *clip,
                }
            }
            Command::UpdateClip {
                clip,
                before,
                after,
            } => {
                let slot = project
                    .clips
                    .get_mut(*clip as usize)
                    .ok_or_else(|| invalid_argument("update clip: unknown clip"))?;
                let mut next = after.clone();
                next.revision = slot.revision.wrapping_add(1);
                *slot = next;
                Command::UpdateClip {
                    clip: *clip,
                    before: after.clone(),
                    after: before.clone(),
                }
            }
            Command::UpdateTrack {
                track,
                before,
                after,
            } => {
                let slot = project
                    .tracks
                    .get_mut(*track as usize)
                    .ok_or_else(|| invalid_argument("update track: unknown track"))?;
                let mut next = after.clone();
                next.revision = slot.revision.wrapping_add(1);
                *slot = next;
                Command::UpdateTrack {
                    track: *track,
                    before: after.clone(),
                    after: before.clone(),
                }
            }
            Command::AddEffect { owner, effect } => {
                let id = project.effects.len() as EffectId;
                let mut node = effect.clone();
                node.alive = true;
                project.effects.push(node);
                let list_index = attach_effect(project, *owner, id)?;
                Command::RemoveEffect {
                    owner: *owner,
                    list_index,
                    effect: id,
                }
            }
            Command::RemoveEffect {
                owner,
                list_index,
                effect,
            } => {
                let list = effect_list_mut(project, *owner)?;
                let pos = list.iter().position(|e| e == effect).unwrap_or(*list_index);
                list.remove(pos);
                if let Some(e) = project.effects.get_mut(*effect as usize) {
                    e.alive = false;
                }
                Command::RestoreEffect {
                    owner: *owner,
                    list_index: pos,
                    effect: *effect,
                }
            }
            Command::RestoreEffect {
                owner,
                list_index,
                effect,
            } => {
                let list = effect_list_mut(project, *owner)?;
                let pos = (*list_index).min(list.len());
                if !list.contains(effect) {
                    list.insert(pos, *effect);
                }
                if let Some(e) = project.effects.get_mut(*effect as usize) {
                    e.alive = true;
                }
                Command::RemoveEffect {
                    owner: *owner,
                    list_index: pos,
                    effect: *effect,
                }
            }
            Command::UpdateEffect {
                effect,
                before,
                after,
            } => {
                let slot = project
                    .effects
                    .get_mut(*effect as usize)
                    .ok_or_else(|| invalid_argument("update effect: unknown effect"))?;
                let mut next = after.clone();
                next.revision = slot.revision.wrapping_add(1);
                *slot = next;
                Command::UpdateEffect {
                    effect: *effect,
                    before: after.clone(),
                    after: before.clone(),
                }
            }
            Command::SetParam {
                effect,
                param,
                before,
                after,
            } => {
                let slot = project
                    .effects
                    .get_mut(*effect as usize)
                    .ok_or_else(|| invalid_argument("set param: unknown effect"))?;
                slot.params.set(*param, *after);
                slot.revision = slot.revision.wrapping_add(1);
                Command::SetParam {
                    effect: *effect,
                    param: *param,
                    before: *after,
                    after: *before,
                }
            }
            Command::SetEffectEnabled {
                effect,
                before,
                after,
            } => {
                let slot = project
                    .effects
                    .get_mut(*effect as usize)
                    .ok_or_else(|| invalid_argument("toggle effect: unknown effect"))?;
                slot.enabled = *after;
                slot.revision = slot.revision.wrapping_add(1);
                Command::SetEffectEnabled {
                    effect: *effect,
                    before: *after,
                    after: *before,
                }
            }
            Command::AddKeyframe { effect, track, key } => {
                let slot = project
                    .effects
                    .get_mut(*effect as usize)
                    .ok_or_else(|| invalid_argument("add keyframe: unknown effect"))?;
                let anim = slot
                    .animation
                    .get_mut(*track)
                    .ok_or_else(|| invalid_argument("add keyframe: unknown animation track"))?;
                let index = anim.insert(*key);
                slot.revision = slot.revision.wrapping_add(1);
                Command::RemoveKeyframe {
                    effect: *effect,
                    track: *track,
                    index,
                    key: *key,
                }
            }
            Command::RemoveKeyframe {
                effect,
                track,
                index,
                key,
            } => {
                let slot = project
                    .effects
                    .get_mut(*effect as usize)
                    .ok_or_else(|| invalid_argument("remove keyframe: unknown effect"))?;
                let anim = slot
                    .animation
                    .get_mut(*track)
                    .ok_or_else(|| invalid_argument("remove keyframe: unknown animation track"))?;
                anim.remove(*index);
                slot.revision = slot.revision.wrapping_add(1);
                Command::AddKeyframe {
                    effect: *effect,
                    track: *track,
                    key: *key,
                }
            }
            Command::MoveKeyframe {
                effect,
                track,
                from,
                to,
            } => {
                let slot = project
                    .effects
                    .get_mut(*effect as usize)
                    .ok_or_else(|| invalid_argument("move keyframe: unknown effect"))?;
                let anim = slot
                    .animation
                    .get_mut(*track)
                    .ok_or_else(|| invalid_argument("move keyframe: unknown animation track"))?;
                if *from < anim.keys.len() && *to < anim.keys.len() {
                    let k = anim.keys.remove(*from);
                    anim.keys.insert(*to, k);
                }
                slot.revision = slot.revision.wrapping_add(1);
                Command::MoveKeyframe {
                    effect: *effect,
                    track: *track,
                    from: *to,
                    to: *from,
                }
            }
            Command::SetKeyframeInterpolation {
                effect,
                track,
                index,
                before,
                after,
            } => {
                let slot = project
                    .effects
                    .get_mut(*effect as usize)
                    .ok_or_else(|| invalid_argument("set easing: unknown effect"))?;
                let anim = slot
                    .animation
                    .get_mut(*track)
                    .ok_or_else(|| invalid_argument("set easing: unknown animation track"))?;
                if let Some(k) = anim.keys.get_mut(*index) {
                    k.interpolation = *after;
                }
                slot.revision = slot.revision.wrapping_add(1);
                Command::SetKeyframeInterpolation {
                    effect: *effect,
                    track: *track,
                    index: *index,
                    before: *after,
                    after: *before,
                }
            }
            Command::AddTransition { transition } => {
                project.transitions.push(transition.clone());
                Command::RemoveTransition {
                    index: (project.transitions.len() - 1) as TransitionId,
                }
            }
            Command::RemoveTransition { index } => {
                let t = project
                    .transitions
                    .get_mut(*index as usize)
                    .ok_or_else(|| invalid_argument("remove transition: index out of range"))?;
                t.alive = false;
                Command::RestoreTransition { index: *index }
            }
            Command::RestoreTransition { index } => {
                let t = project
                    .transitions
                    .get_mut(*index as usize)
                    .ok_or_else(|| invalid_argument("restore transition: index out of range"))?;
                t.alive = true;
                Command::RemoveTransition { index: *index }
            }
            Command::Batch(cmds) => {
                let mut inverses = Vec::with_capacity(cmds.len());
                for cmd in cmds {
                    inverses.push(cmd.apply(project)?);
                }
                inverses.reverse();
                Command::Batch(inverses)
            }
        };
        project.bump();
        Ok(inverse)
    }
}

/// Get the mutable effect list of an owner.
fn effect_list_mut(project: &mut Project, owner: EffectOwner) -> VimoResult<&mut Vec<EffectId>> {
    match owner {
        EffectOwner::Clip(id) => project
            .clips
            .get_mut(id as usize)
            .map(|c| &mut c.effects)
            .ok_or_else(|| invalid_argument("effect owner: unknown clip")),
        EffectOwner::Track(id) => project
            .tracks
            .get_mut(id as usize)
            .map(|t| &mut t.effects)
            .ok_or_else(|| invalid_argument("effect owner: unknown track")),
        EffectOwner::Transition(_id) => Err(invalid_argument(
            "effect owner: transitions carry no effect list",
        )),
    }
}

/// Attach an effect to an owner, returning the index it was inserted at.
fn attach_effect(project: &mut Project, owner: EffectOwner, effect: EffectId) -> VimoResult<usize> {
    let list = effect_list_mut(project, owner)?;
    list.push(effect);
    Ok(list.len() - 1)
}

/// Find or create the animation track for a parameter on an effect.
///
/// Returns the index of the track and whether it had to be created.
#[must_use]
pub fn ensure_anim_track(effect: &mut EffectNode, param: u16) -> (usize, bool) {
    if let Some(idx) = effect.animation.iter().position(|a| a.param == param) {
        return (idx, false);
    }
    effect.animation.push(AnimTrack::new(param));
    (effect.animation.len() - 1, true)
}

/// Convenience: the timeline duration of a clip, in nanoseconds.
#[must_use]
pub fn clip_duration(clip: &Clip) -> Time {
    clip.duration()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Asset, TrackKind};
    use vimo_foundation::time::TimeRange;

    fn project_with_track() -> Project {
        let mut p = Project::new();
        p.assets.push(Asset::video("src", Time::from_secs(30)));
        p.tracks.push(Track::new("V1", TrackKind::Video));
        p
    }

    fn clip() -> Clip {
        Clip::new(
            0,
            TimeRange::new(Time::ZERO, Time::from_secs(2)),
            Time::ZERO,
        )
    }

    #[test]
    fn add_and_remove_clip_roundtrip() {
        let mut p = project_with_track();
        let cmd = Command::add_clip(0, 0, clip());
        let inv = cmd.apply(&mut p).unwrap();
        assert_eq!(p.tracks[0].clips.len(), 1);
        assert!(p.clips[0].alive);

        let inv2 = inv.apply(&mut p).unwrap();
        assert!(p.tracks[0].clips.is_empty());
        assert!(!p.clips[0].alive);

        inv2.apply(&mut p).unwrap();
        assert_eq!(p.tracks[0].clips.len(), 1);
        assert!(p.clips[0].alive);
    }

    #[test]
    fn update_clip_restores_previous_state() {
        let mut p = project_with_track();
        Command::add_clip(0, 0, clip()).apply(&mut p).unwrap();
        let before = p.clips[0].clone();
        let cmd = Command::edit_clip(0, before.clone(), |c| c.opacity = 0.25);
        let inv = cmd.apply(&mut p).unwrap();
        assert_eq!(p.clips[0].opacity, 0.25);
        assert!(p.clips[0].revision > before.revision);
        inv.apply(&mut p).unwrap();
        assert_eq!(p.clips[0].opacity, before.opacity);
    }

    #[test]
    fn asset_removal_is_blocked_while_in_use() {
        let mut p = project_with_track();
        Command::add_clip(0, 0, clip()).apply(&mut p).unwrap();
        assert!(Command::RemoveAsset { index: 0 }.apply(&mut p).is_err());
    }

    #[test]
    fn effect_param_undo() {
        let mut p = project_with_track();
        Command::add_clip(0, 0, clip()).apply(&mut p).unwrap();
        let desc = vimo_manifest::EffectDesc::new(
            "x",
            "x",
            "x",
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
        let node = EffectNode::new(vimo_manifest::EffectTypeId::from_index(0), &desc);
        let cmd = Command::AddEffect {
            owner: EffectOwner::Clip(0),
            effect: node,
        };
        let inv = cmd.apply(&mut p).unwrap();
        let effect_id = inv.effect_id().unwrap();

        let before = p.effects[effect_id as usize].params.get(0).unwrap();
        let set = Command::SetParam {
            effect: effect_id,
            param: 0,
            before,
            after: ParamValue::Float(0.5),
        };
        let inv = set.apply(&mut p).unwrap();
        assert_eq!(
            p.effects[effect_id as usize].params.get(0),
            Some(ParamValue::Float(0.5))
        );
        inv.apply(&mut p).unwrap();
        assert_eq!(p.effects[effect_id as usize].params.get(0), Some(before));
    }

    #[test]
    fn batch_is_atomic_in_reverse() {
        let mut p = project_with_track();
        let batch = Command::Batch(vec![
            Command::add_clip(0, 0, clip()),
            Command::add_clip(0, 1, clip()),
        ]);
        let inv = batch.apply(&mut p).unwrap();
        assert_eq!(p.tracks[0].clips.len(), 2);
        inv.apply(&mut p).unwrap();
        assert!(p.tracks[0].clips.is_empty());
    }
}
