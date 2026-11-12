//! # vimo-project
//!
//! The Vimo project model: the pure data that describes an edit, the command
//! system that mutates it, and the versioned file format that persists it.
//!
//! The crate is deliberately free of any graphics, audio or platform code, so
//! it can be unit tested on a desktop and fuzzed on a server.
//!
//! ```
//! use vimo_project::{Command, History, Project};
//! use vimo_project::model::{Asset, Clip, Track, TrackKind};
//! use vimo_foundation::time::{Time, TimeRange};
//!
//! let mut project = Project::new();
//! project.assets.push(Asset::video("clip.mp4", Time::from_secs(10)));
//! project.tracks.push(Track::new("V1", TrackKind::Video));
//!
//! let mut history = History::new();
//! let clip = Clip::new(0, TimeRange::new(Time::ZERO, Time::from_secs(2)), Time::ZERO);
//! history.execute(&mut project, Command::add_clip(0, 0, clip)).unwrap();
//!
//! assert_eq!(project.tracks[0].clips.len(), 1);
//! history.undo(&mut project).unwrap();
//! assert_eq!(project.tracks[0].clips.len(), 0);
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod anim;
pub mod autosave;
pub mod codec;
pub mod command;
pub mod history;
pub mod migrate;
pub mod model;
pub mod project;
pub mod snapshot;

pub use anim::{AnimTrack, Interpolation, Keyframe};
pub use autosave::{AutosaveStore, SnapshotInfo};
pub use codec::{LoadReport, CURRENT_VERSION};
pub use command::Command;
pub use history::History;
pub use model::{
    Asset, AssetId, AssetKind, BlendMode, Clip, ClipId, EffectId, EffectNode, EffectOwner,
    ProjectMeta, Track, TrackId, TrackKind, Transform, Transition, TransitionId, TransitionKind,
};
pub use project::Project;
pub use snapshot::snapshot;
