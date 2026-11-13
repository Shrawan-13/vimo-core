//! The engine: the object that owns everything.
//!
//! The engine is the only thing the FFI layer talks to. It owns the project,
//! the render graph, the playback clock, the resource pools and the export
//! pipeline, and it emits **events** for the UI.
//!
//! ```text
//!   commands in                                          events out
//!  ───────────►   ┌────────────────────────────────┐   ─────────────►
//!   (FFI / UI)    │  Engine                         │   (FFI / UI)
//!                 │   ├─ Session (project + history)│
//!                 │   ├─ RenderGraph                │
//!                 │   ├─ MasterClock + Prefetcher   │
//!                 │   ├─ ResourceManager            │
//!                 │   └─ ExportPipeline             │
//!                 └────────────────────────────────┘
//! ```

use std::collections::VecDeque;

use vimo_device::{DeviceProfile, QualitySettings, ThermalMonitor, ThermalReading, ThermalState};
use vimo_foundation::error::{invalid_state, VimoResult};
use vimo_foundation::map::FxHashMap;
use vimo_foundation::time::Time;
use vimo_manifest::EffectRegistry;
use vimo_project::{Command, History, Project};
use vimo_render::{FramePlan, RenderGraph, TextureResolver};
use vimo_resources::{FrameCache, FrameKey, MemoryTracker, PressureLevel};

use crate::clock::{MasterClock, PlaybackState, TimeSource};
use crate::export::{ExportPipeline, ExportPreset, ExportProgress, ExportState};
use crate::files::SessionFiles;
use crate::prefetch::{ActiveSource, Prefetcher};
use crate::render_thread::RenderThread;

/// An event emitted by the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// The project changed (an edit was applied).
    ProjectChanged {
        /// The new global revision.
        revision: u64,
    },
    /// The playback state changed.
    PlaybackStateChanged(PlaybackState),
    /// The playhead moved.
    PositionChanged(Time),
    /// A frame was rendered.
    FrameRendered {
        /// The time of the frame.
        time: Time,
        /// Whether the frame was reused from the cache.
        reused: bool,
    },
    /// The memory pressure level changed.
    MemoryPressure(PressureLevel),
    /// The thermal state changed.
    Thermal(ThermalState),
    /// An export progressed.
    ExportProgress(ExportProgress),
    /// An export finished.
    ExportFinished(ExportState),
    /// Something went wrong.
    Error {
        /// The error code (stable across releases).
        code: i32,
        /// A human readable description.
        message: String,
    },
    /// A JSON snapshot of the project is available.
    Snapshot {
        /// The snapshot, as JSON.
        json: String,
    },
}

/// Statistics about playback.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlaybackStats {
    /// The number of frames rendered.
    pub frames_rendered: u64,
    /// The number of frames that were dropped because the renderer fell behind.
    pub frames_dropped: u64,
    /// The number of frames that were held (repeated) because the renderer was
    /// ahead of the clock.
    pub frames_held: u64,
    /// The number of cache hits.
    pub cache_hits: u64,
    /// The number of cache misses.
    pub cache_misses: u64,
}

/// The engine.
pub struct Engine {
    project: Project,
    history: History,
    graph: RenderGraph,
    clock: MasterClock,
    prefetcher: Prefetcher,
    cache: FrameCache,
    memory: MemoryTracker,
    profile: DeviceProfile,
    thermal: ThermalMonitor,
    quality: QualitySettings,
    events: VecDeque<EngineEvent>,
    stats: PlaybackStats,
    /// The last rendered time, used to detect dropped frames.
    last_render_time: Option<Time>,
    /// Interned source ids: the media source name → a dense `u64`.
    source_ids: FxHashMap<String, u64>,
    /// The next interned source id.
    next_source_id: u64,
    /// The duration of one output frame.
    frame_duration: Time,
    export: Option<ExportPipeline>,
    /// Whether the project has unsaved changes.
    dirty: bool,
    /// The file services (load, save, autosave).
    files: SessionFiles,
    /// Resolves the pictures a plan refers to into GPU textures.
    resolver: Option<Box<dyn TextureResolver>>,
    /// The plan shared with the render thread.
    render_plan: std::sync::Arc<std::sync::Mutex<FramePlan>>,
    /// The render thread, if one is attached.
    render_thread: Option<RenderThread>,
    /// The effect registry together with the shader sources.
    shaders: vimo_gl::ShaderLibrary,
}

