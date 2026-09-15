//! Where a second's worth of frames spent their time.
//!
//! Printed once a second on stderr in a debug build, and in a release one with
//! `RBX_STUDIO_STATS=1`. Both threads write to it — the uploads are timed on the
//! UI thread, the rest on the render thread — which is what the atomics are for.
//!
//! The line carries both threads' rates on purpose: the render thread's count
//! is what it drew, `shown` is what the UI thread actually collected and put
//! on screen. They part ways only when the UI thread was too busy to keep up —
//! frames it overtakes are dropped unseen (see `WorkspaceView::advance`) — and
//! that is invisible to the render thread's own counter, which keeps reading a
//! full frame rate through a view that visibly stutters.

use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const STATS_VARIABLE: &str = "RBX_STUDIO_STATS";
const REPORT_EVERY: Duration = Duration::from_secs(1);

#[derive(Debug, Default)]
pub(super) struct Stats {
    frames: AtomicU64,
    render: AtomicU64,
    readback: AtomicU64,
    uploads: AtomicU64,
    upload: AtomicU64,
    /// The last upload on its own, not a total: the render thread reads it to
    /// charge the whole round trip to the frame rate manager, and a running
    /// average over a window it never resets would lag a level change.
    latest_upload: AtomicU64,
    /// Nanoseconds since the last line was printed, against a start the whole
    /// process shares so both threads can read the same clock without one.
    opened: AtomicU64,
}

impl Stats {
    /// One frame queued and one collected, as [`rbx_viewer::Rendered`] timed them.
    pub(super) fn drew(&self, render: Duration, readback: Duration) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        add(&self.render, render);
        add(&self.readback, readback);
    }

    /// One frame turned into the image GPUI paints. The atlas upload itself
    /// happens later, inside GPUI's own frame, and cannot be timed from here:
    /// this is the UI thread's share of a frame, which is the actionable half.
    pub(super) fn uploaded(&self, took: Duration) {
        self.uploads.fetch_add(1, Ordering::Relaxed);
        add(&self.upload, took);
        self.latest_upload.store(nanos(took), Ordering::Relaxed);
    }

    /// What the UI thread last spent turning a frame into an image.
    ///
    /// One frame behind by construction — the upload of a frame happens after
    /// the render thread has asked for the next one — which is close enough for
    /// a decision taken over a second of them.
    pub(super) fn last_upload(&self) -> Duration {
        Duration::from_nanos(self.latest_upload.load(Ordering::Relaxed))
    }

    /// Prints the last second, if a second has passed and anything was drawn in
    /// it. `interval` is the budget a frame had, for the line to be read against,
    /// and `level` the graphics quality those frames were drawn at.
    pub(super) fn report(&self, interval: Duration, level: u8) {
        let elapsed = self.since_last_report();
        if elapsed < REPORT_EVERY {
            return;
        }

        let frames = self.frames.swap(0, Ordering::Relaxed);
        let render = self.render.swap(0, Ordering::Relaxed);
        let readback = self.readback.swap(0, Ordering::Relaxed);
        let uploads = self.uploads.swap(0, Ordering::Relaxed);
        let upload = self.upload.swap(0, Ordering::Relaxed);
        if frames == 0 || !enabled() {
            return;
        }

        eprintln!(
            "rbxstudio: {}",
            line(
                Counted { frames, uploads },
                Totals {
                    render,
                    readback,
                    upload
                },
                elapsed,
                interval,
                level,
            )
        );
    }

    /// How long since the last report, resetting the window when a second is up.
    fn since_last_report(&self) -> Duration {
        let now = nanos(started().elapsed());
        let opened = self.opened.load(Ordering::Relaxed);
        let elapsed = Duration::from_nanos(now.saturating_sub(opened));
        if elapsed >= REPORT_EVERY {
            self.opened.store(now, Ordering::Relaxed);
        }

        elapsed
    }
}

struct Counted {
    frames: u64,
    uploads: u64,
}

/// Nanoseconds summed over the reporting window.
struct Totals {
    render: u64,
    readback: u64,
    upload: u64,
}

/// The whole process's clock, so both threads measure against the same zero.
fn started() -> Instant {
    use std::sync::OnceLock;

    static STARTED: OnceLock<Instant> = OnceLock::new();
    *STARTED.get_or_init(Instant::now)
}

pub(super) fn enabled() -> bool {
    cfg!(debug_assertions) || env::var(STATS_VARIABLE).is_ok_and(|value| value == "1")
}

fn add(total: &AtomicU64, took: Duration) {
    total.fetch_add(nanos(took), Ordering::Relaxed);
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// The one line a second, averages per frame and the rate they came at.
fn line(
    counted: Counted,
    totals: Totals,
    elapsed: Duration,
    interval: Duration,
    level: u8,
) -> String {
    let fps = counted.frames as f32 / elapsed.as_secs_f32();
    let shown = counted.uploads as f32 / elapsed.as_secs_f32();
    let cap = 1.0 / interval.as_secs_f32();

    format!(
        "{fps:.0}/{cap:.0} fps · shown {shown:.0}/s · Q{level} · render {} · readback {} · upload {}",
        average(totals.render, counted.frames),
        average(totals.readback, counted.frames),
        average(totals.upload, counted.uploads),
    )
}

fn average(total: u64, count: u64) -> String {
    if count == 0 {
        return "n/a".to_string();
    }

    format!("{:.1} ms", total as f64 / count as f64 / 1e6)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{average, line, Counted, Totals};

    #[test]
    fn the_line_reads_as_a_rate_against_its_cap_a_level_and_three_averages() {
        let reported = line(
            Counted {
                frames: 74,
                uploads: 40,
            },
            Totals {
                render: 74 * 800_000,
                readback: 74 * 9_500_000,
                // Averaged over the frames actually shown, not the ones drawn:
                // an upload only happens for a frame that reached the window.
                upload: 40 * 1_200_000,
            },
            Duration::from_secs(1),
            Duration::from_micros(13_333),
            17,
        );

        assert_eq!(
            reported,
            "74/75 fps · shown 40/s · Q17 · render 0.8 ms · readback 9.5 ms · upload 1.2 ms"
        );
    }

    // A window that is not exactly a second long still reports a per-second rate.
    #[test]
    fn the_rate_is_per_second_however_long_the_window_was() {
        let reported = line(
            Counted {
                frames: 30,
                uploads: 30,
            },
            Totals {
                render: 0,
                readback: 0,
                upload: 0,
            },
            Duration::from_millis(1500),
            Duration::from_micros(16_667),
            21,
        );

        assert!(reported.starts_with("20/60 fps"), "{reported}");
    }

    #[test]
    fn nothing_counted_averages_to_nothing_rather_than_to_zero() {
        assert_eq!(average(0, 0), "n/a");
        assert_eq!(average(5_000_000, 2), "2.5 ms");
    }
}
