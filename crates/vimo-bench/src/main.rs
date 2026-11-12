//! A tiny benchmark harness with **enforced budgets**.
//!
//! Benchmarks that nobody checks are documentation. Every benchmark here has a
//! budget derived from the product requirements, and the run fails if a
//! budget is exceeded:
//!
//! | Operation | Budget | Why |
//! |-----------|--------|-----|
//! | `frame_at` (10 clips) | 100 µs | the preview must fit in a 16 ms frame with room for GL |
//! | `frame_at` (200 clips) | 1 ms | a busy timeline must still be interactive |
//! | project save | 50 ms | saving must not stall the UI |
//! | project load | 100 ms | opening a project must feel instant |
//! | undo | 50 µs | undo must feel instant |
//!
//! Run with `cargo run --release -p vimo-bench`.

use std::time::{Duration, Instant};

use vimo_foundation::time::{Time, TimeRange};
use vimo_manifest::EffectRegistry;
use vimo_project::model::{Asset, Clip, EffectNode, EffectOwner, Track, TrackKind};
use vimo_project::{codec, Command, History, Project};
use vimo_render::RenderGraph;

/// The result of one benchmark.
struct Bench {
    name: &'static str,
    iterations: u32,
    total: Duration,
    budget: Duration,
}

impl Bench {
    fn per_iteration(&self) -> Duration {
        self.total / self.iterations.max(1)
    }

    fn report(&self) -> bool {
        let per = self.per_iteration();
        let ok = per <= self.budget;
        let status = if ok { "ok  " } else { "FAIL" };
        println!(
            "{status} {:<34} {:>10.3} µs/iter  (budget {:>8.3} µs, {} iters)",
            self.name,
            per.as_secs_f64() * 1e6,
            self.budget.as_secs_f64() * 1e6,
            self.iterations
        );
        ok
    }
}

/// Run `f` repeatedly and record the timing.
fn bench<F: FnMut()>(name: &'static str, iterations: u32, budget: Duration, mut f: F) -> Bench {
    // Warm up so that the first-call costs (page faults, lazy init) do not
    // dominate the measurement.
    for _ in 0..(iterations / 10).max(1) {
        f();
    }
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let total = start.elapsed();
    Bench {
        name,
        iterations,
        total,
        budget,
    }
}

/// Build a project with `tracks` tracks and `clips_per_track` clips each.
fn build_project(tracks: usize, clips_per_track: usize) -> Project {
    let mut p = Project::new();
    p.meta.width = 1920;
    p.meta.height = 1080;
    p.assets.push(Asset::video("src-0", Time::from_secs(600)));
    for _ in 0..tracks {
        p.tracks.push(Track::new("V", TrackKind::Video));
    }
    for t in 0..tracks {
        for c in 0..clips_per_track {
            let start = Time::from_secs((c * 2) as i64);
            let end = Time::from_secs((c * 2 + 2) as i64);
            let id = p.clips.len() as u32;
            p.clips
                .push(Clip::new(0, TimeRange::new(start, end), Time::ZERO));
            p.tracks[t].clips.push(id);
        }
    }
    p
}