impl Engine {
    /// Open a project.
    pub fn open(
        project: Project,
        registry: EffectRegistry,
        profile: DeviceProfile,
        source: std::sync::Arc<dyn TimeSource>,
    ) -> Self {
        let quality = profile.quality();
        let memory = MemoryTracker::new(profile.cache_budget_bytes());
        let cache = FrameCache::new(profile.cache_budget_bytes() / 2);
        let duration = project.duration();
        let frame_duration = {
            let fps = project.meta.timebase.fps().as_f64().max(1.0);
            Time::from_secs_f64(1.0 / fps)
        };
        let mut clock = MasterClock::new(source);
        clock.set_duration(duration);
        let graph = RenderGraph::new(project.clone(), registry.clone());
        let mut shaders = vimo_gl::ShaderLibrary::new();
        // The registry lives in the shader library, so that the effects and
        // the code that implements them cannot drift apart.
        *shaders.registry_mut() = registry;
        let mut engine = Engine {
            project,
            history: History::new(),
            graph,
            clock,
            prefetcher: Prefetcher::new(quality.prefetch_frames),
            cache,
            memory,
            profile,
            thermal: ThermalMonitor::new(),
            quality,
            events: VecDeque::new(),
            stats: PlaybackStats::default(),
            last_render_time: None,
            source_ids: FxHashMap::default(),
            next_source_id: 1,
            frame_duration,
            export: None,
            dirty: false,
            files: SessionFiles::new(),
            resolver: None,
            render_plan: std::sync::Arc::new(std::sync::Mutex::new(FramePlan::new())),
            render_thread: None,
            shaders,
        };
        engine.intern_sources();
        engine
    }

    /// The project.
    #[must_use]
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Start a new, empty project.
    ///
    /// The resolution and the frame rate are kept: they are the settings of
    /// the workspace, not of the project. The tracks are the default ones, so
    /// that there is always somewhere to put a clip.
    pub fn new_project(&mut self) {
        let meta = self.project.meta.clone();
        let mut project = Project::with_default_tracks();
        project.meta = meta;
        self.project = project;
        self.history = History::new();
        self.dirty = false;
        self.sync_graph();
        let revision = self.project.revision;
        self.events
            .push_back(EngineEvent::ProjectChanged { revision });
    }

    /// The project, mutably.
    ///
    /// Mutating the project invalidates the layer table, so the next frame
    /// rebuilds it.
    #[must_use]
    pub fn project_mut(&mut self) -> &mut Project {
        self.graph.invalidate();
        &mut self.project
    }

    /// Mark the project as changed without touching its data.
    ///
    /// Used when the project metadata (name, resolution, frame rate) was
    /// edited directly.
    pub fn invalidate_project(&mut self) {
        self.dirty = true;
        self.sync_graph();
        let revision = self.project.revision;
        self.events
            .push_back(EngineEvent::ProjectChanged { revision });
    }

    /// The render graph.
    #[must_use]
    pub fn graph(&self) -> &RenderGraph {
        &self.graph
    }

    /// The clock.
    #[must_use]
    pub fn clock(&self) -> &MasterClock {
        &self.clock
    }

    /// The clock, mutably.
    #[must_use]
    pub fn clock_mut(&mut self) -> &mut MasterClock {
        &mut self.clock
    }

    /// The device profile.
    #[must_use]
    pub fn profile(&self) -> &DeviceProfile {
        &self.profile
    }

    /// The current quality settings.
    #[must_use]
    pub fn quality(&self) -> QualitySettings {
        self.quality
    }

    /// Playback statistics.
    #[must_use]
    pub fn stats(&self) -> PlaybackStats {
        self.stats
    }

    /// Whether the project has unsaved changes.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark the project as saved.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// Apply a command to the project.
    pub fn execute(&mut self, command: Command) -> VimoResult<()> {
        self.history.execute(&mut self.project, command)?;
        self.dirty = true;
        self.sync_graph();
        let revision = self.project.revision;
        self.events
            .push_back(EngineEvent::ProjectChanged { revision });
        Ok(())
    }

    /// Undo the last command.
    pub fn undo(&mut self) -> VimoResult<bool> {
        let undone = self.history.undo(&mut self.project)?;
        if undone {
            self.dirty = true;
            self.sync_graph();
            let revision = self.project.revision;
            self.events
                .push_back(EngineEvent::ProjectChanged { revision });
        }
        Ok(undone)
    }

