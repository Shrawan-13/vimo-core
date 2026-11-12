//! The project container and the queries the render graph runs against it.

use vimo_foundation::error::{invalid_state, VimoError, VimoResult};
use vimo_foundation::time::{Time, TimeRange};

use crate::model::{
    Asset, AssetId, Clip, ClipId, EffectId, EffectNode, EffectOwner, ProjectMeta, Track, TrackId,
    Transition, TransitionId,
};

/// A Vimo project.
///
/// The project is **pure data**. All mutation goes through
/// [`crate::command::Command`] so that it can be undone, journalled and
/// serialised deterministically.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    /// Project metadata (name, resolution, frame rate).
    pub meta: ProjectMeta,
    /// Referenced media sources.
    pub assets: Vec<Asset>,
    /// Timeline tracks, topmost first.
    pub tracks: Vec<Track>,
    /// All clips. Tracks reference them by index.
    pub clips: Vec<Clip>,
    /// All effect nodes.
    pub effects: Vec<EffectNode>,
    /// All transitions.
    pub transitions: Vec<Transition>,
    /// Global revision counter, bumped on every mutation.
    pub revision: u64,
}

impl Default for Project {
    fn default() -> Self {
        Project::new()
    }
}

impl Project {
    /// An empty project at 1920x1080 / 30 fps.
    #[must_use]
    pub fn new() -> Self {
        Project {
            meta: ProjectMeta::default(),
            assets: Vec::new(),
            tracks: Vec::new(),
            clips: Vec::new(),
            effects: Vec::new(),
            transitions: Vec::new(),
            revision: 0,
        }
    }

    /// Bump the global revision.
    pub fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Get an asset.
    #[must_use]
    pub fn asset(&self, id: AssetId) -> Option<&Asset> {
        self.assets.get(id as usize).filter(|a| a.alive)
    }

