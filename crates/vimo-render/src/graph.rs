//! The render graph.
//!
//! # What a frame costs
//!
//! Building a frame is split into two very different kinds of work:
//!
//! * **Structural work** — deciding which clips are on which tracks, their
//!   transforms, their effect chains. This is done by the *layer table* and is
//!   rebuilt only when the project structure changes.
//! * **Per-frame work** — finding the active clip on each track (one binary
//!   search per track), evaluating animated effect parameters and filling in
//!   the uniforms.
//!
//! On a low-end device the per-frame work is what matters, so it is kept
//! allocation free: the plan, the layer table and every uniform block are
//! reused between frames.
//!
//! # Dirty tracking
//!
//! Three levels of caching keep the per-frame cost proportional to what
//! actually changed:
//!
//! 1. A **structure fingerprint** of the project. When it changes, the layer
//!    table is rebuilt.
//! 2. Per-layer **clip revisions**. A clip that has not changed keeps its
//!    cached transform, opacity and blend.
//! 3. Per-effect **revisions**. Static effects keep their uniform block;
//!    only animated effects are re-evaluated every frame.

use vimo_foundation::error::VimoResult;
use vimo_foundation::math::{Mat3, Rect};
use vimo_foundation::time::Time;
use vimo_manifest::{EffectRegistry, EffectTypeId, ParamValue};
use vimo_project::model::{BlendMode, ClipId, TransitionKind};
use vimo_project::Project;

use crate::plan::{DrawStep, EffectIndex, FrameEffect, FramePlan, LayerIndex, TextureRef};
use crate::shader::ShaderProvider;
use crate::uniforms::{write_param, BuiltinUniforms, UniformBlock};

/// Statistics about the graph's work, useful for benchmarks and telemetry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GraphStats {
    /// Number of frames produced.
    pub frames: u64,
    /// Number of times the layer table was rebuilt.
    pub rebuilds: u64,
    /// Number of layers emitted.
    pub layers: u64,
    /// Number of effect uniform blocks rebuilt.
    pub uniform_updates: u64,
    /// Number of times a cached frame was reused verbatim.
    pub cache_hits: u64,
}

/// A clip slot in the layer table.
#[derive(Debug, Clone, Copy)]
struct ClipSlot {
    clip: ClipId,
    start: Time,
    end: Time,
    /// The revision of the clip when the slot was built.
    revision: u64,
}

/// A transition slot in the layer table.
#[derive(Debug, Clone, Copy)]
struct TransitionSlot {
    transition: u32,
    start: Time,
    end: Time,
    from_clip: ClipId,
    to_clip: ClipId,
}

/// The cached state of one effect instance.
#[derive(Debug, Clone)]
struct EffectSlot {
    effect: u32,
    effect_type: EffectTypeId,
    /// Whether the effect has any animation track.
    animated: bool,
    /// The revision of the effect when the uniforms were built.
    revision: u64,
    /// The evaluated parameter values (one per declared parameter).
    values: Vec<ParamValue>,
    /// The uniform block uploaded for this effect.
    uniforms: UniformBlock,
}

/// The cached state of one layer.
#[derive(Debug, Clone, Default)]
struct LayerState {
    clip: ClipId,
    clip_revision: u64,
    transform: Mat3,
    opacity: f32,
    blend: BlendMode,
    /// The effect chain, in application order.
    effects: Vec<EffectSlot>,
    /// Whether the layer was refreshed this frame.
    dirty: bool,
}

/// The static structure of the render: which clips live where.
#[derive(Debug, Default)]
struct LayerTable {
    /// All clip slots, grouped by track.
    slots: Vec<ClipSlot>,
    /// Per track: `(start index, count)` into `slots`.
    track_ranges: Vec<(u32, u32)>,
    /// Per track: the transitions on that track.
    transitions: Vec<Vec<TransitionSlot>>,
}