    /// Redo the last undone command.
    pub fn redo(&mut self) -> VimoResult<bool> {
        let redone = self.history.redo(&mut self.project)?;
        if redone {
            self.dirty = true;
            self.sync_graph();
            let revision = self.project.revision;
            self.events
                .push_back(EngineEvent::ProjectChanged { revision });
        }
        Ok(redone)
    }

    /// Replace the project (after loading a file).
    pub fn replace_project(&mut self, project: Project) {
        self.project = project;
        self.clock.set_duration(self.project.duration());
        self.sync_graph();
        let revision = self.project.revision;
        self.events
            .push_back(EngineEvent::ProjectChanged { revision });
    }

    /// Push the project into the render graph.
    fn sync_graph(&mut self) {
        let project = self.project.clone();
        *self.graph.project_mut() = project;
        let duration = self.project.duration();
        self.clock.set_duration(duration);
        self.intern_sources();
    }

    /// Intern the source ids of every asset in the project.
    ///
    /// Interning happens on edit, not per frame, so that the per-frame path
    /// only reads from the map.
    fn intern_sources(&mut self) {
        let names: Vec<String> = self
            .project
            .assets
            .iter()
            .map(|a| a.source_id.clone())
            .collect();
        for name in names {
            self.source_id(&name);
        }
    }

    /// Start playback.
    pub fn play(&mut self) {
        self.clock.play();
        self.events
            .push_back(EngineEvent::PlaybackStateChanged(self.clock.state()));
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        self.clock.pause();
        self.events
            .push_back(EngineEvent::PlaybackStateChanged(self.clock.state()));
    }

    /// Stop playback.
    pub fn stop(&mut self) {
        self.clock.stop();
        self.events
            .push_back(EngineEvent::PlaybackStateChanged(self.clock.state()));
    }

    /// Scrub to a position.
    ///
    /// Scrubbing is a seek that also invalidates the prefetch queue and the
    /// frame cache entries around the old position, because the user is
    /// dragging and the old frames are unlikely to be needed again.
    pub fn scrub(&mut self, time: Time) {
        self.prefetcher.clear();
        self.last_render_time = None;
        self.seek(time);
    }

    /// Follow an audio position, correcting the clock's drift.
    ///
    /// The audio device is the master during playback: it reports how far it
    /// has played and the clock follows.
    pub fn sync_to_audio(&mut self, audio_time: Time) {
        let tolerance = self.frame_duration;
        self.clock.sync_to(audio_time, tolerance);
    }

    /// Seek to a position.
    pub fn seek(&mut self, time: Time) {
        self.clock.seek(time);
        self.last_render_time = None;
        self.events.push_back(EngineEvent::PositionChanged(time));
    }

    /// Set the playback rate.
    pub fn set_rate(&mut self, rate: f32) {
        self.clock.set_rate(rate);
    }

    /// Set the preview resolution scale (`1.0` = full resolution).
    pub fn set_preview_scale(&mut self, scale: f32) {
        let scale = scale.clamp(0.0625, 1.0);
        self.quality.preview_scale = scale;
        self.graph.set_preview_scale(scale);
    }

    /// Set the output resolution.
    pub fn set_resolution(&mut self, width: u32, height: u32) {
        self.graph.set_resolution(width, height);
    }

    /// The current preview resolution scale.
    #[must_use]
    pub fn preview_scale(&self) -> f32 {
        self.quality.preview_scale
    }

    /// Render the frame at the current playhead position.
    ///
    /// Returns the plan. The plan is owned by the engine and is valid until
    /// the next call.
    pub fn render_frame(&mut self) -> VimoResult<&FramePlan> {
        let time = self.clock.tick();
        self.events.push_back(EngineEvent::PositionChanged(time));

        // Drop frames when the renderer falls behind: the clock is the master.
        let mut render_time = time;
        if let Some(last) = self.last_render_time {
            let behind = time - last;
            if behind > self.frame_duration * 2 {
                let skipped = ((behind - self.frame_duration) / self.frame_duration).max(1);
                render_time = last + self.frame_duration * skipped;
                self.stats.frames_dropped += skipped as u64;
            } else if behind < self.frame_duration / 2 {
                // The renderer is ahead of the clock: hold the previous frame.
                self.stats.frames_held += 1;
            }
        }

        let reused = self.last_render_time == Some(render_time) && self.last_render_time.is_some();
        self.last_render_time = Some(render_time);
        self.graph
            .set_resolution(self.project.meta.width, self.project.meta.height);
        self.graph.set_preview_scale(self.quality.preview_scale);
        self.graph.set_preview_mode(true);
        self.graph.frame_at(render_time)?;
        resolve_textures(&mut self.resolver, self.graph.plan_mut());
        self.stats.frames_rendered += 1;
        self.events.push_back(EngineEvent::FrameRendered {
            time: render_time,
            reused,
        });
        Ok(self.graph.plan())
    }

