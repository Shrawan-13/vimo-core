//! Session services: loading, saving, autosaving and snapshots.
//!
//! These live on the engine rather than in the FFI layer because they are
//! core features: the codec, the autosave store and the snapshot are all
//! platform independent. The platform only supplies *paths*.

use std::path::{Path, PathBuf};

use vimo_foundation::error::{invalid_argument, VimoResult};
use vimo_manifest::EffectRegistry;
use vimo_project::autosave::AutosaveStore;
use vimo_project::codec;
use vimo_project::snapshot::snapshot;
use vimo_project::Project;

/// The file services of a session.
#[derive(Debug, Default)]
pub struct SessionFiles {
    autosave: Option<AutosaveStore>,
}

impl SessionFiles {
    /// A session with no autosave configured.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable autosave into `dir`.
    pub fn set_autosave_dir(&mut self, dir: impl Into<PathBuf>) {
        self.autosave = Some(AutosaveStore::new(dir.into()));
    }

    /// The autosave store, if one is configured.
    #[must_use]
    pub fn autosave_store(&self) -> Option<&AutosaveStore> {
        self.autosave.as_ref()
    }

    /// Write a snapshot of the project.
    pub fn autosave(
        &mut self,
        project: &Project,
        registry: &EffectRegistry,
    ) -> VimoResult<PathBuf> {
        let store = self
            .autosave
            .as_mut()
            .ok_or_else(|| invalid_argument("autosave: no directory configured"))?;
        store.write_snapshot(project, registry)
    }

    /// Load the newest autosave, if there is one.
    pub fn recover(&self, registry: &EffectRegistry) -> VimoResult<Option<Project>> {
        let Some(store) = &self.autosave else {
            return Ok(None);
        };
        Ok(store.load_latest(registry)?.map(|(project, _)| project))
    }

    /// Write the project to `path`.
    pub fn save(
        &self,
        project: &Project,
        registry: &EffectRegistry,
        path: &Path,
    ) -> VimoResult<()> {
        let bytes = codec::write(project, registry, codec::CURRENT_VERSION)?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    vimo_foundation::error::VimoError::detailed(
                        vimo_foundation::ErrorKind::Io,
                        "save project",
                        format!("create {}: {e}", parent.display()),
                    )
                })?;
            }
        }
        // Write to a temporary file and rename, so that a crash cannot leave a
        // half-written project behind.
        let temp = path.with_extension("vimo.tmp");
        std::fs::write(&temp, &bytes).map_err(|e| {
            vimo_foundation::error::VimoError::detailed(
                vimo_foundation::ErrorKind::Io,
                "save project",
                format!("write {}: {e}", temp.display()),
            )
        })?;
        std::fs::rename(&temp, path).map_err(|e| {
            vimo_foundation::error::VimoError::detailed(
                vimo_foundation::ErrorKind::Io,
                "save project",
                format!("rename {}: {e}", temp.display()),
            )
        })?;
        Ok(())
    }

    /// Load a project from `path`.
    pub fn load(
        path: &Path,
        registry: &EffectRegistry,
    ) -> VimoResult<(Project, codec::LoadReport)> {
        let bytes = std::fs::read(path).map_err(|e| {
            vimo_foundation::error::VimoError::detailed(
                vimo_foundation::ErrorKind::Io,
                "load project",
                format!("read {}: {e}", path.display()),
            )
        })?;
        codec::read(&bytes, registry)
    }
}

/// The JSON snapshot of a project, for the UI.
#[must_use]
pub fn project_snapshot(project: &Project) -> String {
    serde_json::to_string(&snapshot(project)).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vimo_project::model::{Asset, Track, TrackKind};

    fn project() -> Project {
        let mut p = Project::new();
        p.assets.push(Asset::video(
            "src",
            vimo_foundation::time::Time::from_secs(5),
        ));
        p.tracks.push(Track::new("V1", TrackKind::Video));
        p
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("vimo-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("project.vimo");

        let registry = EffectRegistry::new();
        let files = SessionFiles::new();
        files.save(&project(), &registry, &path).unwrap();
        assert!(path.exists());

        let (loaded, _) = SessionFiles::load(&path, &registry).unwrap();
        assert_eq!(loaded.assets.len(), 1);
        assert_eq!(loaded.tracks.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autosave_writes_and_recovers() {
        let dir = std::env::temp_dir().join(format!("vimo-autosave-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let registry = EffectRegistry::new();
        let mut files = SessionFiles::new();
        files.set_autosave_dir(&dir);
        files.autosave(&project(), &registry).unwrap();
        let recovered = files.recover(&registry).unwrap().unwrap();
        assert_eq!(recovered.assets.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_snapshot_is_json() {
        let text = project_snapshot(&project());
        assert!(text.contains("\"tracks\""));
        assert!(text.contains("\"meta\""));
    }
}