fn main() {
    println!("vimo-core benchmarks (release)\n");
    let mut results = Vec::new();

    // --- render graph -----------------------------------------------------
    let registry = EffectRegistry::new();
    let small = build_project(4, 4);
    let mut graph = RenderGraph::new(small, registry.clone());
    let mut t = Time::ZERO;
    results.push(bench(
        "frame_at (16 clips)",
        2_000,
        Duration::from_micros(100),
        || {
            graph.frame_at(t).expect("frame");
            t += Time::from_millis(1);
            if t.as_secs() > 5 {
                t = Time::ZERO;
            }
        },
    ));

    let large = build_project(20, 10);
    let mut graph = RenderGraph::new(large, registry.clone());
    let mut t = Time::ZERO;
    results.push(bench(
        "frame_at (200 clips)",
        500,
        Duration::from_millis(1),
        || {
            graph.frame_at(t).expect("frame");
            t += Time::from_millis(1);
            if t.as_secs() > 5 {
                t = Time::ZERO;
            }
        },
    ));

    // --- project serialisation -------------------------------------------
    let project = build_project(8, 8);
    let bytes = codec::write(&project, &registry, codec::CURRENT_VERSION).expect("write");
    println!("     project file size: {} bytes", bytes.len());
    results.push(bench(
        "project save (64 clips)",
        200,
        Duration::from_millis(50),
        || {
            let _ = codec::write(&project, &registry, codec::CURRENT_VERSION).expect("write");
        },
    ));
    results.push(bench(
        "project load (64 clips)",
        200,
        Duration::from_millis(100),
        || {
            let (p, _) = codec::read(&bytes, &registry).expect("read");
            std::hint::black_box(p);
        },
    ));

    // --- undo / redo ------------------------------------------------------
    let mut project = build_project(2, 4);
    let before = project.clips[0].clone();
    results.push(bench(
        "undo (clip edit)",
        20_000,
        Duration::from_micros(50),
        || {
            let cmd = Command::edit_clip(0, before.clone(), |c| c.opacity = 0.5);
            let inverse = cmd.apply(&mut project).expect("apply");
            inverse.apply(&mut project).expect("apply");
        },
    ));

    // --- manifest parsing -------------------------------------------------
    let manifest_json = r#"{"effects":[
        {"id":"com.vimo.effect.blur","params":[{"name":"radius","type":"float","default":4.0}]},
        {"id":"com.vimo.effect.gain","params":[{"name":"gain","type":"float","default":1.0}]}
    ]}"#;
    results.push(bench(
        "manifest parse",
        20_000,
        Duration::from_micros(200),
        || {
            let m = vimo_manifest::Manifest::parse_str(manifest_json).expect("parse");
            std::hint::black_box(m);
        },
    ));

    // --- frame cache ------------------------------------------------------
    let mut cache = vimo_resources::FrameCache::new(64 * 1024 * 1024);
    for i in 0..1000u64 {
        cache.insert(
            vimo_resources::FrameKey::new(1, i as i64),
            1024 * 1024,
            i,
            Time::ZERO,
        );
    }
    results.push(bench(
        "frame cache lookup",
        1_000_000,
        Duration::from_nanos(500),
        || {
            let key = vimo_resources::FrameKey::new(1, 500);
            std::hint::black_box(cache.get(key));
        },
    ));

    // --- audio mixing -----------------------------------------------------
    let mixer = vimo_audio::Mixer::new(2);
    let source = vimo_audio::MixSource::new(vec![0.25; 4096], 2, Time::ZERO);
    let mut out = vec![0.0f32; 4096];
    results.push(bench(
        "mix 4096 frames",
        20_000,
        Duration::from_micros(200),
        || {
            out.fill(0.0);
            mixer.mix(std::slice::from_ref(&source), Time::ZERO, &mut out);
        },
    ));

    // --- resampling -------------------------------------------------------
    let mut resampler =
        vimo_audio::Resampler::new(44100, 48000, vimo_audio::ResamplerQuality::Cubic);
    let input: Vec<f32> = (0..1024).map(|i| (i as f32 / 1024.0).sin()).collect();
    let mut sink = Vec::with_capacity(4096);
    results.push(bench(
        "resample 1024 frames",
        20_000,
        Duration::from_micros(200),
        || {
            sink.clear();
            resampler.process(&input, 1, &mut sink);
        },
    ));

    // --- effect parameter evaluation -------------------------------------
    let mut project = build_project(1, 2);
    let desc = vimo_manifest::EffectDesc::new(
        "com.vimo.effect.blur",
        "Blur",
        "blur",
        1,
        None,
        vec![vimo_manifest::ParamDesc {
            name: "radius".into(),
            ptype: vimo_manifest::ParamType::Float,
            default: vimo_manifest::ParamValue::Float(4.0),
            min: Some(0.0),
            max: Some(32.0),
            step: None,
            label: "Radius".into(),
            animatable: true,
            options: Vec::new(),
        }],
        Default::default(),
    )
    .expect("desc");
    let mut reg = EffectRegistry::new();
    let type_id = reg.register(desc).expect("register");
    let node = EffectNode::new(type_id, reg.get(type_id).expect("get"));
    let mut history = History::new();
    let owner = EffectOwner::Clip(0);
    history
        .execute(
            &mut project,
            Command::AddEffect {
                owner,
                effect: node,
            },
        )
        .expect("effect");

    let mut graph = RenderGraph::new(project, reg);
    results.push(bench(
        "frame_at (1 effect)",
        20_000,
        Duration::from_micros(100),
        || {
            std::hint::black_box(graph.frame_at(Time::from_secs(1)).expect("frame"));
        },
    ));

    println!();
    let mut failures = 0;
    for r in &results {
        if !r.report() {
            failures += 1;
        }
    }
    println!();
    if failures == 0 {
        println!("all {} benchmarks within budget", results.len());
    } else {
        println!(
            "{} of {} benchmarks exceeded their budget",
            failures,
            results.len()
        );
        std::process::exit(1);
    }
}
