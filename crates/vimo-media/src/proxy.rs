//! Proxy generation.
//!
//! Editing 4K footage on a low-end phone is painful. The engine therefore
//! generates small, low-bitrate **proxies** in the background and edits
//! against those, switching to the full-resolution media only for the final
//! export (and for effects that need full resolution).
//!
//! The scheduler is deliberately simple: a priority queue of jobs, a bounded
//! number of workers, and cancellation.

use std::collections::VecDeque;

use vimo_foundation::error::{invalid_argument, VimoResult};
use vimo_foundation::time::Time;

use crate::codec::CodecId;

/// How a proxy should be encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxyPolicy {
    /// Target width in pixels.
    pub width: u32,
    /// Target height in pixels.
    pub height: u32,
    /// Target bitrate in bits per second.
    pub bitrate: u32,
    /// The codec to use.
    pub codec: CodecId,
}

impl Default for ProxyPolicy {
    fn default() -> Self {
        // 960x540 at 2 Mbps is a good trade-off for smooth scrubbing on a
        // low-end device.
        ProxyPolicy {
            width: 960,
            height: 540,
            bitrate: 2_000_000,
            codec: CodecId::H264,
        }
    }
}

impl ProxyPolicy {
    /// A policy that keeps the aspect ratio of `width` x `height`.
    #[must_use]
    pub fn scaled_from(width: u32, height: u32, max_width: u32) -> Self {
        if width == 0 || height == 0 {
            return ProxyPolicy::default();
        }
        let scale = (max_width as f64 / width as f64).min(1.0);
        let w = ((width as f64 * scale).round() as u32).max(2) & !1;
        let h = ((height as f64 * scale).round() as u32).max(2) & !1;
        ProxyPolicy { width: w, height: h, ..ProxyPolicy::default() }
    }

    /// Whether the policy is actually smaller than the source.
    #[must_use]
    pub fn is_reduction(&self, source_width: u32, source_height: u32) -> bool {
        self.width < source_width || self.height < source_height
    }
}

/// The state of a proxy job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyState {
    /// Queued, not started.
    Queued,
    /// Running.
    Running,
    /// Finished successfully.
    Complete,
    /// Failed.
    Failed,
    /// Cancelled.
    Cancelled,
}

/// A proxy generation job.
#[derive(Debug, Clone, PartialEq)]
pub struct ProxyJob {
    /// The source being proxied.
    pub source_id: String,
    /// Where the proxy is written.
    pub output_path: String,
    /// The encoding policy.
    pub policy: ProxyPolicy,
    /// Higher runs first.
    pub priority: u32,
    /// The duration of the source.
    pub duration: Time,
    /// The current state.
    pub state: ProxyState,
    /// Progress in `0..=1`.
    pub progress: f32,
}

impl ProxyJob {
    /// A new job.
    #[must_use]
    pub fn new(source_id: impl Into<String>, policy: ProxyPolicy) -> Self {
        ProxyJob {
            source_id: source_id.into(),
            output_path: String::new(),
            policy,
            priority: 0,
            duration: Time::ZERO,
            state: ProxyState::Queued,
            progress: 0.0,
        }
    }
}

/// An update emitted by a running job.
#[derive(Debug, Clone, PartialEq)]
pub enum ProxyUpdate {
    /// The job made progress.
    Progress {
        /// The source being proxied.
        source_id: String,
        /// Progress in `0..=1`.
        progress: f32,
    },
    /// The job finished.
    Complete {
        /// The source that was proxied.
        source_id: String,
        /// The path of the proxy.
        path: String,
    },
    /// The job failed.
    Failed {
        /// The source that was being proxied.
        source_id: String,
        /// The reason.
        reason: String,
    },
}

/// Runs proxy jobs.
pub trait ProxyRunner: Send {
    /// Start a job. Returns `false` when the runner is saturated.
    fn start(&mut self, job: &ProxyJob) -> VimoResult<bool>;
    /// Poll for updates.
    fn poll(&mut self) -> Vec<ProxyUpdate>;
    /// Cancel a job.
    fn cancel(&mut self, source_id: &str);
}

/// A scheduler that keeps the queue ordered by priority.
#[derive(Debug, Default)]
pub struct ProxyScheduler {
    queue: Vec<ProxyJob>,
    running: usize,
    max_running: usize,
    updates: VecDeque<ProxyUpdate>,
}