    /// Update the prefetch queue for the current playhead.
    pub fn update_prefetch(&mut self) {
        let playhead = self.clock.position();
        let sources = self.active_sources(playhead);
        self.prefetcher.update(playhead, &sources);
    }

    /// The sources visible at `time`.
    fn active_sources(&self, time: Time) -> Vec<ActiveSource> {
        let mut out = Vec::new();
        for (track_index, track) in self.project.tracks.iter().enumerate() {
            if !track.alive || !track.enabled {
                continue;
            }
            let Some(clip_id) = self.project.active_clip(track_index as u32, time) else {
                continue;
            };
            let Some(clip) = self.project.clips.get(clip_id as usize) else {
                continue;
            };
            let Some(asset) = self.project.assets.get(clip.asset as usize) else {
                continue;
            };
            let Some(source_id) = self.lookup_source_id(&asset.source_id) else {
                continue;
            };
            out.push(ActiveSource {
                source: source_id,
                visible: clip.timeline,
                timebase: asset.timebase,
                rate: clip.speed,
            });
        }
        out
    }

    /// The interned id of a media source.
    ///
    /// Source ids are interned so that the prefetcher and the frame cache can
    /// key on a `u64` without allocating.
    pub fn source_id(&mut self, source: &str) -> u64 {
        if let Some(id) = self.source_ids.get(source) {
            return *id;
        }
        let id = self.next_source_id;
        self.next_source_id += 1;
        self.source_ids.insert(source.to_string(), id);
        id
    }

    /// Look up the interned id of a media source without creating one.
    #[must_use]
    pub fn lookup_source_id(&self, source: &str) -> Option<u64> {
        self.source_ids.get(source).copied()
    }

    /// The prefetcher.
    #[must_use]
    pub fn prefetcher(&self) -> &Prefetcher {
        &self.prefetcher
    }

    /// The frame cache.
    #[must_use]
    pub fn cache(&self) -> &FrameCache {
        &self.cache
    }

    /// Look up a cached frame.
    pub fn cached_frame(&mut self, key: FrameKey) -> bool {
        self.cache.get(key).is_some()
    }

    /// Insert a frame into the cache.
    pub fn cache_frame(&mut self, key: FrameKey, bytes: usize, handle: u64, pts: Time) {
        self.cache.insert(key, bytes, handle, pts);
        self.memory.allocate(bytes);
    }

    /// Report a thermal reading.
    pub fn report_thermal(&mut self, reading: ThermalReading) {
        let before = self.thermal.effective_state();
        let after = self.thermal.update(reading);
        if after != before {
            self.quality = self
                .thermal
                .apply(self.profile.quality(), self.profile.tier);
            self.prefetcher.prefetch_frames = self.quality.prefetch_frames;
            self.events.push_back(EngineEvent::Thermal(after));
        }
    }

    /// Report the current memory pressure.
    pub fn refresh_memory_pressure(&mut self) {
        let level = self.memory.pressure();
        if level != PressureLevel::Normal {
            self.trim_caches();
        }
    }

    /// Trim the caches in response to memory pressure.
    pub fn trim_caches(&mut self) {
        let target = (self.cache.cap_bytes() / 2).min(self.memory.headroom());
        let removed = self.cache.trim_to(target);
        for _ in 0..removed {
            self.memory.release(0);
        }
        self.events
            .push_back(EngineEvent::MemoryPressure(self.memory.pressure()));
    }

    /// The memory tracker.
    #[must_use]
    pub fn memory(&self) -> &MemoryTracker {
        &self.memory
    }

