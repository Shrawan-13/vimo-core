# Vimo Core

The Rust core of the Vimo Android video editor.

The goal of this crate set is a **fast, predictable, testable** editing engine
that stays smooth on low-end hardware: no allocations in the hot path, no
panics, no unbounded memory growth, and the same rendering code for preview and
export.

---

## Crate layout

The dependency direction is strictly downwards. Each crate is independently
testable and has no knowledge of the layer above it.

```
                  ┌──────────────────────────────┐
   Android / JNI  │  vimo-ffi   (C ABI)          │  commands in, events out
                  └───────────────┬──────────────┘
                                  │
                  ┌───────────────▼──────────────┐
                  │  vimo-engine                  │  session, clock, prefetch,
                  │  (orchestration)               │  export, render thread
                  └───┬────────┬────────┬────────┘
                      │        │        │
        ┌─────────────▼──┐ ┌───▼─────┐ ┌▼──────────────┐
        │ vimo-render    │ │vimo-    │ │vimo-resources │
        │ frame plans,   │ │project  │ │ pools, caches, │
        │ layers, GLSL   │ │model,   │ │ memory        │
        │ uniforms       │ │history, │ │               │
        └────┬───────────┘ │ file    │ └───────┬───────┘
             │             │ format  │         │
             │        ┌────▼─────────▼──┐      │
             │        │ vimo-manifest    │      │
             │        │ JSON effect defs │      │
             │        │ typed parameters │      │
             │        └────────┬─────────┘      │
        ┌────▼─────────────────▼──┐ ┌──────────▼──┐
        │ vimo-gl                  │ │vimo-device │
        │ the OpenGL ES renderer   │ │tiers,      │
        │ behind the `GlApi` trait │ │thermal     │
        └────────────┬─────────────┘ └─────────────┘
                     │
              ┌──────▼─────┐ ┌────▼──────┐ ┌▼───────────┐
              │ vimo-media │ │vimo-audio │ │vimo-      │
              │ decoders,  │ │mixing,    │ │foundation │
              │ encoders,  │ │fades, SRC │ │time, ids, │
              │ VFR, proxy │ │waveforms  │ │errors     │
              └────────────┘ └───────────┘ └────────────┘
```

| Crate | Responsibility |
|-------|----------------|
| `vimo-foundation` | Zero-dependency primitives: exact time, ids, errors, math, containers, tracing. `unsafe` is forbidden. |
| `vimo-manifest` | Parsing and validating effect manifests (JSON): typed parameters, defaults, interning. |
| `vimo-project` | The project model, the command system, undo/redo, the versioned file format and migrations, autosave, the JSON snapshot for the UI. |
| `vimo-render` | The render graph: layer tables, frame plans, effect uniforms, dirty tracking, a CPU reference rasteriser, GLSL introspection. |
| `vimo-gl` | The portable OpenGL ES renderer behind the `GlApi` trait, plus a mock that records every call. |
| `vimo-media` | Decoder/encoder traits, capability detection, frame buffers, VFR tables, rotation metadata, proxy scheduling, the zero-copy surface contract. |
| `vimo-audio` | Mixing, gain envelopes, fades, sample-rate conversion, waveform caching, the low-latency output trait. |
| `vimo-resources` | Decoder/texture/buffer pools, the LRU frame cache, memory accounting and pressure handling. |
| `vimo-device` | Capability tiers, the quality ladder, thermal monitoring with hysteresis. |
| `vimo-engine` | The engine: session, master clock, playback, prefetching, export, the dedicated render thread. |
| `vimo-ffi` | The C ABI (`libvimo_core.so`) plus `include/vimo_core.h`. |
| `vimo-android` | The Android platform layer: EGL + OpenGL ES 3, MediaCodec, Oboe, the device profile, the Perfetto sink and the zero-copy surface path. |
| `vimo-jni` | The JNI bindings between Kotlin and the core. |
| `vimo-bench` | Benchmarks with **enforced performance budgets**. |

The `assets/` directory holds the **effect library**: the manifest that says
which effects exist and the GLSL that implements them. The app packages them
as APK assets and hands them to the core at start-up, where the manifest and
the shaders are checked against each other.

---

## Design rules

These are the invariants the code is written around.

### 1. Exact time, never floating point on the timeline

Every timeline position is a signed count of nanoseconds
(`vimo_foundation::time::Time`). Frame rates are rationals (`30000/1001` is
never rounded to `29.97`). A three-hour 120 fps project does not drift by a
single microsecond. Floating point only appears where the answer is
inherently fractional (e.g. "where is 12.5 frames?").

Frame boundaries are rounded **up** to the next nanosecond, which guarantees
that `time_to_frame(frame_to_time(n)) == n`.

### 2. Ids are stable, entities are never renumbered

