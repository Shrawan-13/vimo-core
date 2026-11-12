//! Golden-frame tests.
//!
//! These render a scene with the CPU reference rasteriser and compare the
//! result against a stored image. If the golden file does not exist it is
//! created, so adding a new case is a two-step process (run once to generate,
//! run again to verify).
//!
//! The golden files are binary PPMs: dependency free, and a byte-level diff
//! shows exactly what changed.

use std::path::{Path, PathBuf};

use vimo_foundation::color::Rgba8;
use vimo_foundation::math::{Rect, Vec2};
use vimo_foundation::time::{Time, TimeRange};
use vimo_manifest::EffectRegistry;
use vimo_project::model::{Asset, Clip, Transform, Track, TrackKind};
use vimo_project::Project;
use vimo_render::software::{diff, execute, read_ppm, write_ppm, FrameBuffer, FrameProvider, FrameView};
use vimo_render::{FramePlan, RenderGraph, TextureRef};

/// A provider that returns a solid colour per clip.
struct SolidProvider {
    colors: Vec<(u32, Rgba8)>,
    /// One pixel per clip, so the view can borrow from the provider.
    pixels: Vec<Rgba8>,
}

impl SolidProvider {
    fn new(colors: Vec<(u32, Rgba8)>) -> Self {
        let pixels = colors.iter().map(|(_, c)| *c).collect();
        SolidProvider { colors, pixels }
    }
}

impl FrameProvider for SolidProvider {
    fn frame_for(&self, request: &TextureRef) -> Option<FrameView<'_>> {
        let index = self.colors.iter().position(|(c, _)| *c == request.clip)?;
        Some(FrameView { width: 1, height: 1, pixels: &self.pixels[index..index + 1] })
    }
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden")
}

/// Compare a rendered frame against its golden file.
fn check_golden(name: &str, frame: &FrameBuffer) {
    let path = golden_dir().join(format!("{name}.ppm"));
    if !path.exists() {
        write_ppm(frame, &path).expect("write golden");
        eprintln!("created golden image {path:?}");
        return;
    }
    let expected = read_ppm(&path).expect("read golden");
    let stats = diff(&expected, frame);
    assert_eq!(
        stats.differing_pixels, 0,
        "{name}: {} pixels differ (max channel delta {}). If this change is \
         intended, delete {} and re-run the test.",
        stats.differing_pixels, stats.max_channel_delta, path.display()
    );
}

/// Build a project with two stacked video tracks.
fn two_clip_project() -> Project {
    let mut p = Project::new();
    p.meta.width = 64;
    p.meta.height = 64;
    p.meta.background = Rgba8::rgb(0, 0, 0);
    p.assets.push(Asset::video("a", Time::from_secs(30)));
    p.assets.push(Asset::video("b", Time::from_secs(30)));

    let mut v1 = Track::new("V1", TrackKind::Video);
    v1.clips.push(0);
    p.tracks.push(v1);

    let mut v2 = Track::new("V2", TrackKind::Video);
    v2.clips.push(1);
    p.tracks.push(v2);

    p.clips.push(Clip::new(
        0,
        TimeRange::new(Time::ZERO, Time::from_secs(10)),
        Time::ZERO,
    ));
    let mut top = Clip::new(
        1,
        TimeRange::new(Time::ZERO, Time::from_secs(10)),
        Time::ZERO,
    );
    top.opacity = 0.5;
    p.clips.push(top);
    p
}

fn render(graph: &mut RenderGraph, time: Time, provider: &SolidProvider) -> FrameBuffer {
    let plan: &FramePlan = graph.frame_at(time).expect("frame");
    let mut fb = FrameBuffer::new(plan.width, plan.height);
    execute(plan, &mut fb, provider);
    fb
}

#[test]
fn golden_opaque_over_opaque() {
    let mut graph = RenderGraph::new(two_clip_project(), EffectRegistry::new());
    graph.set_resolution(64, 64);
    let provider = SolidProvider::new(vec![
        (0, Rgba8::new(200, 40, 40, 255)),
        (1, Rgba8::new(40, 40, 200, 255)),
    ]);
    let frame = render(&mut graph, Time::from_secs(1), &provider);
    check_golden("opaque_over_opaque", &frame);
}

#[test]
fn golden_alpha_over_opaque() {
    let mut graph = RenderGraph::new(two_clip_project(), EffectRegistry::new());
    graph.set_resolution(64, 64);
    let provider = SolidProvider::new(vec![
        (0, Rgba8::new(0, 0, 0, 255)),
        (1, Rgba8::new(255, 255, 255, 255)),
    ]);
    let frame = render(&mut graph, Time::from_secs(1), &provider);
    check_golden("alpha_over_opaque", &frame);
}

#[test]
fn golden_transformed_clip() {
    let mut project = two_clip_project();
    // Scale the top clip down and push it to the corner.
    project.clips[1].transform = Transform {
        position: Vec2::new(-0.25, -0.25),
        scale: Vec2::new(0.5, 0.5),
        rotation: 0.0,
        anchor: Vec2::new(0.5, 0.5),
        crop: Rect::FULL,
    };
    let mut graph = RenderGraph::new(project, EffectRegistry::new());
    graph.set_resolution(64, 64);
    let provider = SolidProvider::new(vec![
        (0, Rgba8::new(0, 0, 0, 255)),
        (1, Rgba8::new(255, 128, 0, 255)),
    ]);
    let frame = render(&mut graph, Time::from_secs(1), &provider);
    check_golden("transformed_clip", &frame);
}

#[test]
fn golden_clear_only() {
    let mut project = Project::new();
    project.meta.width = 32;
    project.meta.height = 32;
    project.meta.background = Rgba8::rgb(12, 34, 56);
    let mut graph = RenderGraph::new(project, EffectRegistry::new());
    graph.set_resolution(32, 32);
    let frame = render(&mut graph, Time::ZERO, &SolidProvider::new(vec![]));
    check_golden("clear_only", &frame);
}
