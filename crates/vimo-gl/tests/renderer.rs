//! Tests for the GL renderer, driven by the mock GL implementation.
//!
//! These assert the things that are easy to get wrong and hard to debug on a
//! device: the blend factors, the uniform values, the pass structure and the
//! draw order.

use vimo_foundation::color::Rgba8;
use vimo_foundation::math::{Mat3, Rect};
use vimo_foundation::time::Time;
use vimo_gl::api::consts;
use vimo_gl::mock::{Call, MockGl};
use vimo_gl::{GlRenderer, NoShaders, ShaderSourceProvider};
use vimo_manifest::{EffectDesc, EffectRegistry, EffectTypeId, ParamDesc, ParamType, ParamValue};
use vimo_project::model::BlendMode;
use vimo_render::uniforms::{BuiltinUniforms, UniformArena, UniformBlock, UniformRange};
use vimo_render::{DrawStep, EffectIndex, FrameEffect, FramePlan, LayerIndex, TextureId, TextureRef};

const UNIFORM_NAMES: &[&str] = &[
    "u_transform",
    "u_aspect",
    "u_flip_y",
    "u_texture",
    "u_texture2",
    "u_opacity",
    "u_time",
    "u_resolution",
    "u_texel",
    "u_progress",
    "u_radius",
];

/// A provider that returns a fixed shader for every effect.
struct FixedShaders {
    source: String,
}

impl ShaderSourceProvider for FixedShaders {
    fn fragment_source(&mut self, _effect_type: EffectTypeId) -> vimo_foundation::VimoResult<String> {
        Ok(self.source.clone())
    }
}

fn registry_with_effect() -> (EffectRegistry, EffectTypeId) {
    let mut registry = EffectRegistry::new();
    let desc = EffectDesc::new(
        "com.vimo.effect.test",
        "Test",
        "test",
        1,
        Some("test.frag".to_string()),
        vec![ParamDesc {
            name: "radius".into(),
            ptype: ParamType::Float,
            default: ParamValue::Float(1.0),
            min: None,
            max: None,
            step: None,
            label: "Radius".into(),
            animatable: true,
            options: Vec::new(),
        }],
        Default::default(),
    )
    .unwrap();
    let id = registry.register(desc).unwrap();
    (registry, id)
}

/// A plan with one composite step and no effects.
fn simple_plan(texture: TextureId, blend: BlendMode, opacity: f32) -> FramePlan {
    let mut plan = FramePlan::new();
    plan.width = 640;
    plan.height = 360;
    plan.steps.push(DrawStep::Clear { color: Rgba8::BLACK });
    plan.steps.push(DrawStep::Composite {
        layer: LayerIndex(0),
        source: TextureRef { clip: 0, source_time: Time::ZERO, texture },
        transform: Mat3::IDENTITY,
        src_rect: Rect::FULL,
        dst_rect: Rect::FULL,
        blend,
        opacity,
        effects: Vec::new(),
    });
    plan
}

#[test]
fn a_frame_clears_and_draws_once() {
    let mock = MockGl::with_uniforms(UNIFORM_NAMES);
    let handle = mock.clone();
    let mut renderer = GlRenderer::new(Box::new(mock), EffectRegistry::new(), 640, 360).unwrap();
    let plan = simple_plan(TextureId(7), BlendMode::Normal, 1.0);
    renderer.draw(&plan, &mut NoShaders).unwrap();

    assert_eq!(handle.draw_calls(), 1, "one composite, one draw call");
    assert!(
        handle.calls_of(|c| matches!(c, Call::Clear(_))).len() == 1,
        "the clear must happen"
    );
    assert_eq!(handle.bound_textures(), vec![TextureId(7)], "the source must be bound");
}

#[test]
fn the_blend_factors_follow_the_blend_mode() {
    let cases = [
        (BlendMode::Normal, (consts::ONE, consts::ONE_MINUS_SRC_ALPHA)),
        (BlendMode::Multiply, (consts::DST_COLOR, consts::ZERO)),
        (BlendMode::Screen, (consts::ONE, consts::ONE_MINUS_SRC_COLOR)),
        (BlendMode::Add, (consts::ONE, consts::ONE)),
        (BlendMode::Lighten, (consts::MAX, consts::MAX)),
    ];
    for (mode, expected) in cases {
        let mock = MockGl::with_uniforms(UNIFORM_NAMES);
        let handle = mock.clone();
        let mut renderer = GlRenderer::new(Box::new(mock), EffectRegistry::new(), 640, 360).unwrap();
        let plan = simple_plan(TextureId(1), mode, 1.0);
        renderer.draw(&plan, &mut NoShaders).unwrap();

        let factors = handle.last_blend_func().expect("a blend function must be set");
        assert_eq!(
            (factors.0, factors.1),
            expected,
            "wrong blend factors for {mode:?}"
        );
    }
}

#[test]
fn non_separable_modes_fall_back_to_over() {
    let mock = MockGl::with_uniforms(UNIFORM_NAMES);
    let handle = mock.clone();
    let mut renderer = GlRenderer::new(Box::new(mock), EffectRegistry::new(), 640, 360).unwrap();
    let plan = simple_plan(TextureId(1), BlendMode::Hue, 1.0);
    renderer.draw(&plan, &mut NoShaders).unwrap();
    let factors = handle.last_blend_func().expect("a blend function must be set");
    assert_eq!(factors.0, consts::ONE);
    assert_eq!(factors.1, consts::ONE_MINUS_SRC_ALPHA);
}