impl LayerTable {
    /// Rebuild from the project.
    fn build(project: &Project) -> Self {
        let mut table = LayerTable {
            slots: Vec::new(),
            track_ranges: Vec::with_capacity(project.tracks.len()),
            transitions: Vec::with_capacity(project.tracks.len()),
        };
        for (ti, track) in project.tracks.iter().enumerate() {
            if !track.alive {
                table.track_ranges.push((0, 0));
                table.transitions.push(Vec::new());
                continue;
            }
            let start = table.slots.len() as u32;
            let mut count = 0u32;
            for &cid in &track.clips {
                let Some(clip) = project.clips.get(cid as usize) else {
                    continue;
                };
                if !clip.alive {
                    continue;
                }
                table.slots.push(ClipSlot {
                    clip: cid,
                    start: clip.timeline.start,
                    end: clip.timeline.end,
                    revision: clip.revision,
                });
                count += 1;
            }
            table.track_ranges.push((start, count));

            let mut transitions = Vec::new();
            for (ti2, tr) in project.transitions.iter().enumerate() {
                if !tr.alive || tr.track as usize != ti || tr.duration <= Time::ZERO {
                    continue;
                }
                let (Some(from), Some(to)) = (
                    project.clips.get(tr.from_clip as usize),
                    project.clips.get(tr.to_clip as usize),
                ) else {
                    continue;
                };
                if !from.alive || !to.alive {
                    continue;
                }
                // The transition is centred on the cut.
                let cut = to.timeline.start.max(from.timeline.end);
                let half = Time::from_nanos(tr.duration.as_nanos() / 2);
                transitions.push(TransitionSlot {
                    transition: ti2 as u32,
                    start: cut - half,
                    end: cut + half,
                    from_clip: tr.from_clip,
                    to_clip: tr.to_clip,
                });
            }
            transitions.sort_by_key(|t| t.start);
            table.transitions.push(transitions);
        }
        table
    }

    /// Find the clip active on `track` at `time`.
    fn active(&self, track: u32, time: Time) -> Option<ClipSlot> {
        let &(start, count) = self.track_ranges.get(track as usize)?;
        if count == 0 {
            return None;
        }
        let slots = &self.slots[start as usize..(start + count) as usize];
        // The slots are sorted by start time within a track.
        let mut lo = 0usize;
        let mut hi = slots.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            let s = slots[mid];
            if s.start > time {
                hi = mid;
            } else if s.end <= time {
                lo = mid + 1;
            } else {
                return Some(s);
            }
        }
        None
    }

    /// Find the transition covering `time` on `track`.
    fn transition_at(&self, track: u32, time: Time) -> Option<TransitionSlot> {
        let list = self.transitions.get(track as usize)?;
        list.iter()
            .find(|t| time >= t.start && time < t.end)
            .copied()
    }
}

/// The render graph.
///
/// Owns the project, the effect registry and the cached layer state.
pub struct RenderGraph {
    project: Project,
    registry: EffectRegistry,
    table: LayerTable,
    /// The structure fingerprint the table was built from.
    fingerprint: u64,
    /// Per-slot layer state, indexed the same way as `table.slots`.
    states: Vec<LayerState>,
    /// The reusable plan.
    plan: FramePlan,
    /// Output resolution.
    width: u32,
    height: u32,
    /// Preview scale (1.0 = full resolution).
    scale: f32,
    /// Whether the plan is for preview (allows cheaper effects).
    preview: bool,
    /// Optional shader provider used to resolve programs.
    shader_provider: Option<Box<dyn ShaderProvider>>,
    /// Statistics.
    stats: GraphStats,
    /// The last time a plan was produced for.
    last_time: Time,
    /// Whether the last plan is still valid.
    last_valid: bool,
}

impl RenderGraph {
    /// Create a graph for `project`.
    #[must_use]
    pub fn new(project: Project, registry: EffectRegistry) -> Self {
        let width = project.meta.width;
        let height = project.meta.height;
        let mut graph = RenderGraph {
            project,
            registry,
            table: LayerTable::default(),
            fingerprint: 0,
            states: Vec::new(),
            plan: FramePlan::new(),
            width,
            height,
            scale: 1.0,
            preview: true,
            shader_provider: None,
            stats: GraphStats::default(),
            last_time: Time::ZERO,
            last_valid: false,
        };
        graph.invalidate();
        graph
    }

    /// Attach a shader provider.
    pub fn set_shader_provider(&mut self, provider: Box<dyn ShaderProvider>) {
        self.shader_provider = Some(provider);
    }

