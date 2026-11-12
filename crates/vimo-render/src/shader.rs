//! Shader program resolution and GLSL introspection.
//!
//! The core never compiles GLSL: that is the platform layer's job (OpenGL ES
//! on Android, a desktop GL context in tests). The core only needs to know
//! *which* program an effect uses, so that a renderer can bind the right
//! program and upload the matching uniforms.
//!
//! It does, however, **parse** the GLSL source to check that the uniforms a
//! shader declares match the parameters the manifest declares. Catching that
//! mismatch at load time (instead of at draw time, on the user's device) is
//! worth a few lines of parsing.

use vimo_foundation::error::{unsupported, VimoResult};
use vimo_manifest::{EffectDesc, EffectTypeId, ParamType};

/// An opaque handle to a compiled shader program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ProgramId(pub u32);

/// The GLSL type of a uniform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlType {
    /// `float`
    Float,
    /// `vec2`
    Vec2,
    /// `vec3`
    Vec3,
    /// `vec4`
    Vec4,
    /// `int`
    Int,
    /// `bool`
    Bool,
    /// `sampler2D`
    Sampler2D,
    /// `mat3` (a column-major 3x3 matrix).
    Mat3,
    /// `mat4` (a column-major 4x4 matrix).
    Mat4,
}

impl GlType {
    /// The GLSL type name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            GlType::Float => "float",
            GlType::Vec2 => "vec2",
            GlType::Vec3 => "vec3",
            GlType::Vec4 => "vec4",
            GlType::Int => "int",
            GlType::Bool => "bool",
            GlType::Sampler2D => "sampler2D",
            GlType::Mat3 => "mat3",
            GlType::Mat4 => "mat4",
        }
    }

    /// The number of components (1 for scalars and samplers).
    #[must_use]
    pub const fn components(self) -> u32 {
        match self {
            GlType::Float | GlType::Int | GlType::Bool | GlType::Sampler2D => 1,
            GlType::Vec2 => 2,
            GlType::Vec3 => 3,
            GlType::Vec4 => 4,
            GlType::Mat3 => 9,
            GlType::Mat4 => 16,
        }
    }

    /// Parse a GLSL type name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "float" => GlType::Float,
            "vec2" => GlType::Vec2,
            "vec3" => GlType::Vec3,
            "vec4" => GlType::Vec4,
            "int" => GlType::Int,
            "bool" => GlType::Bool,
            "sampler2D" => GlType::Sampler2D,
            "mat3" => GlType::Mat3,
            "mat4" => GlType::Mat4,
            _ => return None,
        })
    }

    /// The manifest parameter type this uniform expects, if any.
    #[must_use]
    pub const fn expected_param_type(self) -> Option<ParamType> {
        Some(match self {
            GlType::Float => ParamType::Float,
            GlType::Vec2 => ParamType::Vec2,
            GlType::Vec3 => ParamType::Vec3,
            GlType::Vec4 => ParamType::Color,
            GlType::Int => ParamType::Int,
            GlType::Bool => ParamType::Bool,
            GlType::Sampler2D | GlType::Mat3 | GlType::Mat4 => return None,
        })
    }
}

/// A uniform declared by a shader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderUniform {
    /// The uniform's name (without the `u_` prefix).
    pub name: String,
    /// The GLSL type.
    pub gl_type: GlType,
    /// The array length (0 for a scalar).
    pub array_size: u32,
}

/// The uniforms the renderer supplies, which no manifest parameter may shadow.
///
/// `u_texture` and `u_texture2` carry the pictures a pass samples; the rest
/// are the values the engine computes.
pub const BUILTIN_UNIFORMS: [&str; 7] = [
    "u_time",
    "u_resolution",
    "u_texel",
    "u_opacity",
    "u_progress",
    "u_texture",
    "u_texture2",
];

/// Parse the uniform declarations out of a GLSL source file.
///
/// The parser is intentionally simple: it looks for `uniform <type> <name>;`
/// declarations, ignoring comments and preprocessor lines. It is not a full
/// GLSL parser and does not need to be — the goal is to check the interface,
/// not to validate the shader.
#[must_use]
pub fn parse_uniforms(source: &str) -> Vec<ShaderUniform> {
    let mut out = Vec::new();
    for line in source.lines() {
        // Strip comments.
        let line = match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        };
        let line = line.trim();
        if line.starts_with('#') || !line.starts_with("uniform ") {
            continue;
        }
        let rest = &line["uniform ".len()..];
        let Some((type_name, declarators)) = rest.split_once(char::is_whitespace) else {
            continue;
        };
        let Some(gl_type) = GlType::parse(type_name.trim()) else {
            continue;
        };
        for declarator in declarators.split(',') {
            let declarator = declarator.trim().trim_end_matches(';');
            if declarator.is_empty() {
                continue;
            }
            let (name, array_size) = match declarator.split_once('[') {
                Some((n, rest)) => {
                    let size = rest.trim_end_matches(']').trim();
                    let size = size.parse::<u32>().unwrap_or(0);
                    (n.trim().to_string(), size)
                }
                None => (declarator.to_string(), 0),
            };
            out.push(ShaderUniform { name, gl_type, array_size });
        }
    }
    out
}

/// A mismatch between a shader's uniforms and the manifest's parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UniformMismatch {
    /// The shader wants a uniform the manifest does not declare.
    MissingParam {
        /// The uniform's name.
        uniform: String,
    },
    /// The manifest declares a parameter the shader does not use.
    UnusedParam {
        /// The parameter's name.
        param: String,
    },
    /// The types do not line up.
    TypeMismatch {
        /// The uniform's name.
        uniform: String,
        /// The GLSL type.
        gl_type: GlType,
        /// The manifest type.
        param_type: ParamType,
    },
}

