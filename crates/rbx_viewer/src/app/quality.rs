//! `--quality auto` in the windowed viewer.
//!
//! The window presents through the swapchain, so its frames are paced by the
//! display whether they are cheap or not: a loop keeping up measures exactly one
//! refresh per frame, and one that is not measures two or more. That is enough
//! to see a level that is too high, and not enough to see headroom — which is
//! why the target handed to the manager sits below the refresh rate (see
//! [`VSYNC_HEADROOM`]) and why, in this window, the level falls but rarely
//! climbs back. The editor's viewport has no such blind spot: it renders
//! offscreen and times the work itself.

use std::time::Duration;

use winit::window::Window;

use crate::quality::{FrameRateManager, QualityLevel};

/// What a frame is allowed to cost, as a share of one display refresh. A frame
/// presented on every refresh costs exactly one; measured against the refresh
/// itself it would read as *just* over budget as often as under, and the level
/// would walk down a display that was keeping up perfectly.
const VSYNC_HEADROOM: f32 = 0.9;
/// What to aim for when the window is on no monitor the platform will name.
const FALLBACK_HZ: f32 = 60.0;
/// How many frames after a switch go unmeasured.
///
/// A switch costs a fraction of a frame (see [`crate::renderer::Renderer::set_quality`]),
/// but the frame it lands in still carries it, and presenting one is already
/// paced by the display: feeding those to the manager would read as a slow patch
/// and walk the level straight back down.
const SETTLE_FRAMES: u8 = 3;

/// The frame rate manager, while `--quality auto` is in force.
pub(super) struct Automatic {
    manager: FrameRateManager,
    /// Frames left to ignore after a switch, so the manager never measures the
    /// switch itself.
    settling: u8,
}

impl Automatic {
    /// `None` for any pinned level: there is nothing to manage, and a manager
    /// built anyway would quietly override the level that was asked for.
    pub(super) fn new(quality: QualityLevel, window: &Window) -> Option<Self> {
        if quality != QualityLevel::Automatic {
            return None;
        }

        Some(Automatic {
            manager: FrameRateManager::new(target_hz(refresh_hz(window))),
            settling: SETTLE_FRAMES,
        })
    }

    /// One presented frame, timed from the last one, and the level to redraw at
    /// if it moved.
    pub(super) fn record(&mut self, frame: Duration) -> Option<u8> {
        if self.settling > 0 {
            self.settling -= 1;
            return None;
        }

        self.manager.record(frame);
        let level = self.manager.changed();
        if level.is_some() {
            self.settling = SETTLE_FRAMES;
        }
        level
    }
}

/// The refresh rate of the display the window is on, as far as the platform will
/// say. X11 through winit answers in millihertz, and 0 when it cannot answer.
fn refresh_hz(window: &Window) -> Option<f32> {
    let millihertz = window.current_monitor()?.refresh_rate_millihertz()?;
    (millihertz > 0).then(|| millihertz as f32 / 1e3)
}

fn target_hz(refresh_hz: Option<f32>) -> f32 {
    refresh_hz.unwrap_or(FALLBACK_HZ) * VSYNC_HEADROOM
}

#[cfg(test)]
mod tests {
    use super::{target_hz, FALLBACK_HZ, VSYNC_HEADROOM};

    #[test]
    fn the_target_sits_below_the_refresh_rate_it_was_given() {
        let target = target_hz(Some(60.0));
        assert!(target < 60.0, "{target}");
        assert!((target - 60.0 * VSYNC_HEADROOM).abs() < 0.01);
    }

    #[test]
    fn a_display_that_answers_nothing_is_taken_for_a_60_hz_one() {
        assert_eq!(target_hz(None), target_hz(Some(FALLBACK_HZ)));
    }
}