    /// Start an export.
    ///
    /// The engine owns the *timing*: it decides which frame comes next and
    /// builds the plan for it. The platform owns the encoding: it draws the
    /// plan with its own renderer and feeds the pixels to its encoder. That
    /// split keeps the codec (which differs per device) in the platform layer
    /// while the core keeps the frame numbering deterministic.
    pub fn start_export(&mut self, preset: ExportPreset) -> VimoResult<()> {
        let duration = self.project.duration();
        let mut pipeline = ExportPipeline::new(preset, duration)?;
        pipeline.begin()?;
        self.export = Some(pipeline);
        Ok(())
    }

    /// The time of the next frame the export needs, if any.
    ///
    /// The engine builds the plan for that frame into the shared render plan,
    /// so the platform can draw it.
    pub fn export_next_frame(&mut self) -> VimoResult<Option<Time>> {
        let Some(pipeline) = &self.export else {
            return Ok(None);
        };
        if pipeline.state() != ExportState::Running {
            return Ok(None);
        }
        let frame = pipeline.frame();
        let time = pipeline.time_of_frame(frame);
        {
            let mut guard = match self.render_plan.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            self.graph
                .set_resolution(pipeline.preset().width, pipeline.preset().height);
            self.graph.set_preview_scale(1.0);
            self.graph.set_preview_mode(false);
            self.graph.frame_at_into(time, &mut guard)?;
            resolve_textures(&mut self.resolver, &mut guard);
        }
        Ok(Some(time))
    }

    /// Mark the frame the export just produced as done.
    pub fn export_frame_done(&mut self) -> VimoResult<ExportProgress> {
        let Some(pipeline) = &mut self.export else {
            return Err(vimo_foundation::error::invalid_state("export: not running"));
        };
        pipeline.complete_frame();
        let progress = pipeline.progress();
        if progress.state == ExportState::Completed {
            self.events
                .push_back(EngineEvent::ExportFinished(ExportState::Completed));
        }
        Ok(progress)
    }

    /// Pause the export.
    pub fn pause_export(&mut self) {
        if let Some(pipeline) = &mut self.export {
            pipeline.pause();
        }
    }

    /// Resume a paused export.
    pub fn resume_export(&mut self) {
        if let Some(pipeline) = &mut self.export {
            pipeline.resume_paused();
        }
    }

    /// The export pipeline.
    #[must_use]
    pub fn export(&self) -> Option<&ExportPipeline> {
        self.export.as_ref()
    }

    /// Encode the next export frame.
    pub fn export_step(&mut self, frame: &vimo_media::VideoFrame) -> VimoResult<ExportProgress> {
        let Some(mut pipeline) = self.export.take() else {
            return Err(invalid_state("export: not running"));
        };
        let result = pipeline.encode_next(&mut self.graph, frame);
        let progress = result
            .as_ref()
            .map(|p| *p)
            .unwrap_or_else(|_| pipeline.progress());
        if progress.state == ExportState::Completed {
            self.events
                .push_back(EngineEvent::ExportFinished(ExportState::Completed));
        }
        self.export = Some(pipeline);
        result
    }

    /// Cancel the export.
    pub fn cancel_export(&mut self) {
        if let Some(pipeline) = &mut self.export {
            pipeline.cancel();
            let state = pipeline.state();
            self.events.push_back(EngineEvent::ExportFinished(state));
        }
        self.export = None;
    }

    /// Take the pending events.
    pub fn poll_events(&mut self) -> Vec<EngineEvent> {
        self.events.drain(..).collect()
    }

    /// Push an event.
    pub fn push_event(&mut self, event: EngineEvent) {
        self.events.push_back(event);
    }

    /// The effect registry.
    #[must_use]
    pub fn registry(&self) -> &EffectRegistry {
        self.shaders.registry()
    }

    /// The shader library (the registry plus the GLSL sources).
    #[must_use]
    pub fn shaders(&self) -> &vimo_gl::ShaderLibrary {
        &self.shaders
    }

    /// The shader library, mutably.
    ///
    /// The platform loads the manifest and the shader sources here at start-up.
    #[must_use]
    pub fn shaders_mut(&mut self) -> &mut vimo_gl::ShaderLibrary {
        &mut self.shaders
    }

    /// Register an effect type that the manifest declared.
    pub fn add_effect_type(
        &mut self,
        desc: vimo_manifest::EffectDesc,
    ) -> VimoResult<vimo_manifest::EffectTypeId> {
        let id = self.shaders.add_effect(desc)?;
        self.sync_registry();
        Ok(id)
    }