/// Compare a shader's uniforms against an effect's manifest entry.
///
/// Built-in uniforms are ignored.
#[must_use]
pub fn validate_uniforms(uniforms: &[ShaderUniform], desc: &EffectDesc) -> Vec<UniformMismatch> {
    let mut mismatches = Vec::new();
    for uniform in uniforms {
        if BUILTIN_UNIFORMS.contains(&uniform.name.as_str()) {
            continue;
        }
        // The manifest parameter names have no `u_` prefix.
        let Some(param_name) = uniform.name.strip_prefix("u_") else {
            continue;
        };
        let Some(index) = desc.param_index(param_name) else {
            mismatches.push(UniformMismatch::MissingParam { uniform: uniform.name.clone() });
            continue;
        };
        let param = desc.param(index).expect("index in range");
        if let Some(expected) = uniform.gl_type.expected_param_type() {
            if expected != param.ptype {
                mismatches.push(UniformMismatch::TypeMismatch {
                    uniform: uniform.name.clone(),
                    gl_type: uniform.gl_type,
                    param_type: param.ptype,
                });
            }
        }
    }
    // Parameters the shader never reads are not an error (the manifest may
    // declare parameters for several shader variants), so they are reported
    // separately and only by `unused_parameters`.
    mismatches
}

/// The manifest parameters that no uniform refers to.
#[must_use]
pub fn unused_parameters(uniforms: &[ShaderUniform], desc: &EffectDesc) -> Vec<String> {
    let mut out = Vec::new();
    for param in &desc.params {
        let uniform_name = format!("u_{}", param.name);
        if !uniforms.iter().any(|u| u.name == uniform_name) {
            out.push(param.name.clone());
        }
    }
    out
}

/// Resolves effect types to compiled programs.
pub trait ShaderProvider {
    /// Return the program for an effect type, compiling it if necessary.
    fn program_for(&mut self, effect_type: EffectTypeId, desc: &EffectDesc) -> VimoResult<ProgramId>;

    /// Release a single program.
    fn release(&mut self, program: ProgramId);

    /// Release every program.
    fn release_all(&mut self);
}

/// A provider that has no GPU behind it.
///
/// Effects that require a shader are reported as unsupported, which lets the
/// engine fall back to a CPU path or disable the effect gracefully.
#[derive(Debug, Default)]
pub struct NullShaderProvider {
    compiled: u32,
}

impl NullShaderProvider {
    /// A new provider.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl ShaderProvider for NullShaderProvider {
    fn program_for(&mut self, _effect_type: EffectTypeId, desc: &EffectDesc) -> VimoResult<ProgramId> {
        if desc.capabilities.requires_gpu {
            return Err(unsupported("shader: no GPU available"));
        }
        let id = self.compiled;
        self.compiled += 1;
        Ok(ProgramId(id))
    }

    fn release(&mut self, _program: ProgramId) {}

    fn release_all(&mut self) {
        self.compiled = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vimo_manifest::ParamValue;

    fn desc() -> EffectDesc {
        EffectDesc::new(
            "com.vimo.effect.test",
            "Test",
            "test",
            1,
            Some("test.frag".into()),
            vec![
                vimo_manifest::ParamDesc {
                    name: "radius".into(),
                    ptype: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: None,
                    max: None,
                    step: None,
                    label: "Radius".into(),
                    animatable: true,
                    options: Vec::new(),
                },
                vimo_manifest::ParamDesc {
                    name: "tint".into(),
                    ptype: ParamType::Color,
                    default: ParamValue::Color(vimo_foundation::color::Rgba8::WHITE),
                    min: None,
                    max: None,
                    step: None,
                    label: "Tint".into(),
                    animatable: true,
                    options: Vec::new(),
                },
            ],
            Default::default(),
        )
        .expect("descriptor")
    }

    #[test]
    fn uniforms_are_parsed() {
        let source = r#"
            // a comment
            #version 300 es
            precision mediump float;
            uniform float u_radius;
            uniform vec4 u_tint;
            uniform sampler2D u_tex;
            uniform float u_weights[8];
        "#;
        let uniforms = parse_uniforms(source);
        assert_eq!(uniforms.len(), 4);
        assert_eq!(uniforms[0].name, "u_radius");
        assert_eq!(uniforms[0].gl_type, GlType::Float);
        assert_eq!(uniforms[1].gl_type, GlType::Vec4);
        assert_eq!(uniforms[2].gl_type, GlType::Sampler2D);
        assert_eq!(uniforms[3].array_size, 8);
    }

    #[test]
    fn mismatches_are_detected() {
        let uniforms = parse_uniforms("uniform float u_radius;\nuniform float u_missing;");
        let mismatches = validate_uniforms(&uniforms, &desc());
        assert_eq!(mismatches.len(), 1);
        assert!(matches!(mismatches[0], UniformMismatch::MissingParam { .. }));
    }

    #[test]
    fn type_mismatches_are_detected() {
        let uniforms = parse_uniforms("uniform vec2 u_radius;");
        let mismatches = validate_uniforms(&uniforms, &desc());
        assert!(matches!(mismatches[0], UniformMismatch::TypeMismatch { .. }));
    }

    #[test]
    fn builtins_are_ignored() {
        let uniforms = parse_uniforms("uniform float u_time;\nuniform vec2 u_resolution;");
        assert!(validate_uniforms(&uniforms, &desc()).is_empty());
    }

    #[test]
    fn unused_parameters_are_listed() {
        let uniforms = parse_uniforms("uniform float u_radius;");
        let unused = unused_parameters(&uniforms, &desc());
        assert_eq!(unused, vec!["tint".to_string()]);
    }
}
