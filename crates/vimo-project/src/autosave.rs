//! Autosave and crash recovery.
//!
//! # Durability model
//!
//! A snapshot is written to a temporary file, flushed to stable storage with
//! `fsync`, and only then renamed over the final name. A crash at any point
//! therefore leaves either the previous snapshot or the new one — never a
//! half-written file. The directory entry is fsynced too, so the rename itself
//! is durable.
//!
//! # Recovery
//!
//! On startup the app looks for the newest snapshot that is newer than the
//! last explicitly saved project and offers to restore it. Snapshots are
//! pruned to a bounded count so that a long session cannot fill the disk.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use vimo_foundation::error::{VimoError, VimoResult};
use vimo_manifest::EffectRegistry;

use crate::codec::{self, LoadReport};
use crate::project::Project;

/// File extension used for snapshots.
pub const SNAPSHOT_EXTENSION: &str = "vimo";
/// Default number of snapshots kept.
pub const DEFAULT_MAX_SNAPSHOTS: usize = 5;

/// Metadata about a snapshot on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotInfo {
    /// Full path.
    pub path: PathBuf,
    /// Creation time (from the file name, which encodes it).
    pub created_unix_ms: u64,
    /// Size in bytes.
    pub size: u64,
}

/// A directory of autosave snapshots.
#[derive(Debug, Clone)]
pub struct AutosaveStore {
    dir: PathBuf,
    max_snapshots: usize,
    sequence: u64,
}