Entities live in flat vectors and are addressed by index. Deleting an entity
flips an `alive` flag instead of removing it, so every cross-reference
(clip → effect, track → clip, transition → clip) stays valid forever and no
index fix-ups are needed. Undo simply flips the flag back.

### 3. Commands are the only way to mutate a project

`Command::apply(&mut project)` returns the **inverse** of what it did. The
undo stack stores inverses; the redo stack stores the commands that undo
them. A command therefore cannot drift out of sync with its own inverse.

### 4. The render graph produces data, not commands

`frame_at(t)` fills a `FramePlan` — a flat list of draw steps. The GPU
renderer and the CPU reference rasteriser consume the same structure, which
is what makes preview and export impossible to diverge.

### 5. Three levels of dirty tracking

| Level | Detects | Cost |
|-------|---------|------|
| Structure fingerprint | tracks/clips/effect lists changed | one hash per frame |
| Clip revision | a clip's transform/opacity/blend changed | one compare per layer |
| Effect revision | an effect's parameters changed | one compare per effect |

Only the *active* layers are examined, so the per-frame cost is proportional
to what is on screen, not to the size of the project.

### 6. Nothing allocates in the hot path

The plan, the layer table, the uniform blocks and the frame cache are all
reused. Buffers come from pools. The benchmark suite measures this; the
budgets fail the build if it regresses.

### 7. The core never panics

Every fallible operation returns `VimoError`. The FFI boundary wraps every
entry point in `catch_unwind`, so even a bug cannot take the app down.

### 8. Bounded memory

Every cache is bounded **by bytes**, not by entry count. The memory tracker
knows what the app is using, and thermal/memory pressure trims the caches
before the system starts killing the process.

---

## Where each requirement lives

### [1] Project model
| Requirement | Where |
|-------------|-------|
| Clips, tracks, transitions, effect nodes | `vimo-project/src/model.rs` |
| Keyframes + interpolation (linear, bezier, hold) | `vimo-project/src/anim.rs` |
| Undo/redo (command history) | `vimo-project/src/command.rs`, `history.rs` |
| Save/load (versioned format, migrations) | `vimo-project/src/codec/`, `migrate.rs` |
| Autosave + crash recovery | `vimo-project/src/autosave.rs` |
| Effect manifest parser (JSON) | `vimo-manifest/src/parse.rs` |

The file format is a versioned, sectioned binary container. Sections that a
reader does not understand are skipped, so new sections never break old
builds. Migrations run forward one version at a time (`v1 → v2 → v3`).

### [2] Render graph
`vimo-render/src/graph.rs` builds the plan; `plan.rs` defines it;
`uniforms.rs` maps typed parameters onto GLSL uniforms; `shader.rs` parses the
GLSL source and checks that the shader's uniforms match the manifest's
parameters (at load time, not at draw time); `software.rs` is the CPU
reference rasteriser used by the golden tests.

`vimo-gl` turns a plan into GL calls. It talks to the `GlApi` trait rather
than to OpenGL, so the renderer — where the bugs live — is tested on a
desktop CI with `MockGl`, which records every call. The Android layer
implements `GlApi` on top of the NDK's GLES3.

### [3] Media I/O
`vimo-media/src/decoder.rs` defines the decoder/encoder traits and ships
software implementations for tests. `vimo-android/src/mediacodec.rs` is the
hardware implementation: it decodes into a surface, which the GPU then samples
directly. `capability.rs` detects what the device
can do and picks the best path. `vfr.rs` handles variable frame rates by
keeping the real presentation timestamps. `surface.rs` defines the zero-copy
`Surface → GPU texture` contract.

### [4] Playback
`vimo-engine/src/clock.rs` is the master clock (audio driven, injectable time
source, looping, reverse, rate changes, drift correction). `prefetch.rs`
decides what to decode ahead of the playhead. The engine's `render_frame`
implements the drop/hold policy when the renderer falls behind the clock.

### [5] Audio engine
`vimo-audio/src/mixer.rs` (multi-track mixing with pan and gain),
`envelope.rs` (fades and automation curves), `resampler.rs` (streaming
Catmull-Rom sample-rate conversion), `waveform.rs` (peak generation and
cache), `output.rs` (the low-latency sink trait that Oboe implements).

### [6] Resource management
`vimo-resources/src/pools.rs` (decoder pool that respects the hardware limit,
texture pool, buffer pool), `frame_cache.rs` (LRU frame cache bounded by
bytes), `memory.rs` (accounting, pressure levels, trim policy).

### [7] Device adaptation
`vimo-device/src/tier.rs` classifies the device and maps it onto the quality
ladder (preview scale, prefetch depth, decoder limit, proxy usage).
`thermal.rs` watches the thermal status with hysteresis and lowers the preview
quality before the device throttles.

### [8] Export
`vimo-engine/src/export.rs`. The export uses the same render graph at full
resolution with `preview = false`, and renders frame *N* at exactly
`N * frame_duration`, which makes the output deterministic. The pipeline is
step-driven so it can run inside an Android foreground service, and supports
progress, cancel, pause and resume from a checkpoint.