    /// Get a track.
    #[must_use]
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.get(id as usize).filter(|t| t.alive)
    }

    /// Get a clip.
    #[must_use]
    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.clips.get(id as usize).filter(|c| c.alive)
    }

    /// Get a clip mutably.
    #[must_use]
    pub fn clip_mut(&mut self, id: ClipId) -> Option<&mut Clip> {
        self.clips.get_mut(id as usize).filter(|c| c.alive)
    }

    /// Get an effect.
    #[must_use]
    pub fn effect(&self, id: EffectId) -> Option<&EffectNode> {
        self.effects.get(id as usize).filter(|e| e.alive)
    }

    /// Get an effect mutably.
    #[must_use]
    pub fn effect_mut(&mut self, id: EffectId) -> Option<&mut EffectNode> {
        self.effects.get_mut(id as usize).filter(|e| e.alive)
    }

    /// Get a transition.
    #[must_use]
    pub fn transition(&self, id: TransitionId) -> Option<&Transition> {
        self.transitions.get(id as usize).filter(|t| t.alive)
    }

    /// Iterate over the live tracks.
    pub fn live_tracks(&self) -> impl Iterator<Item = (TrackId, &Track)> {
        self.tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.alive)
            .map(|(i, t)| (i as TrackId, t))
    }

    /// Iterate over the live clips.
    pub fn live_clips(&self) -> impl Iterator<Item = (ClipId, &Clip)> {
        self.clips
            .iter()
            .enumerate()
            .filter(|(_, c)| c.alive)
            .map(|(i, c)| (i as ClipId, c))
    }

    /// Iterate over the live effects.
    pub fn live_effects(&self) -> impl Iterator<Item = (EffectId, &EffectNode)> {
        self.effects
            .iter()
            .enumerate()
            .filter(|(_, e)| e.alive)
            .map(|(i, e)| (i as EffectId, e))
    }

    /// Find the track a clip belongs to.
    #[must_use]
    pub fn track_of_clip(&self, clip: ClipId) -> Option<TrackId> {
        self.tracks
            .iter()
            .enumerate()
            .find(|(_, t)| t.alive && t.clips.contains(&clip))
            .map(|(i, _)| i as TrackId)
    }

    /// Position of a clip within its track's clip list.
    #[must_use]
    pub fn clip_position(&self, track: TrackId, clip: ClipId) -> Option<usize> {
        self.tracks.get(track as usize)?.clips.iter().position(|c| *c == clip)
    }

    /// The clip that covers `time` on `track`, if any.
    ///
    /// Clips on a track are kept sorted and non-overlapping, so this is a
    /// binary search — the render graph calls it once per track per frame.
    #[must_use]
    pub fn active_clip(&self, track: TrackId, time: Time) -> Option<ClipId> {
        let t = self.tracks.get(track as usize).filter(|t| t.alive)?;
        let mut lo = 0usize;
        let mut hi = t.clips.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            let clip = self.clips.get(t.clips[mid] as usize).filter(|c| c.alive)?;
            if clip.timeline.start > time {
                hi = mid;
            } else if clip.timeline.end <= time {
                lo = mid + 1;
            } else {
                return Some(t.clips[mid]);
            }
        }
        None
    }

    /// All clips that overlap `range` on `track`.
    #[must_use]
    pub fn clips_overlapping(&self, track: TrackId, range: TimeRange) -> Vec<ClipId> {
        let mut out = Vec::new();
        if let Some(t) = self.tracks.get(track as usize) {
            for &cid in &t.clips {
                if let Some(c) = self.clips.get(cid as usize) {
                    if c.timeline.overlaps(range) {
                        out.push(cid);
                    }
                }
            }
        }
        out
    }

    /// The transition that covers `time` on `track`, if any.
    #[must_use]
    pub fn active_transition(&self, track: TrackId, time: Time) -> Option<&Transition> {
        self.transitions.iter().find(|t| {
            t.track == track
                && t.duration > Time::ZERO
                && t.from_clip != t.to_clip
                && self
                    .clip(t.from_clip)
                    .is_some_and(|c| time >= c.timeline.start && time < c.timeline.start + t.duration)
        })
    }

    /// Total duration: the end of the last clip on any track.
    #[must_use]
    pub fn duration(&self) -> Time {
        self.meta.duration.max(self.compute_duration())
    }

    /// Recompute the duration from the clips and store it in the metadata.
    pub fn recompute_duration(&mut self) {
        let d = self.compute_duration();
        self.meta.duration = d;
    }

    /// Duration computed from the clips.
    #[must_use]
    pub fn compute_duration(&self) -> Time {
        let mut end = Time::ZERO;
        for c in &self.clips {
            if c.timeline.end > end {
                end = c.timeline.end;
            }
        }
        end
    }

    /// Find an asset by its platform source id.
    #[must_use]
    pub fn asset_by_source(&self, source_id: &str) -> Option<AssetId> {
        self.assets
            .iter()
            .position(|a| a.source_id == source_id)
            .map(|i| i as AssetId)
    }

    /// Find the owner of an effect node.
    #[must_use]
    pub fn effect_owner(&self, effect: EffectId) -> Option<EffectOwner> {
        for (i, c) in self.clips.iter().enumerate() {
            if c.effects.contains(&effect) {
                return Some(EffectOwner::Clip(i as ClipId));
            }
        }
        for (i, t) in self.tracks.iter().enumerate() {
            if t.effects.contains(&effect) {
                return Some(EffectOwner::Track(i as TrackId));
            }
        }
        None
    }

    /// Check referential integrity.
    ///
    /// Returns an error describing the first problem found. Callers should run
    /// this after loading a project from disk.
    pub fn validate(&self) -> VimoResult<()> {
        for c in self.clips.iter().filter(|c| c.alive) {
            if c.asset as usize >= self.assets.len() || !self.assets[c.asset as usize].alive {
                return Err(invalid_state("project: clip references a missing asset"));
            }
        }
        for t in self.tracks.iter().filter(|t| t.alive) {
            for &cid in &t.clips {
                if cid as usize >= self.clips.len() {
                    return Err(invalid_state("project: track references a missing clip"));
                }
            }
            for &eid in &t.effects {
                if eid as usize >= self.effects.len() || !self.effects[eid as usize].alive {
                    return Err(invalid_state("project: track references a missing effect"));
                }
            }
        }
        for c in self.clips.iter().filter(|c| c.alive) {
            for &eid in &c.effects {
                if eid as usize >= self.effects.len() || !self.effects[eid as usize].alive {
                    return Err(invalid_state("project: clip references a missing effect"));
                }
            }
            if c.timeline.end < c.timeline.start {
                return Err(invalid_state("project: clip has a negative duration"));
            }
        }
        for t in self.transitions.iter().filter(|t| t.alive) {
            if t.from_clip as usize >= self.clips.len()
                || t.to_clip as usize >= self.clips.len()
                || !self.clips[t.from_clip as usize].alive
                || !self.clips[t.to_clip as usize].alive
            {
                return Err(invalid_state("project: transition references a missing clip"));
            }
        }
        Ok(())
    }

    /// Total number of effect instances.
    #[must_use]
    pub fn effect_count(&self) -> usize {
        self.effects.iter().filter(|e| e.alive).count()
    }

    /// A compact fingerprint of the project structure.
    ///
    /// The render graph compares this against the fingerprint of the last frame
    /// it built. Because it mixes every structural field, any change to the
    /// timeline produces a different value. It is deliberately *not* a
    /// cryptographic hash — it only has to detect "did anything change".
    #[must_use]
    pub fn structural_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| {
            h ^= v;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        mix(self.tracks.len() as u64);
        mix(self.clips.len() as u64);
        mix(self.effects.len() as u64);
        mix(self.transitions.len() as u64);
        mix(self.assets.len() as u64);
        mix(self.meta.width as u64);
        mix(self.meta.height as u64);
        for c in self.clips.iter().filter(|c| c.alive) {
            mix(c.revision);
            mix(c.timeline.start.as_nanos() as u64);
            mix(c.timeline.end.as_nanos() as u64);
            mix(c.source_in.as_nanos() as u64);
            mix(c.speed.to_bits() as u64);
            mix(c.opacity.to_bits() as u64);
            mix(c.blend.as_u8() as u64);
            mix(c.enabled as u64);
            mix(c.effects.len() as u64);
        }
        for e in self.effects.iter().filter(|e| e.alive) {
            mix(e.revision);
            mix(e.enabled as u64);
            mix(e.animation.len() as u64);
        }
        for t in self.transitions.iter().filter(|t| t.alive) {
            mix(t.duration.as_nanos() as u64);
            mix(t.kind.as_u8() as u64);
        }
        h
    }
}

