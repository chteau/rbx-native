//! A per-second frame rate for the title bar.
//!
//! Fed the same wall-clock timestamp `Viewer::redraw` already takes every
//! frame for `dt` — not a second stopwatch: this just counts how many of
//! those already-happening redraws land inside a rolling one-second window,
//! the same technique `rbxstudio`'s own `workspace_view::stats` uses to drive
//! its Viewport dock's readout, minus the render/readback/upload split that has
//! no equivalent in a single windowed loop with no render thread of its own.

use std::time::{Duration, Instant};

/// How long a window's frame count is averaged over before the rate refreshes.
const WINDOW: Duration = Duration::from_secs(1);

/// Counts frames over a rolling one-second window and keeps the last complete
/// window's average.
pub(super) struct FrameRate {
    window_start: Instant,
    frames: u32,
    latest: Option<f32>,
}

impl FrameRate {
    pub(super) fn new(now: Instant) -> Self {
        FrameRate {
            window_start: now,
            frames: 0,
            latest: None,
        }
    }

    /// One frame drawn. Returns whether this call closed a one-second window
    /// and refreshed [`FrameRate::latest`] — the title bar only needs to
    /// redraw itself then, not on every single frame.
    pub(super) fn record(&mut self, now: Instant) -> bool {
        self.frames += 1;
        let elapsed = now.duration_since(self.window_start);
        if elapsed < WINDOW {
            return false;
        }

        self.latest = Some(self.frames as f32 / elapsed.as_secs_f32());
        self.frames = 0;
        self.window_start = now;
        true
    }

    /// The last full window's frame rate, `None` until one has completed.
    pub(super) fn latest(&self) -> Option<f32> {
        self.latest
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::FrameRate;

    #[test]
    fn nothing_is_reported_before_a_full_window_closes() {
        let start = Instant::now();
        let mut rate = FrameRate::new(start);
        assert!(!rate.record(start + Duration::from_millis(500)));
        assert_eq!(rate.latest(), None);
    }

    #[test]
    fn a_full_window_reports_frames_over_its_actual_length() {
        let start = Instant::now();
        let mut rate = FrameRate::new(start);
        for i in 0..29 {
            assert!(!rate.record(start + Duration::from_millis(i * 10)));
        }
        assert!(rate.record(start + Duration::from_secs(1)));
        assert_eq!(rate.latest(), Some(30.0));
    }

    #[test]
    fn a_closed_window_resets_and_starts_counting_again() {
        let start = Instant::now();
        let mut rate = FrameRate::new(start);
        let closed = start + Duration::from_secs(1);
        assert!(rate.record(closed));
        assert_eq!(rate.latest(), Some(1.0));

        assert!(!rate.record(closed + Duration::from_millis(500)));
        assert_eq!(
            rate.latest(),
            Some(1.0),
            "still the last full window's rate, not the partial one being counted"
        );

        assert!(rate.record(closed + Duration::from_secs(1)));
        assert_eq!(rate.latest(), Some(2.0));
    }
}
