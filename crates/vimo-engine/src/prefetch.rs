//! Frame prefetching.
//!
//! Decoding a frame takes far longer than drawing one. To stay smooth the
//! engine keeps a few frames decoded ahead of the playhead. The prefetcher
//! decides *what* to decode; the platform's worker threads do the decoding.
//!
//! The request queue is ordered by distance from the playhead, so the frame
//! that is needed next is always first.

use vimo_foundation::time::{Time, TimeRange, Timebase};

/// A request to decode one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefetchRequest {
    /// The interned source id.
    pub source: u64,
    /// The position in the source media.
    pub time: Time,
    /// Lower values are requested first.
    pub priority: u32,
}

/// A source that is currently visible on the timeline.
#[derive(Debug, Clone, Copy)]
pub struct ActiveSource {
    /// The interned source id.
    pub source: u64,
    /// The range of the source that is visible.
    pub visible: TimeRange,
    /// The source's timebase.
    pub timebase: Timebase,
    /// The playback rate applied to the source.
    pub rate: f32,
}

/// Decides which frames to decode ahead of the playhead.
#[derive(Debug, Clone)]
pub struct Prefetcher {
    /// How many frames to decode ahead.
    pub prefetch_frames: u32,
    /// The maximum number of outstanding requests.
    pub max_requests: usize,
    /// The pending requests, most important first.
    queue: Vec<PrefetchRequest>,
    /// The requests that have been handed out but not completed.
    in_flight: Vec<PrefetchRequest>,
    /// The requests that have been satisfied.
    completed: u64,
    /// The requests that could not be satisfied.
    dropped: u64,
}

impl Default for Prefetcher {
    fn default() -> Self {
        Prefetcher::new(4)
    }
}

impl Prefetcher {
    /// A prefetcher that looks `prefetch_frames` ahead.
    #[must_use]
    pub fn new(prefetch_frames: u32) -> Self {
        Prefetcher {
            prefetch_frames,
            max_requests: 32,
            queue: Vec::new(),
            in_flight: Vec::new(),
            completed: 0,
            dropped: 0,
        }
    }

    /// The number of pending requests.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// The number of in-flight requests.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    /// The number of completed requests.
    #[must_use]
    pub fn completed(&self) -> u64 {
        self.completed
    }

    /// The number of dropped requests.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Rebuild the request queue for the given playhead position.
    ///
    /// Called once per frame. It is cheap: the queue is rebuilt from scratch,
    /// but the number of requests is tiny (a handful).
    pub fn update(&mut self, playhead: Time, sources: &[ActiveSource]) {
        self.queue.clear();
        if self.prefetch_frames == 0 {
            return;
        }
        for src in sources {
            if src.visible.duration() <= Time::ZERO {
                continue;
            }
            let fps = src.timebase.fps().as_f64().max(1.0);
            let rate = if src.rate.abs() < f32::EPSILON {
                1.0
            } else {
                src.rate
            };
            for i in 0..=self.prefetch_frames {
                let offset = Time::from_secs_f64(i as f64 / fps);
                let timeline_pos = playhead + offset;
                if !src.visible.contains(timeline_pos) {
                    continue;
                }
                // Where in the source media that timeline position lives.
                let into = timeline_pos - src.visible.start;
                let source_time =
                    src.visible.start + Time::from_secs_f64(into.as_secs_f64() * rate as f64);
                self.queue.push(PrefetchRequest {
                    source: src.source,
                    time: source_time,
                    priority: i,
                });
            }
        }
        self.queue.sort_by_key(|r| r.priority);
        if self.queue.len() > self.max_requests {
            self.dropped += (self.queue.len() - self.max_requests) as u64;
            self.queue.truncate(self.max_requests);
        }
    }

    /// Take the next request to work on.
    pub fn next_request(&mut self) -> Option<PrefetchRequest> {
        let request = self.queue.first().copied()?;
        self.queue.remove(0);
        self.in_flight.push(request);
        Some(request)
    }

    /// Mark a request as finished.
    pub fn complete(&mut self, request: PrefetchRequest) {
        self.in_flight.retain(|r| *r != request);
        self.completed += 1;
    }

    /// Drop every pending and in-flight request.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.in_flight.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_follow_the_playhead() {
        let mut p = Prefetcher::new(3);
        let src = ActiveSource {
            source: 1,
            visible: TimeRange::new(Time::ZERO, Time::from_secs(10)),
            timebase: Timebase::from_fps(30, 1),
            rate: 1.0,
        };
        p.update(Time::from_secs(1), &[src]);
        assert_eq!(p.pending(), 4);
        let first = p.next_request().unwrap();
        assert_eq!(first.time, Time::from_secs(1));
        assert_eq!(first.priority, 0);
    }

    #[test]
    fn requests_are_priority_ordered() {
        let mut p = Prefetcher::new(3);
        let src = ActiveSource {
            source: 1,
            visible: TimeRange::new(Time::ZERO, Time::from_secs(10)),
            timebase: Timebase::from_fps(30, 1),
            rate: 1.0,
        };
        p.update(Time::from_secs(1), &[src]);
        let mut priorities = Vec::new();
        while let Some(r) = p.next_request() {
            priorities.push(r.priority);
        }
        assert_eq!(priorities, vec![0, 1, 2, 3]);
    }

    #[test]
    fn requests_outside_the_visible_range_are_skipped() {
        let mut p = Prefetcher::new(5);
        let src = ActiveSource {
            source: 1,
            visible: TimeRange::new(Time::from_secs(4), Time::from_secs(5)),
            timebase: Timebase::from_fps(30, 1),
            rate: 1.0,
        };
        p.update(Time::from_secs(4), &[src]);
        // Only the frames that fall inside the one-second window.
        assert!(p.pending() <= 31);
        assert!(p.pending() > 0);
    }
}
