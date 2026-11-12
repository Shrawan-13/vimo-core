//! Undo / redo history.
//!
//! The history stores **inverse** commands. Executing a command applies it to
//! the project and pushes the returned inverse onto the undo stack; undoing
//! pops that inverse, applies it, and pushes the command it returns onto the
//! redo stack. Because both stacks hold commands that undo each other, redo is
//! exact — not an approximation reconstructed from scratch.

use vimo_foundation::error::VimoResult;

use crate::command::Command;
use crate::project::Project;

/// Default maximum number of undo steps.
pub const DEFAULT_MAX_DEPTH: usize = 256;

/// A command history.
#[derive(Debug, Clone)]
pub struct History {
    undo: Vec<Command>,
    redo: Vec<Command>,
    max_depth: usize,
}

impl Default for History {
    fn default() -> Self {
        History::with_max_depth(DEFAULT_MAX_DEPTH)
    }
}

impl History {
    /// A history with the default depth limit.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A history that keeps at most `max_depth` undo steps.
    #[must_use]
    pub fn with_max_depth(max_depth: usize) -> Self {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            max_depth: max_depth.max(1),
        }
    }

    /// Execute a command and record it.
    pub fn execute(&mut self, project: &mut Project, command: Command) -> VimoResult<()> {
        let inverse = command.apply(project)?;
        self.undo.push(inverse);
        self.redo.clear();
        if self.undo.len() > self.max_depth {
            // Drop the oldest entries; they can no longer be undone.
            let excess = self.undo.len() - self.max_depth;
            self.undo.drain(0..excess);
        }
        Ok(())
    }

    /// Undo the most recent command.
    ///
    /// Returns `false` when there is nothing to undo.
    pub fn undo(&mut self, project: &mut Project) -> VimoResult<bool> {
        let Some(inverse) = self.undo.pop() else {
            return Ok(false);
        };
        let redo_command = inverse.apply(project)?;
        self.redo.push(redo_command);
        Ok(true)
    }

    /// Redo the most recently undone command.
    ///
    /// Returns `false` when there is nothing to redo.
    pub fn redo(&mut self, project: &mut Project) -> VimoResult<bool> {
        let Some(command) = self.redo.pop() else {
            return Ok(false);
        };
        let inverse = command.apply(project)?;
        self.undo.push(inverse);
        Ok(true)
    }

    /// Whether an undo step is available.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether a redo step is available.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Label of the command that would be undone.
    #[must_use]
    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.last().map(Command::label)
    }

    /// Label of the command that would be redone.
    #[must_use]
    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(Command::label)
    }

    /// Number of undo steps.
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Number of redo steps.
    #[must_use]
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Drop all history.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Asset, Clip, Track, TrackKind};
    use vimo_foundation::time::{Time, TimeRange};

    fn project() -> Project {
        let mut p = Project::new();
        p.assets.push(Asset::video("src", Time::from_secs(30)));
        p.tracks.push(Track::new("V1", TrackKind::Video));
        p
    }

    fn clip() -> Clip {
        Clip::new(
            0,
            TimeRange::new(Time::ZERO, Time::from_secs(2)),
            Time::ZERO,
        )
    }

    #[test]
    fn undo_and_redo_roundtrip() {
        let mut p = project();
        let mut h = History::new();
        h.execute(&mut p, Command::add_clip(0, 0, clip())).unwrap();
        assert_eq!(p.tracks[0].clips.len(), 1);
        assert!(h.can_undo());

        assert!(h.undo(&mut p).unwrap());
        assert_eq!(p.tracks[0].clips.len(), 0);
        assert!(h.can_redo());

        assert!(h.redo(&mut p).unwrap());
        assert_eq!(p.tracks[0].clips.len(), 1);
    }

    #[test]
    fn new_command_clears_the_redo_stack() {
        let mut p = project();
        let mut h = History::new();
        h.execute(&mut p, Command::add_clip(0, 0, clip())).unwrap();
        h.undo(&mut p).unwrap();
        assert!(h.can_redo());
        h.execute(&mut p, Command::add_clip(0, 0, clip())).unwrap();
        assert!(!h.can_redo());
    }

    #[test]
    fn undo_on_empty_history_is_a_noop() {
        let mut p = project();
        let mut h = History::new();
        assert!(!h.undo(&mut p).unwrap());
        assert!(!h.redo(&mut p).unwrap());
    }

    #[test]
    fn depth_is_capped() {
        let mut p = project();
        let mut h = History::with_max_depth(3);
        for _ in 0..10 {
            h.execute(&mut p, Command::add_clip(0, 0, clip())).unwrap();
        }
        assert_eq!(h.undo_depth(), 3);
    }

    #[test]
    fn redo_restores_the_exact_state() {
        let mut p = project();
        let mut h = History::new();
        h.execute(&mut p, Command::add_clip(0, 0, clip())).unwrap();
        let before = p.clips[0].clone();
        h.execute(&mut p, Command::edit_clip(0, before, |c| c.opacity = 0.3))
            .unwrap();
        h.undo(&mut p).unwrap();
        h.undo(&mut p).unwrap();
        assert_eq!(p.tracks[0].clips.len(), 0);
        h.redo(&mut p).unwrap();
        h.redo(&mut p).unwrap();
        assert_eq!(p.tracks[0].clips.len(), 1);
        assert!((p.clips[0].opacity - 0.3).abs() < 1e-6);
    }
}