impl AutosaveStore {
    /// Create a store rooted at `dir`.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        AutosaveStore {
            dir: dir.into(),
            max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            sequence: 0,
        }
    }

    /// Set how many snapshots to keep.
    #[must_use]
    pub fn with_max_snapshots(mut self, max: usize) -> Self {
        self.max_snapshots = max.max(1);
        self
    }

    /// The directory snapshots are stored in.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Ensure the snapshot directory exists.
    pub fn ensure_dir(&self) -> VimoResult<()> {
        fs::create_dir_all(&self.dir).map_err(|e| {
            VimoError::detailed(
                vimo_foundation::ErrorKind::Io,
                "autosave",
                format!("create {}: {e}", self.dir.display()),
            )
        })
    }

    /// Write a snapshot of `project`.
    ///
    /// The write is atomic: on return the snapshot is durable.
    pub fn write_snapshot(
        &self,
        project: &Project,
        registry: &EffectRegistry,
    ) -> VimoResult<PathBuf> {
        self.ensure_dir()?;
        let bytes = codec::write(project, registry, codec::CURRENT_VERSION)?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let seq = self.next_sequence();
        let name = format!("snapshot-{now:013}-{seq:06}.{SNAPSHOT_EXTENSION}");
        let final_path = self.dir.join(&name);
        let temp_path = self.dir.join(format!("{name}.tmp"));

        {
            let mut file = fs::File::create(&temp_path).map_err(|e| {
                VimoError::detailed(
                    vimo_foundation::ErrorKind::Io,
                    "autosave",
                    format!("create {}: {e}", temp_path.display()),
                )
            })?;
            file.write_all(&bytes).map_err(|e| {
                VimoError::detailed(
                    vimo_foundation::ErrorKind::Io,
                    "autosave",
                    format!("write {}: {e}", temp_path.display()),
                )
            })?;
            file.sync_all().map_err(|e| {
                VimoError::detailed(
                    vimo_foundation::ErrorKind::Io,
                    "autosave",
                    format!("fsync {}: {e}", temp_path.display()),
                )
            })?;
        }

        fs::rename(&temp_path, &final_path).map_err(|e| {
            VimoError::detailed(
                vimo_foundation::ErrorKind::Io,
                "autosave",
                format!("rename {}: {e}", temp_path.display()),
            )
        })?;
        self.sync_dir()?;
        self.prune()?;
        Ok(final_path)
    }

    /// Sync the directory entry so that the rename is durable.
    fn sync_dir(&self) -> VimoResult<()> {
        match fs::File::open(&self.dir) {
            Ok(dir) => {
                let _ = dir.sync_all();
                Ok(())
            }
            Err(e) => Err(VimoError::detailed(
                vimo_foundation::ErrorKind::Io,
                "autosave",
                format!("open dir: {e}"),
            )),
        }
    }

    fn next_sequence(&self) -> u64 {
        // The sequence keeps names unique even when two snapshots land in the
        // same millisecond.
        let existing = self.list_snapshots().unwrap_or_default().len() as u64;
        existing.max(self.sequence) + 1
    }

    /// List the snapshots in the store, oldest first.
    pub fn list_snapshots(&self) -> VimoResult<Vec<SnapshotInfo>> {
        let mut out = Vec::new();
        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => {
                return Err(VimoError::detailed(
                    vimo_foundation::ErrorKind::Io,
                    "autosave",
                    format!("read dir: {e}"),
                ))
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.ends_with(SNAPSHOT_EXTENSION) || name.ends_with(".tmp") {
                continue;
            }
            let Some(stamp) = parse_timestamp(name) else {
                continue;
            };
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(SnapshotInfo {
                path,
                created_unix_ms: stamp,
                size,
            });
        }
        out.sort_by_key(|s| s.created_unix_ms);
        Ok(out)
    }

    /// The newest snapshot, if any.
    pub fn latest(&self) -> VimoResult<Option<SnapshotInfo>> {
        Ok(self.list_snapshots()?.pop())
    }

    /// Delete all but the newest `max_snapshots` snapshots.
    ///
    /// Returns the number of snapshots removed.
    pub fn prune(&self) -> VimoResult<usize> {
        let mut snaps = self.list_snapshots()?;
        if snaps.len() <= self.max_snapshots {
            return Ok(0);
        }
        let excess = snaps.len() - self.max_snapshots;
        let to_remove: Vec<PathBuf> = snaps.drain(0..excess).map(|s| s.path).collect();
        let mut removed = 0;
        for p in to_remove {
            if fs::remove_file(&p).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Load the newest snapshot.
    pub fn load_latest(
        &self,
        registry: &EffectRegistry,
    ) -> VimoResult<Option<(Project, LoadReport)>> {
        let Some(info) = self.latest()? else {
            return Ok(None);
        };
        let bytes = fs::read(&info.path).map_err(|e| {
            VimoError::detailed(
                vimo_foundation::ErrorKind::Io,
                "autosave",
                format!("read {}: {e}", info.path.display()),
            )
        })?;
        Ok(Some(codec::read(&bytes, registry)?))
    }
}

/// Extract the millisecond timestamp encoded in a snapshot file name.
fn parse_timestamp(name: &str) -> Option<u64> {
    let rest = name.strip_prefix("snapshot-")?;
    let (stamp, _) = rest.split_once('-')?;
    stamp.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Asset, Track, TrackKind};
    use vimo_foundation::time::Time;

    fn registry() -> EffectRegistry {
        EffectRegistry::new()
    }

    fn project() -> Project {
        let mut p = Project::new();
        p.assets.push(Asset::video("src", Time::from_secs(5)));
        p.tracks.push(Track::new("V1", TrackKind::Video));
        p
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("vimo-autosave-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn writes_and_reads_snapshots() {
        let dir = temp_dir("basic");
        let store = AutosaveStore::new(&dir);
        let reg = registry();
        let p = project();
        store.write_snapshot(&p, &reg).unwrap();
        let loaded = store.load_latest(&reg).unwrap().unwrap();
        assert_eq!(loaded.0.tracks.len(), 1);
        assert_eq!(loaded.0.assets.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_keeps_only_the_newest() {
        let dir = temp_dir("prune");
        let store = AutosaveStore::new(&dir).with_max_snapshots(2);
        let reg = registry();
        let p = project();
        for _ in 0..4 {
            store.write_snapshot(&p, &reg).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let snaps = store.list_snapshots().unwrap();
        assert!(snaps.len() <= 2, "kept {} snapshots", snaps.len());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_temp_files_are_left_behind() {
        let dir = temp_dir("tmp");
        let store = AutosaveStore::new(&dir);
        let reg = registry();
        store.write_snapshot(&project(), &reg).unwrap();
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().map(|x| x == "tmp").unwrap_or(false))
            .collect();
        assert!(leftovers.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_directory_is_not_an_error() {
        let dir = temp_dir("missing");
        let store = AutosaveStore::new(dir.join("nested"));
        assert!(store.list_snapshots().unwrap().is_empty());
        assert!(store.latest().unwrap().is_none());
    }
}