### [9] FFI boundary
`vimo-ffi/src/lib.rs` and `crates/vimo-ffi/include/vimo_core.h`. Opaque
handle, commands in, events out, `catch_unwind` on every entry point, the
surface handed over once.

The command set covers the whole editor: playback, clips (add, remove, move,
trim, split), tracks, effects, keyframes, assets, transitions, the project
settings, the files and the export. The UI reads the project through
`vimo_engine_snapshot`, which returns a JSON view of the timeline.

### [10] Engineering
- Dedicated render thread: `vimo-engine/src/render_thread.rs` — the GL context
  belongs to that thread and never crosses a thread boundary.
- Tracing: `vimo-foundation/src/trace.rs` — named spans that the Android
  layer maps onto Perfetto tracks. Disabled by default, one atomic load when
  off.
- Errors: `vimo-foundation/src/error.rs` — a closed enum with stable numeric
  codes.
- Tests: 250 unit tests, 4 golden-frame tests with image diffs
  (`crates/vimo-render/tests/golden.rs`), 9 renderer tests that assert the
  exact GL calls through the mock (`crates/vimo-gl/tests/renderer.rs`), and
  deterministic fuzz-style tests for the project loader and the manifest
  parser.
- Fuzzing: `fuzz/` contains `cargo-fuzz` targets for the project loader and
  the manifest parser.

---

## Building and testing

```sh
cargo build --workspace          # build everything
cargo test  --workspace          # 250+ tests
cargo run  --release -p vimo-bench   # benchmarks with enforced budgets
cargo +nightly fuzz run project_loader
```

The Android libraries are built with `cargo ndk` and published as a release
asset; the Vimo app downloads that asset at build time, so the app repository
contains no native code. See `.github/workflows/native-release.yml`.

### The caches

Every cache lives under the **XDG base directory**, so there is one
predictable root to clear and nothing is scattered through the home
directory:

| Cache | Location |
|-------|----------|
| The cargo registry and the installed binaries | `$XDG_CACHE_HOME/cargo` |
| The Gradle dependencies and the downloaded native libraries | `$XDG_CACHE_HOME/gradle` and `$XDG_CACHE_HOME/vimo-native` |

The CI sets `XDG_CACHE_HOME` explicitly, and a **prune job** runs after every
build and keeps only the twenty newest repository caches.

## Benchmarks

Every benchmark has a budget derived from the product requirements; the run
fails if one is exceeded. Measured on the development machine (x86_64, 4
cores):

```
ok   frame_at (16 clips)                     0.610 µs/iter  (budget  100.000 µs)
ok   frame_at (200 clips)                    4.872 µs/iter  (budget 1000.000 µs)
ok   project save (64 clips)                 4.934 µs/iter  (budget 50000.000 µs)
ok   project load (64 clips)                 6.644 µs/iter  (budget 100000.000 µs)
ok   undo (clip edit)                        0.201 µs/iter  (budget   50.000 µs)
ok   manifest parse                          4.917 µs/iter  (budget  200.000 µs)
ok   frame cache lookup                      0.013 µs/iter  (budget    0.500 µs)
ok   mix 4096 frames                        44.192 µs/iter  (budget  200.000 µs)
ok   resample 1024 frames                   62.735 µs/iter  (budget  200.000 µs)
ok   frame_at (1 effect)                     0.089 µs/iter  (budget  100.000 µs)
```

A 16 ms frame budget at 60 fps leaves roughly 25 000× headroom for building
the frame plan, which is the point: the plan costs microseconds, so the time
goes to decoding and drawing, where it belongs.

## Adding a platform layer

The core never talks to `MediaCodec`, Oboe, EGL or Perfetto directly. To
port it to a platform, implement:

| Trait | Crate | Purpose |
|-------|-------|---------|
| `Decoder` / `Encoder` | `vimo-media` | decode and encode media |
| `CapabilityProbe` | `vimo-media` | report what the device supports |
| `SurfaceSource` | `vimo-media` | zero-copy `Surface → texture` |
| `AudioOutput` | `vimo-audio` | low-latency playback (Oboe) |
| `TextureAllocator` | `vimo-resources` | allocate and free GL textures |
| `ShaderProvider` | `vimo-render` | compile GLSL into programs |
| `RenderContext` | `vimo-engine` | draw a frame plan (owns the GL context) |
| `TraceSink` | `vimo-foundation` | emit Perfetto spans |
| `DeviceProbe` | `vimo-device` | report the device tier |
| `GlApi` | `vimo-gl` | the OpenGL ES calls the renderer makes |
| `ShaderSourceProvider` | `vimo-gl` | where an effect's GLSL comes from |

The Android implementations live in a separate repository
(`Shrawan-13/vimo-android`), so the core's CI stays fast and does not need
the NDK.