    /// The project.
    #[must_use]
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// The project, mutably. Mutating the project invalidates the layer table.
    #[must_use]
    pub fn project_mut(&mut self) -> &mut Project {
        self.invalidate();
        &mut self.project
    }

    /// The effect registry.
    #[must_use]
    pub fn registry(&self) -> &EffectRegistry {
        &self.registry
    }

    /// Set the output resolution.
    pub fn set_resolution(&mut self, width: u32, height: u32) {
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.last_valid = false;
        }
    }

    /// Set the preview scale factor.
    ///
    /// `0.5` renders at half resolution. The scale is applied on top of the
    /// output resolution.
    pub fn set_preview_scale(&mut self, scale: f32) {
        let s = scale.clamp(0.0625, 1.0);
        if (self.scale - s).abs() > f32::EPSILON {
            self.scale = s;
            self.last_valid = false;
        }
    }

    /// Switch between preview and export mode.
    pub fn set_preview_mode(&mut self, preview: bool) {
        if self.preview != preview {
            self.preview = preview;
            self.last_valid = false;
        }
    }

    /// Force the layer table to be rebuilt on the next frame.
    pub fn invalidate(&mut self) {
        self.fingerprint = 0;
        self.last_valid = false;
    }

    /// Statistics.
    #[must_use]
    pub fn stats(&self) -> GraphStats {
        self.stats
    }

    /// The current plan.
    #[must_use]
    pub fn plan(&self) -> &FramePlan {
        &self.plan
    }

    /// The current plan, mutably.
    ///
    /// The renderer uses this to fill in the resolved texture handles.
    #[must_use]
    pub fn plan_mut(&mut self) -> &mut FramePlan {
        &mut self.plan
    }

    /// Build the frame at `time` and return the plan.
    pub fn frame_at(&mut self, time: Time) -> VimoResult<&FramePlan> {
        self.build(time)?;
        Ok(&self.plan)
    }

    /// Build the frame at `time` into an externally owned plan.
    ///
    /// Useful when the caller wants to keep several plans alive (for example a
    /// preview plan and an export plan).
    pub fn frame_at_into(&mut self, time: Time, plan: &mut FramePlan) -> VimoResult<()> {
        self.build(time)?;
        plan.time = self.plan.time;
        plan.width = self.plan.width;
        plan.height = self.plan.height;
        plan.scale = self.plan.scale;
        plan.preview = self.plan.preview;
        plan.layer_count = self.plan.layer_count;
        plan.steps.clear();
        plan.steps.extend_from_slice(&self.plan.steps);
        Ok(())
    }

    fn build(&mut self, time: Time) -> VimoResult<()> {
        self.stats.frames += 1;

        // 1. Rebuild the layer table if the structure changed.
        let fingerprint = self.project.structural_fingerprint();
        if fingerprint != self.fingerprint {
            self.table = LayerTable::build(&self.project);
            self.states.clear();
            self.states
                .resize(self.table.slots.len(), LayerState::default());
            self.fingerprint = fingerprint;
            self.stats.rebuilds += 1;
            self.last_valid = false;
        }

        // 2. Nothing changed at all: reuse the previous plan verbatim.
        if self.last_valid && self.last_time == time {
            self.stats.cache_hits += 1;
            return Ok(());
        }

        let width = ((self.width as f32 * self.scale).round() as u32).max(1);
        let height = ((self.height as f32 * self.scale).round() as u32).max(1);
        self.plan
            .reset(time, width, height, self.scale, self.preview);
        self.plan.steps.push(DrawStep::Clear {
            color: self.project.meta.background,
        });

        // 3. Walk the tracks bottom to top.
        let track_count = self.project.tracks.len() as u32;
        for track in 0..track_count {
            let Some(t) = self.project.tracks.get(track as usize) else {
                continue;
            };
            if !t.alive || !t.enabled {
                continue;
            }
            if self.table.transition_at(track, time).is_some() {
                continue; // handled below, as a transition step
            }
            let Some(slot) = self.table.active(track, time) else {
                continue;
            };
            let layer_index = LayerIndex(slot.clip);
            self.emit_layer(slot, time, layer_index)?;
        }

        // 4. Transitions.
        for track in 0..track_count {
            let Some(t) = self.project.tracks.get(track as usize) else {
                continue;
            };
            if !t.alive || !t.enabled {
                continue;
            }
            let Some(tr) = self.table.transition_at(track, time) else {
                continue;
            };
            self.emit_transition(tr, time)?;
        }

        self.plan.layer_count = self.stats.layers as u32;
        self.last_time = time;
        self.last_valid = true;
        Ok(())
    }

    /// Emit a composite step for one clip.
    fn emit_layer(
        &mut self,
        slot: ClipSlot,
        time: Time,
        layer_index: LayerIndex,
    ) -> VimoResult<()> {
        let state_index = slot.clip as usize;
        // Refresh the layer state if the clip changed.
        {
            let state = &mut self.states[state_index];
            if state.clip != slot.clip || state.clip_revision != slot.revision {
                let Some(clip) = self.project.clips.get(slot.clip as usize) else {
                    return Ok(());
                };
                state.clip = slot.clip;
                state.clip_revision = clip.revision;
                state.transform = clip.transform.to_matrix();
                state.opacity = clip.opacity;
                state.blend = clip.blend;
                state.effects.clear();
                state.dirty = true;
            }
        }

        let local_time = time - self.project.clips[slot.clip as usize].timeline.start;
        self.refresh_effects(slot.clip, local_time)?;

        // Copy the effect uniforms into the plan's arena so that the plan is
        // self-contained and can be drawn on another thread.
        let mut effects = Vec::new();
        for slot in &self.states[state_index].effects {
            let range = self.plan.uniforms.push_block(&slot.uniforms);
            let index = self.plan.effects.len() as u32;
            self.plan.effects.push(FrameEffect {
                effect_type: slot.effect_type,
                builtins: slot.uniforms.builtins,
                range,
            });
            effects.push(EffectIndex(index));
        }
        let effects = effects;

        let clip = &self.project.clips[slot.clip as usize];
        let source_time = source_time_for(clip, time);

        self.plan.steps.push(DrawStep::Composite {
            layer: layer_index,
            source: TextureRef::unresolved(clip_id_of(slot), source_time),
            transform: self.states[state_index].transform,
            src_rect: Rect::FULL,
            dst_rect: Rect::FULL,
            blend: self.states[state_index].blend,
            opacity: self.states[state_index].opacity,
            effects,
        });
        self.stats.layers += 1;
        Ok(())
    }

    /// Emit a transition step.
    fn emit_transition(&mut self, tr: TransitionSlot, time: Time) -> VimoResult<()> {
        let Some(transition) = self.project.transitions.get(tr.transition as usize) else {
            return Ok(());
        };
        let kind = transition.kind;
        let (Some(from), Some(to)) = (
            self.project.clips.get(tr.from_clip as usize),
            self.project.clips.get(tr.to_clip as usize),
        ) else {
            return Ok(());
        };
        let span = tr.end - tr.start;
        let progress = if span <= Time::ZERO {
            1.0
        } else {
            ((time - tr.start).as_nanos() as f64 / span.as_nanos() as f64).clamp(0.0, 1.0) as f32
        };
        let from_source = TextureRef::unresolved(tr.from_clip, source_time_for(from, time));
        let to_source = TextureRef::unresolved(tr.to_clip, source_time_for(to, time));
        let transform = to.transform.to_matrix();

        // A transition may carry its own effect chain in the future; for now it
        // inherits the incoming clip's chain so that the transition honours the
        // same look as the clip it leads into.
        let local = time - to.timeline.start;
        self.refresh_effects(tr.to_clip, local)?;
        let state_index = tr.to_clip as usize;
        let mut effects = Vec::new();
        for slot in &self.states[state_index].effects {
            let range = self.plan.uniforms.push_block(&slot.uniforms);
            let index = self.plan.effects.len() as u32;
            self.plan.effects.push(FrameEffect {
                effect_type: slot.effect_type,
                builtins: slot.uniforms.builtins,
                range,
            });
            effects.push(EffectIndex(index));
        }

        self.plan.steps.push(DrawStep::Transition {
            layer: LayerIndex(tr.to_clip),
            from_source,
            to_source,
            transform,
            src_rect: Rect::FULL,
            dst_rect: Rect::FULL,
            kind,
            progress,
            effects,
        });
        self.stats.layers += 1;
        Ok(())
    }

    /// Refresh the effect chain of a clip at `local_time`.
    ///
    /// Static effects keep their uniform block; animated effects are
    /// re-evaluated every frame.
    fn refresh_effects(&mut self, clip: ClipId, local_time: Time) -> VimoResult<()> {
        let Some(clip_data) = self.project.clips.get(clip as usize) else {
            return Ok(());
        };
        let effect_ids: Vec<u32> = clip_data.effects.clone();
        let state_index = clip as usize;

        // Rebuild the chain if the set of effects changed.
        let chain_changed = self.states[state_index].effects.len() != effect_ids.len()
            || self.states[state_index]
                .effects
                .iter()
                .zip(&effect_ids)
                .any(|(s, id)| s.effect != *id);

        if chain_changed {
            self.states[state_index].effects.clear();
            for id in effect_ids {
                let Some(node) = self.project.effects.get(id as usize) else {
                    continue;
                };
                self.states[state_index].effects.push(EffectSlot {
                    effect: id,
                    effect_type: node.effect_type,
                    animated: !node.animation.is_empty(),
                    revision: node.revision,
                    values: Vec::new(),
                    uniforms: UniformBlock::new(),
                });
            }
            self.states[state_index].dirty = true;
        }

        let width = self.width as f32 * self.scale;
        let height = self.height as f32 * self.scale;
        for slot in &mut self.states[state_index].effects {
            let Some(node) = self.project.effects.get(slot.effect as usize) else {
                continue;
            };
            let desc = self.registry.get(node.effect_type);
            if slot.animated || slot.revision != node.revision || slot.values.is_empty() {
                build_effect_uniforms(slot, node, desc, local_time, width, height);
                slot.revision = node.revision;
                self.stats.uniform_updates += 1;
            }
        }
        Ok(())
    }

    /// The uniform block of an effect inside a clip's chain.
    #[must_use]
    pub fn effect_uniforms(&self, clip: ClipId, chain_index: u32) -> Option<&UniformBlock> {
        let state = self.states.get(clip as usize)?;
        state.effects.get(chain_index as usize).map(|s| &s.uniforms)
    }

    /// The effect type behind a chain entry.
    #[must_use]
    pub fn effect_type_at(&self, clip: ClipId, chain_index: u32) -> Option<EffectTypeId> {
        let state = self.states.get(clip as usize)?;
        state
            .effects
            .get(chain_index as usize)
            .map(|s| s.effect_type)
    }
}