#[test]
fn the_opacity_is_uploaded() {
    let mock = MockGl::with_uniforms(UNIFORM_NAMES);
    let handle = mock.clone();
    let mut renderer = GlRenderer::new(Box::new(mock), EffectRegistry::new(), 640, 360).unwrap();
    let plan = simple_plan(TextureId(1), BlendMode::Normal, 0.25);
    renderer.draw(&plan, &mut NoShaders).unwrap();
    assert_eq!(handle.uniform_value("u_opacity"), Some(0.25));
}

#[test]
fn the_final_pass_flips_the_image() {
    let mock = MockGl::with_uniforms(UNIFORM_NAMES);
    let handle = mock.clone();
    let mut renderer = GlRenderer::new(Box::new(mock), EffectRegistry::new(), 640, 360).unwrap();
    let plan = simple_plan(TextureId(1), BlendMode::Normal, 1.0);
    renderer.draw(&plan, &mut NoShaders).unwrap();
    // The final pass composites onto the default framebuffer and flips.
    assert_eq!(handle.uniform_value("u_flip_y"), Some(1.0));
    assert!(
        handle.bound_framebuffers().last().copied().flatten().is_none(),
        "the last bound framebuffer must be the default one"
    );
}

#[test]
fn an_effect_chain_uses_offscreen_targets() {
    let (registry, effect_type) = registry_with_effect();
    let mock = MockGl::with_uniforms(UNIFORM_NAMES);
    let handle = mock.clone();
    let mut renderer = GlRenderer::new(Box::new(mock), registry, 640, 360).unwrap();

    let mut plan = FramePlan::new();
    plan.width = 640;
    plan.height = 360;

    let mut block = UniformBlock::new();
    block.push_float(4.0);
    let range = plan.uniforms.push_block(&block);
    plan.effects.push(FrameEffect {
        effect_type,
        builtins: BuiltinUniforms {
            time: 1.5,
            resolution: [640.0, 360.0],
            texel: [1.0 / 640.0, 1.0 / 360.0],
            opacity: 1.0,
        },
        range,
    });

    plan.steps.push(DrawStep::Clear { color: Rgba8::BLACK });
    plan.steps.push(DrawStep::Composite {
        layer: LayerIndex(0),
        source: TextureRef { clip: 0, source_time: Time::ZERO, texture: TextureId(1) },
        transform: Mat3::IDENTITY,
        src_rect: Rect::FULL,
        dst_rect: Rect::FULL,
        blend: BlendMode::Normal,
        opacity: 1.0,
        effects: vec![EffectIndex(0)],
    });

    let mut shaders = FixedShaders { source: "void main() {}".to_string() };
    renderer.draw(&plan, &mut shaders).unwrap();

    // One framebuffer for the source pass and one for the effect pass.
    assert_eq!(handle.created_framebuffers(), 2, "two offscreen targets");
    // Three draws: the source pass, the effect pass and the final composite.
    assert_eq!(handle.draw_calls(), 3);
    // The effect's parameters were uploaded.
    assert_eq!(handle.uniform_value("u_radius"), Some(4.0));
    // The built-ins were uploaded too.
    assert_eq!(handle.uniform_value("u_time"), Some(1.5));
}

#[test]
fn the_effect_program_is_compiled_once() {
    let (registry, effect_type) = registry_with_effect();
    let mock = MockGl::with_uniforms(UNIFORM_NAMES);
    let handle = mock.clone();
    let mut renderer = GlRenderer::new(Box::new(mock), registry, 640, 360).unwrap();

    let mut plan = FramePlan::new();
    plan.width = 640;
    plan.height = 360;
    let mut block = UniformBlock::new();
    block.push_float(1.0);
    let range = plan.uniforms.push_block(&block);
    plan.effects.push(FrameEffect {
        effect_type,
        builtins: BuiltinUniforms::default(),
        range,
    });
    plan.steps.push(DrawStep::Composite {
        layer: LayerIndex(0),
        source: TextureRef { clip: 0, source_time: Time::ZERO, texture: TextureId(1) },
        transform: Mat3::IDENTITY,
        src_rect: Rect::FULL,
        dst_rect: Rect::FULL,
        blend: BlendMode::Normal,
        opacity: 1.0,
        effects: vec![EffectIndex(0)],
    });

    let mut shaders = FixedShaders { source: "void main() {}".to_string() };
    renderer.draw(&plan, &mut shaders).unwrap();
    let after_first = handle
        .calls_of(|c| matches!(c, Call::CreateProgram { .. }))
        .len();
    renderer.draw(&plan, &mut shaders).unwrap();
    let after_second = handle
        .calls_of(|c| matches!(c, Call::CreateProgram { .. }))
        .len();
    assert_eq!(after_first, after_second, "the program must be cached");
}

#[test]
fn a_gl_error_is_reported() {
    let mock = MockGl::with_uniforms(UNIFORM_NAMES);
    let handle = mock.clone();
    let mut renderer = GlRenderer::new(Box::new(mock), EffectRegistry::new(), 640, 360).unwrap();
    handle.set_error(vimo_gl::GlError::InvalidOperation);
    let plan = simple_plan(TextureId(1), BlendMode::Normal, 1.0);
    let err = renderer.draw(&plan, &mut NoShaders).unwrap_err();
    assert_eq!(err.kind(), vimo_foundation::ErrorKind::Graphics);
}

#[test]
fn the_arena_copies_the_uniform_values() {
    let mut arena = UniformArena::new();
    let mut block = UniformBlock::new();
    block.push_float(1.0);
    block.push_float(2.0);
    let range = arena.push_block(&block);
    assert_eq!(arena.floats_at(range), &[1.0, 2.0]);
    assert!(!range.is_empty());
    assert!(UniformRange::default().is_empty());
}
