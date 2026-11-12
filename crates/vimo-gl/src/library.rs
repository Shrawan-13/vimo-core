//! The shader library.
//!
//! The library pairs the effect **registry** (what effects exist and what
//! parameters they take) with the **sources** (the GLSL that implements them).
//! The two are kept together so that a mismatch between them can be found at
//! load time rather than on the user's device.
//!
//! ```
//! use vimo_gl::ShaderLibrary;
//!
//! let mut library = ShaderLibrary::new();
//! let count = library.load_manifest(
//!     r#"{"effects":[{"id":"com.vimo.effect.brightness",
//!         "shader":"shaders/brightness.frag",
//!         "params":[{"name":"amount","type":"float","default":0.0}]}]}"#,
//! ).unwrap();
//! assert_eq!(count, 1);
//! ```

use std::collections::HashMap;

use vimo_foundation::error::{invalid_argument, VimoResult};
use vimo_manifest::{EffectDesc, EffectRegistry, EffectTypeId};
use vimo_render::shader::{parse_uniforms, validate_uniforms, UniformMismatch};

use crate::renderer::ShaderSourceProvider;

/// A registry of effects plus their GLSL sources.
#[derive(Debug, Clone, Default)]
pub struct ShaderLibrary {
    registry: EffectRegistry,
    sources: HashMap<String, String>,
}

impl ShaderLibrary {
    /// An empty library.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The effect registry.
    #[must_use]
    pub fn registry(&self) -> &EffectRegistry {
        &self.registry
    }

    /// The effect registry, mutably.
    #[must_use]
    pub fn registry_mut(&mut self) -> &mut EffectRegistry {
        &mut self.registry
    }

    /// Add one effect definition.
    pub fn add_effect(&mut self, desc: EffectDesc) -> VimoResult<EffectTypeId> {
        self.registry.register(desc)
    }

    /// Parse a manifest and register every effect in it.
    ///
    /// Returns the number of effects that were registered.
    pub fn load_manifest(&mut self, json: &str) -> VimoResult<usize> {
        let manifest = vimo_manifest::Manifest::parse_str(json)?;
        let count = manifest.effects.len();
        for desc in manifest.effects {
            self.registry.register(desc)?;
        }
        Ok(count)
    }

    /// Add a shader source, keyed by its asset path.
    pub fn add_source(&mut self, path: impl Into<String>, source: impl Into<String>) {
        self.sources.insert(path.into(), source.into());
    }

    /// The number of shader sources.
    #[must_use]
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// The source of a shader, by path.
    #[must_use]
    pub fn source(&self, path: &str) -> Option<&str> {
        self.sources.get(path).map(String::as_str)
    }

    /// Check every effect's shader against its parameters.
    ///
    /// Returns the mismatches that were found. An empty result means the
    /// library is consistent.
    pub fn validate(&self) -> Vec<UniformMismatch> {
        let mut out = Vec::new();
        for (id, desc) in self.registry.iter() {
            let Some(path) = desc.shader.as_ref() else {
                continue;
            };
            let Some(source) = self.sources.get(path) else {
                continue;
            };
            let uniforms = parse_uniforms(source);
            out.extend(validate_uniforms(&uniforms, desc));
            let _ = id;
        }
        out
    }

    /// The path of the shader an effect uses.
    #[must_use]
    pub fn shader_path(&self, effect: EffectTypeId) -> Option<&str> {
        self.registry.get(effect)?.shader.as_deref()
    }
}

impl ShaderSourceProvider for ShaderLibrary {
    fn fragment_source(&mut self, effect_type: EffectTypeId) -> VimoResult<String> {
        let path = self
            .registry
            .get(effect_type)
            .and_then(|d| d.shader.clone())
            .ok_or_else(|| invalid_argument("shader: the effect has no shader"))?;
        self.sources
            .get(&path)
            .cloned()
            .ok_or_else(|| invalid_argument("shader: the source is missing"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r##"{"effects":[
        {"id":"com.vimo.effect.brightness","shader":"vimo/shaders/brightness.frag",
         "params":[{"name":"amount","type":"float","default":0.0,"min":-1.0,"max":1.0}]},
        {"id":"com.vimo.effect.tint","shader":"vimo/shaders/tint.frag",
         "params":[{"name":"color","type":"color","default":"#ffffffff"}]}
    ]}"##;

    #[test]
    fn a_manifest_registers_its_effects() {
        let mut library = ShaderLibrary::new();
        assert_eq!(library.load_manifest(MANIFEST).unwrap(), 2);
        assert!(library.registry().lookup("com.vimo.effect.brightness").is_some());
    }

    #[test]
    fn the_sources_are_found_by_path() {
        let mut library = ShaderLibrary::new();
        library.load_manifest(MANIFEST).unwrap();
        library.add_source("vimo/shaders/brightness.frag", "void main() {}");
        let id = library.registry().lookup("com.vimo.effect.brightness").unwrap();
        assert!(library.fragment_source(id).is_ok());
    }

    #[test]
    fn a_missing_source_is_an_error() {
        let mut library = ShaderLibrary::new();
        library.load_manifest(MANIFEST).unwrap();
        let id = library.registry().lookup("com.vimo.effect.brightness").unwrap();
        assert!(library.fragment_source(id).is_err());
    }

    #[test]
    fn the_validation_finds_a_missing_parameter() {
        let mut library = ShaderLibrary::new();
        library.load_manifest(MANIFEST).unwrap();
        // The shader declares a uniform the manifest does not.
        library.add_source(
            "vimo/shaders/brightness.frag",
            "uniform sampler2D u_texture;\nuniform float u_amount;\nuniform float u_ghost;\nvoid main() {}",
        );
        let mismatches = library.validate();
        assert_eq!(mismatches.len(), 1);
    }

    #[test]
    fn a_consistent_library_validates() {
        let mut library = ShaderLibrary::new();
        library.load_manifest(MANIFEST).unwrap();
        library.add_source(
            "vimo/shaders/brightness.frag",
            "uniform sampler2D u_texture;\nuniform float u_amount;\nvoid main() {}",
        );
        library.add_source(
            "vimo/shaders/tint.frag",
            "uniform sampler2D u_texture;\nuniform vec4 u_color;\nvoid main() {}",
        );
        assert!(library.validate().is_empty());
    }
}