/// Evaluate an effect's parameters and build its uniform block.
fn build_effect_uniforms(
    slot: &mut EffectSlot,
    node: &vimo_project::model::EffectNode,
    desc: Option<&vimo_manifest::EffectDesc>,
    local_time: Time,
    width: f32,
    height: f32,
) {
    slot.uniforms.clear();
    slot.uniforms.builtins = BuiltinUniforms {
        time: local_time.as_secs_f64() as f32,
        resolution: [width, height],
        texel: [1.0 / width.max(1.0), 1.0 / height.max(1.0)],
        opacity: 1.0,
    };

    let Some(desc) = desc else {
        return;
    };
    if slot.values.len() != desc.params.len() {
        slot.values = desc.params.iter().map(|p| p.default).collect();
    }
    // Animated parameters override the stored value at the current time.
    for anim in &node.animation {
        let Some(param) = desc.param(anim.param) else {
            continue;
        };
        if !param.animatable {
            continue;
        }
        if let Some(v) = anim.sample(local_time) {
            slot.values[anim.param as usize] = v;
        }
    }
    // Write every parameter in declaration order.
    for (i, p) in desc.params.iter().enumerate() {
        let value = slot.values[i];
        write_param(&mut slot.uniforms, p.ptype, value);
    }
}

/// The source-media time a clip shows at `time`.
#[must_use]
pub fn source_time_for(clip: &vimo_project::model::Clip, time: Time) -> Time {
    let elapsed = time - clip.timeline.start;
    let speed = if clip.speed.abs() < f32::EPSILON {
        1.0
    } else {
        clip.speed
    };
    let delta = Time::from_secs_f64(elapsed.as_secs_f64() * speed as f64);
    if clip.forward {
        clip.source_in + delta
    } else {
        clip.source_in - delta
    }
}