/// Convenience alias for a project-level operation result.
pub type ProjectResult<T> = Result<T, VimoError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Asset, TrackKind};

    fn sample() -> Project {
        let mut p = Project::new();
        p.assets.push(Asset::video("src-0", Time::from_secs(10)));
        let mut track = Track::new("V1", TrackKind::Video);
        track.clips.push(0);
        p.tracks.push(track);
        let clip = Clip::new(
            0,
            TimeRange::new(Time::from_secs(1), Time::from_secs(3)),
            Time::ZERO,
        );
        p.clips.push(clip);
        p
    }

    #[test]
    fn active_clip_uses_binary_search() {
        let p = sample();
        assert_eq!(p.active_clip(0, Time::from_secs(0)), None);
        assert_eq!(p.active_clip(0, Time::from_secs(1)), Some(0));
        assert_eq!(p.active_clip(0, Time::from_secs(2)), Some(0));
        assert_eq!(p.active_clip(0, Time::from_secs(3)), None);
    }

    #[test]
    fn duration_is_computed_from_clips() {
        let p = sample();
        assert_eq!(p.compute_duration(), Time::from_secs(3));
    }

    #[test]
    fn validation_catches_dangling_references() {
        let mut p = sample();
        p.clips[0].asset = 42;
        assert!(p.validate().is_err());
    }

    #[test]
    fn fingerprint_changes_when_a_clip_moves() {
        let mut p = sample();
        let a = p.structural_fingerprint();
        p.clips[0].timeline.start = Time::from_secs(2);
        let b = p.structural_fingerprint();
        assert_ne!(a, b);
    }
}
