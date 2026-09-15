//! Which graphics quality level the viewport draws at.
//!
//! Two modes, both spelled by [`QualityLevel`]: a level the user pinned, drawn
//! until they pin another, or `Automatic`, where [`FrameRateManager`] is fed the
//! cost of every frame and moves the level to hold the display's refresh rate.
//! Lives on the render thread, which is the only one allowed to touch the
//! viewer.

use std::time::{Duration, Instant};

use rbx_viewer::{FrameRateManager, Headless, QualityLevel};

use super::stats;

/// How many frames after a switch the manager is not fed.
///
/// A switch costs well under a frame (see [`Headless::set_quality`]), but the
/// frame it lands in still carries it, and the one after it re-primes the driver's
/// caches. Feeding those two to the manager would read as a slow patch and walk
/// the level straight back down, so they are skipped — a sixteenth of a second at
/// worst, against a level that would otherwise oscillate.
const SETTLE_FRAMES: u8 = 3;

/// The mode, and the manager that moves the level while the mode is `Automatic`.
pub(super) struct Quality {
    /// Rebuilt from scratch whenever `Automatic` is re-entered: a manager must
    /// never decide on frames drawn at a level the user had pinned.
    manager: Option<FrameRateManager>,
    target_hz: f32,
    /// Frames left to ignore after a switch, so the manager never measures the
    /// switch itself.
    settling: u8,
}

impl Quality {
    /// `budget` is what one frame is paced to — the display's refresh interval,
    /// which is therefore the frame rate the manager aims to hold.
    pub(super) fn new(mode: QualityLevel, budget: Duration, viewer: &mut Headless) -> Self {
        let mut quality = Quality {
            manager: None,
            target_hz: target_hz(budget),
            settling: 0,
        };
        quality.set(mode, viewer);
        quality
    }

    /// Switches mode, applying the level it implies at once. `Automatic` starts
    /// the manager over: the level it opens on is the top one, which is the only
    /// honest probe of a machine nothing is yet known about.
    pub(super) fn set(&mut self, mode: QualityLevel, viewer: &mut Headless) {
        self.settling = SETTLE_FRAMES;
        match mode {
            QualityLevel::Automatic => {
                let manager = FrameRateManager::new(self.target_hz);
                viewer.set_quality(QualityLevel::Level(manager.level()));
                self.manager = Some(manager);
            }
            pinned => {
                self.manager = None;
                viewer.set_quality(pinned);
            }
        }
    }

    /// One rendered frame and what it cost, which in `Automatic` may switch the
    /// level before the next one is drawn.
    ///
    /// Called between frames on purpose: a switch rebuilds bind groups the frame
    /// in flight would still be reading.
    pub(super) fn record(&mut self, cost: Duration, viewer: &mut Headless) {
        if self.settling > 0 {
            self.settling -= 1;
            return;
        }

        let Some(manager) = &mut self.manager else {
            return;
        };
        manager.record(cost);
        if let Some(level) = manager.changed() {
            self.apply(level, viewer);
        }
    }

    fn apply(&mut self, level: u8, viewer: &mut Headless) {
        self.settling = SETTLE_FRAMES;
        let started = Instant::now();
        viewer.set_quality(QualityLevel::Level(level));
        if stats::enabled() {
            // The trajectory is the whole point of the mode: without it a level
            // that walked down for a reason looks like a renderer bug. The cost is
            // logged with it because it is the one number that says whether the
            // switch is still cheap enough to make every few frames.
            eprintln!(
                "rbxstudio: quality → Q{level} (switched in {:.2} ms)",
                started.elapsed().as_secs_f64() * 1e3
            );
        }
    }
}

/// The frame rate a budget stands for. Clamped away from zero by the pacing that
/// produced it, so the only guard needed is against a budget of nothing at all.
fn target_hz(budget: Duration) -> f32 {
    if budget.is_zero() {
        return f32::MAX;
    }

    1.0 / budget.as_secs_f32()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::target_hz;

    #[test]
    fn a_frame_budget_is_the_rate_it_holds() {
        assert!((target_hz(Duration::from_micros(16_667)) - 60.0).abs() < 0.01);
        assert!((target_hz(Duration::from_micros(6_944)) - 144.0).abs() < 0.1);
    }

    #[test]
    fn a_budget_of_nothing_is_not_a_division_by_zero() {
        assert!(target_hz(Duration::ZERO).is_finite());
    }
}