    /// Load the effect manifest and the shader sources.
    ///
    /// The manifest says which effects exist; the shaders implement them.
    /// Both are checked against each other, so a mismatch is found when the
    /// project is opened rather than when the user picks the effect.
    ///
    /// Returns the number of interface mismatches.
    pub fn load_effects(
        &mut self,
        manifest: &str,
        sources: &std::collections::HashMap<String, String>,
    ) -> VimoResult<usize> {
        self.shaders.load_manifest(manifest)?;
        for (path, source) in sources {
            self.shaders.add_source(path, source);
        }
        // The graph needs to know about the new effects.
        self.sync_registry();
        Ok(self.shaders.validate().len())
    }

    /// Give the render graph the current effect registry.
    fn sync_registry(&mut self) {
        self.graph.set_registry(self.shaders.registry().clone());
    }

    /// Check the loaded shaders against the manifest.
    ///
    /// Returns the mismatches. An empty result means the library is
    /// consistent, which is what the loader checks before it hands the
    /// project to the user.
    pub fn validate_shaders(&self) -> Vec<vimo_render::shader::UniformMismatch> {
        self.shaders.validate()
    }

    /// The number of frames the output has per second.
    #[must_use]
    pub fn frame_duration(&self) -> Time {
        self.frame_duration
    }

    // --- files ------------------------------------------------------------

    /// The file services.
    #[must_use]
    pub fn files(&self) -> &SessionFiles {
        &self.files
    }

    /// The file services, mutably.
    #[must_use]
    pub fn files_mut(&mut self) -> &mut SessionFiles {
        &mut self.files
    }

    /// A JSON snapshot of the project, for the UI.
    #[must_use]
    pub fn snapshot_json(&self) -> String {
        crate::files::project_snapshot(&self.project)
    }

    /// Load a project from a file, replacing the current one.
    pub fn load_project(&mut self, path: &std::path::Path) -> VimoResult<()> {
        let (project, report) = SessionFiles::load(path, self.shaders.registry())?;
        if !report.dropped_effects.is_empty() {
            self.events.push_back(EngineEvent::Error {
                code: vimo_foundation::ErrorKind::Unsupported.code(),
                message: format!(
                    "the project uses {} effect(s) that this build does not have",
                    report.dropped_effects.len()
                ),
            });
        }
        self.replace_project(project);
        Ok(())
    }

    /// Save the project to a file.
    pub fn save_project(&mut self, path: &std::path::Path) -> VimoResult<()> {
        self.files
            .save(&self.project, self.shaders.registry(), path)?;
        self.mark_saved();
        Ok(())
    }

    /// Write an autosave snapshot.
    pub fn autosave(&mut self) -> VimoResult<std::path::PathBuf> {
        self.files.autosave(&self.project, self.shaders.registry())
    }

    /// Recover the newest autosave, if there is one.
    pub fn recover_autosave(&mut self) -> VimoResult<bool> {
        let Some(project) = self.files.recover(self.shaders.registry())? else {
            return Ok(false);
        };
        self.replace_project(project);
        Ok(true)
    }

    // --- rendering --------------------------------------------------------

    /// Attach the resolver that turns pictures into GPU textures.
    pub fn set_texture_resolver(&mut self, resolver: Box<dyn TextureResolver>) {
        self.resolver = Some(resolver);
    }

    /// Attach the render thread that draws the frames.
    pub fn attach_render_thread(&mut self, thread: RenderThread) {
        self.render_thread = Some(thread);
    }

    /// The render thread, if one is attached.
    #[must_use]
    pub fn render_thread(&self) -> Option<&RenderThread> {
        self.render_thread.as_ref()
    }

    /// Build the frame at the playhead and hand it to the render thread.
    ///
    /// This is what the app calls from its render loop: no GL calls cross the
    /// FFI boundary, and the plan is built on the calling thread while the
    /// previous frame is still being drawn.
    pub fn render_to_surface(&mut self) -> VimoResult<()> {
        let time = self.clock.tick();
        self.events.push_back(EngineEvent::PositionChanged(time));
        {
            let mut guard = match self.render_plan.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            self.graph.frame_at_into(time, &mut guard)?;
            resolve_textures(&mut self.resolver, &mut guard);
        }
        if let Some(thread) = &self.render_thread {
            thread.render(time, self.render_plan.clone())?;
        }
        self.stats.frames_rendered += 1;
        Ok(())
    }
}