fn clip_id_of(slot: ClipSlot) -> ClipId {
    slot.clip
}

/// The transition kinds that the CPU reference rasteriser can draw.
pub fn transition_is_supported(kind: TransitionKind) -> bool {
    matches!(
        kind,
        TransitionKind::CrossDissolve | TransitionKind::DipToBlack | TransitionKind::LumaDissolve
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use vimo_foundation::time::TimeRange;
    use vimo_project::model::{Asset, Clip, Track, TrackKind};

    fn build_project() -> (Project, EffectRegistry) {
        let mut p = Project::new();
        p.assets.push(Asset::video("src", Time::from_secs(30)));
        p.tracks.push(Track::new("V1", TrackKind::Video));
        p.tracks.push(Track::new("V2", TrackKind::Video));
        p.clips.push(Clip::new(
            0,
            TimeRange::new(Time::ZERO, Time::from_secs(5)),
            Time::ZERO,
        ));
        p.clips.push(Clip::new(
            0,
            TimeRange::new(Time::from_secs(5), Time::from_secs(10)),
            Time::ZERO,
        ));
        p.tracks[0].clips.push(0);
        p.tracks[1].clips.push(1);
        (p, EffectRegistry::new())
    }

    #[test]
    fn frame_lists_the_active_clips() {
        let (p, reg) = build_project();
        let mut g = RenderGraph::new(p, reg);
        let plan = g.frame_at(Time::from_secs(1)).unwrap();
        // Clear + one composite (only V1 has a clip at t=1s).
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.layer_count, 1);
    }

    #[test]
    fn both_tracks_contribute_when_they_overlap() {
        let (mut p, reg) = build_project();
        // Extend the second clip so that it overlaps the first one.
        p.clips[1].timeline = TimeRange::new(Time::from_secs(2), Time::from_secs(10));
        let mut g = RenderGraph::new(p, reg);
        let plan = g.frame_at(Time::from_secs(3)).unwrap();
        assert_eq!(plan.layer_count, 2);
        assert_eq!(plan.steps.len(), 3);
    }

    #[test]
    fn only_the_active_track_contributes() {
        let (p, reg) = build_project();
        let mut g = RenderGraph::new(p, reg);
        let plan = g.frame_at(Time::from_secs(7)).unwrap();
        assert_eq!(plan.layer_count, 1);
    }

    #[test]
    fn repeated_frames_reuse_the_plan() {
        let (p, reg) = build_project();
        let mut g = RenderGraph::new(p, reg);
        g.frame_at(Time::from_secs(1)).unwrap();
        let before = g.stats().cache_hits;
        g.frame_at(Time::from_secs(1)).unwrap();
        assert_eq!(g.stats().cache_hits, before + 1);
    }

    #[test]
    fn preview_scale_changes_the_output_size() {
        let (p, reg) = build_project();
        let mut g = RenderGraph::new(p, reg);
        g.set_preview_scale(0.5);
        let plan = g.frame_at(Time::from_secs(1)).unwrap();
        assert_eq!(plan.width, 960);
        assert_eq!(plan.height, 540);
    }

    #[test]
    fn editing_the_project_rebuilds_the_layer_table() {
        let (p, reg) = build_project();
        let mut g = RenderGraph::new(p, reg);
        g.frame_at(Time::from_secs(1)).unwrap();
        let rebuilds = g.stats().rebuilds;
        {
            let project = g.project_mut();
            project.clips[0].timeline = TimeRange::new(Time::ZERO, Time::from_secs(2));
        }
        g.frame_at(Time::from_secs(1)).unwrap();
        assert!(g.stats().rebuilds > rebuilds);
    }

    #[test]
    fn source_time_follows_speed() {
        let clip = Clip::new(
            0,
            TimeRange::new(Time::ZERO, Time::from_secs(4)),
            Time::from_secs(1),
        );
        let mut clip = clip;
        clip.speed = 2.0;
        let t = source_time_for(&clip, Time::from_secs(1));
        assert_eq!(t, Time::from_secs(3));
    }
}
