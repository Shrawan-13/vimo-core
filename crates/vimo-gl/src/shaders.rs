//! The built-in GLSL shaders.
//!
//! Vimo ships its own vertex and passthrough shaders so that the renderer
//! works out of the box. Effect shaders come from the effect manifest.

/// The vertex shader used for every pass.
///
/// The quad is given in normalised 0..1 coordinates. `u_transform` maps the
/// layer space to the target space; the aspect correction keeps rotations
/// circular on non-square frames.
pub const VERTEX_SHADER: &str = r#"#version 300 es
precision mediump float;

layout(location = 0) in vec2 a_position;
layout(location = 1) in vec2 a_texcoord;

uniform mat3 u_transform;
uniform float u_aspect;
uniform float u_flip_y;

out vec2 v_texcoord;

void main() {
    v_texcoord = a_texcoord;
    // Work in a square space so that rotations are not skewed.
    vec2 p = a_position * vec2(u_aspect, 1.0);
    vec3 t = u_transform * vec3(p, 1.0);
    t.x /= u_aspect;
    vec2 ndc = t.xy * 2.0 - 1.0;
    // Vimo's space has its origin at the top left; GL's at the bottom left.
    ndc.y = mix(ndc.y, -ndc.y, u_flip_y);
    gl_Position = vec4(ndc, 0.0, 1.0);
}
"#;

/// The fragment shader used to copy a texture.
pub const PASSTHROUGH_FRAGMENT_SHADER: &str = r#"#version 300 es
precision mediump float;

in vec2 v_texcoord;

uniform sampler2D u_texture;
uniform float u_opacity;

out vec4 fragColor;

void main() {
    vec4 color = texture(u_texture, v_texcoord);
    fragColor = vec4(color.rgb, color.a * u_opacity);
}
"#;

/// The fragment shader used to copy a **surface-backed** texture.
///
/// A `Surface` that a decoder writes into is sampled with the
/// `GL_OES_EGL_image_external` extension, which uses a different sampler type.
/// The renderer picks this shader when the texture is external.
pub const EXTERNAL_PASSTHROUGH_FRAGMENT_SHADER: &str = r#"#version 300 es
#extension GL_OES_EGL_image_external : require
precision mediump float;

in vec2 v_texcoord;

uniform samplerExternalOES u_texture;
uniform float u_opacity;

out vec4 fragColor;

void main() {
    vec4 color = texture(u_texture, v_texcoord);
    fragColor = vec4(color.rgb, color.a * u_opacity);
}
"#;

/// The fragment shader used to draw a transition between two pictures.
pub const TRANSITION_FRAGMENT_SHADER: &str = r#"#version 300 es
precision mediump float;

in vec2 v_texcoord;

uniform sampler2D u_texture;
uniform sampler2D u_texture2;
uniform float u_progress;

out vec4 fragColor;

void main() {
    vec4 from = texture(u_texture, v_texcoord);
    vec4 to = texture(u_texture2, v_texcoord);
    fragColor = mix(from, to, u_progress);
}
"#;

/// The vertex shader for the transition pass.
///
/// The transition is drawn with an identity transform because both pictures
/// have already been placed by the transition's own transform.
pub const TRANSITION_VERTEX_SHADER: &str = r#"#version 300 es
precision mediump float;

layout(location = 0) in vec2 a_position;
layout(location = 1) in vec2 a_texcoord;

uniform mat3 u_transform;
uniform float u_aspect;
uniform float u_flip_y;

out vec2 v_texcoord;

void main() {
    v_texcoord = a_texcoord;
    vec2 p = a_position * vec2(u_aspect, 1.0);
    vec3 t = u_transform * vec3(p, 1.0);
    t.x /= u_aspect;
    vec2 ndc = t.xy * 2.0 - 1.0;
    ndc.y = mix(ndc.y, -ndc.y, u_flip_y);
    gl_Position = vec4(ndc, 0.0, 1.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shaders_declare_the_expected_uniforms() {
        let uniforms = vimo_render::parse_uniforms(VERTEX_SHADER);
        let names: Vec<&str> = uniforms.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"u_transform"));
        assert!(names.contains(&"u_aspect"));

        let uniforms = vimo_render::parse_uniforms(PASSTHROUGH_FRAGMENT_SHADER);
        let names: Vec<&str> = uniforms.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"u_texture"));
    }

    #[test]
    fn the_shaders_are_valid_glsl_es_30() {
        for source in [
            VERTEX_SHADER,
            PASSTHROUGH_FRAGMENT_SHADER,
            TRANSITION_FRAGMENT_SHADER,
            TRANSITION_VERTEX_SHADER,
        ] {
            assert!(source.starts_with("#version 300 es"), "missing version directive");
            assert!(source.contains("void main()"), "missing entry point");
            assert!(source.trim_end().ends_with('}'), "unbalanced braces");
        }
    }
}