/// Fill the texture handles of a plan.
fn resolve_textures(resolver: &mut Option<Box<dyn TextureResolver>>, plan: &mut FramePlan) {
    let Some(resolver) = resolver.as_deref_mut() else {
        return;
    };
    for step in &mut plan.steps {
        match step {
            vimo_render::DrawStep::Composite { source, .. } => {
                source.texture = resolver.resolve(source);
            }
            vimo_render::DrawStep::Transition {
                from_source,
                to_source,
                ..
            } => {
                from_source.texture = resolver.resolve(from_source);
                to_source.texture = resolver.resolve(to_source);
            }
            vimo_render::DrawStep::Clear { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use vimo_foundation::time::TimeRange;
    use vimo_project::model::{Asset, Clip, Track, TrackKind};

    fn engine() -> Engine {
        let mut project = Project::new();
        project.assets.push(Asset::video(
            "src",
            vimo_foundation::time::Time::from_secs(30),
        ));
        project.tracks.push(Track::new("V1", TrackKind::Video));
        project.clips.push(Clip::new(
            0,
            TimeRange::new(
                vimo_foundation::time::Time::ZERO,
                vimo_foundation::time::Time::from_secs(5),
            ),
            vimo_foundation::time::Time::ZERO,
        ));
        project.tracks[0].clips.push(0);
        let profile = DeviceProfile::new(6 << 30, 4096, vimo_media::modern_phone());
        Engine::open(
            project,
            EffectRegistry::new(),
            profile,
            Arc::new(crate::clock::ManualClock::new()),
        )
    }

    #[test]
    fn editing_emits_an_event() {
        let mut e = engine();
        e.events.clear();
        let cmd = Command::add_clip(
            0,
            1,
            Clip::new(
                0,
                TimeRange::new(
                    vimo_foundation::time::Time::from_secs(5),
                    vimo_foundation::time::Time::from_secs(8),
                ),
                vimo_foundation::time::Time::ZERO,
            ),
        );
        e.execute(cmd).unwrap();
        let events = e.poll_events();
        assert!(events
            .iter()
            .any(|ev| matches!(ev, EngineEvent::ProjectChanged { .. })));
        assert!(e.is_dirty());
    }

    #[test]
    fn rendering_produces_a_plan() {
        let mut e = engine();
        let plan = e.render_frame().unwrap();
        assert!(!plan.steps.is_empty());
        assert_eq!(e.stats().frames_rendered, 1);
    }

    #[test]
    fn thermal_pressure_lowers_the_preview_scale() {
        let mut e = engine();
        let before = e.quality().preview_scale;
        for _ in 0..3 {
            e.report_thermal(ThermalReading {
                state: ThermalState::Severe,
                temperature_c: Some(46.0),
            });
        }
        assert!(e.quality().preview_scale < before);
    }

    #[test]
    fn source_ids_are_interned_and_distinct() {
        let mut e = engine();
        let a = e.source_id("clip-a");
        let b = e.source_id("clip-b");
        assert_ne!(a, b, "different sources must not collide");
        assert_eq!(e.source_id("clip-a"), a, "interning must be stable");
        assert_eq!(e.lookup_source_id("clip-a"), Some(a));
        assert_eq!(e.lookup_source_id("missing"), None);
    }

    #[test]
    fn scrubbing_clears_the_prefetch_queue() {
        let mut e = engine();
        e.update_prefetch();
        assert!(e.prefetcher().pending() > 0);
        e.scrub(vimo_foundation::time::Time::from_secs(2));
        assert_eq!(e.prefetcher().pending(), 0);
        assert_eq!(
            e.clock().position(),
            vimo_foundation::time::Time::from_secs(2)
        );
    }

    #[test]
    fn the_audio_clock_follows_the_device() {
        let mut e = engine();
        e.clock_mut().play();
        e.sync_to_audio(vimo_foundation::time::Time::from_secs(3));
        let position = e.clock().position();
        assert!(
            (position - vimo_foundation::time::Time::from_secs(3)).abs()
                <= vimo_foundation::time::Time::from_millis(50),
            "position={position:?}"
        );
    }
}