impl ProxyScheduler {
    /// A scheduler that runs at most `max_running` jobs at a time.
    #[must_use]
    pub fn new(max_running: usize) -> Self {
        ProxyScheduler {
            queue: Vec::new(),
            running: 0,
            max_running: max_running.max(1),
            updates: VecDeque::new(),
        }
    }

    /// Queue a job.
    pub fn enqueue(&mut self, job: ProxyJob) {
        if job.policy.bitrate == 0 {
            return;
        }
        // Insert sorted by descending priority (stable within a priority).
        let pos = self
            .queue
            .iter()
            .position(|j| j.priority < job.priority)
            .unwrap_or(self.queue.len());
        self.queue.insert(pos, job);
    }

    /// The number of queued jobs.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// The number of running jobs.
    #[must_use]
    pub fn running(&self) -> usize {
        self.running
    }

    /// Remove a queued job.
    pub fn cancel(&mut self, source_id: &str) -> bool {
        if let Some(pos) = self.queue.iter().position(|j| j.source_id == source_id) {
            self.queue.remove(pos);
            return true;
        }
        false
    }

    /// Promote the clips that are currently visible to the front of the queue.
    ///
    /// This is what keeps scrubbing responsive: the media under the playhead is
    /// proxied first.
    pub fn prioritize(&mut self, visible_sources: &[String]) {
        for source in visible_sources {
            for job in &mut self.queue {
                if &job.source_id == source {
                    job.priority = job.priority.max(1);
                }
            }
        }
        self.queue.sort_by_key(|j| std::cmp::Reverse(j.priority));
    }

    /// Start as many queued jobs as the concurrency limit allows.
    pub fn start_ready(&mut self, runner: &mut dyn ProxyRunner) -> VimoResult<()> {
        while self.running < self.max_running {
            let Some(job) = self.queue.first().cloned() else {
                break;
            };
            let started = runner.start(&job)?;
            if !started {
                break;
            }
            self.queue.remove(0);
            self.running += 1;
        }
        Ok(())
    }

    /// Collect the runner's updates and adjust the running count.
    pub fn collect(&mut self, runner: &mut dyn ProxyRunner) {
        for update in runner.poll() {
            match &update {
                ProxyUpdate::Complete { .. } | ProxyUpdate::Failed { .. } => {
                    self.running = self.running.saturating_sub(1);
                }
                _ => {}
            }
            self.updates.push_back(update);
        }
    }

    /// Take the pending updates.
    pub fn take_updates(&mut self) -> Vec<ProxyUpdate> {
        self.updates.drain(..).collect()
    }
}

/// A proxy runner that does nothing, for platforms without a proxy pipeline.
#[derive(Debug, Default)]
pub struct NullProxyRunner;

impl ProxyRunner for NullProxyRunner {
    fn start(&mut self, _job: &ProxyJob) -> VimoResult<bool> {
        Ok(false)
    }
    fn poll(&mut self) -> Vec<ProxyUpdate> {
        Vec::new()
    }
    fn cancel(&mut self, _source_id: &str) {}
}

/// Validate a job's policy.
pub fn validate_job(job: &ProxyJob) -> VimoResult<()> {
    if job.source_id.is_empty() {
        return Err(invalid_argument("proxy: missing source id"));
    }
    if job.policy.width == 0 || job.policy.height == 0 {
        return Err(invalid_argument("proxy: zero dimensions"));
    }
    if job.policy.bitrate == 0 {
        return Err(invalid_argument("proxy: zero bitrate"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_runs_the_highest_priority_first() {
        let mut s = ProxyScheduler::new(1);
        let mut low = ProxyJob::new("low", ProxyPolicy::default());
        low.priority = 0;
        let mut high = ProxyJob::new("high", ProxyPolicy::default());
        high.priority = 10;
        s.enqueue(low);
        s.enqueue(high);
        assert_eq!(s.queued(), 2);
        s.prioritize(&["high".to_string()]);
        assert_eq!(s.queue[0].source_id, "high");
    }

    #[test]
    fn policy_keeps_the_aspect_ratio() {
        let p = ProxyPolicy::scaled_from(3840, 2160, 960);
        assert_eq!(p.width, 960);
        assert_eq!(p.height, 540);
        assert!(p.is_reduction(3840, 2160));
    }

    #[test]
    fn invalid_jobs_are_rejected() {
        let mut job = ProxyJob::new("", ProxyPolicy::default());
        assert!(validate_job(&job).is_err());
        job.source_id = "x".into();
        job.policy.bitrate = 0;
        assert!(validate_job(&job).is_err());
    }
}
