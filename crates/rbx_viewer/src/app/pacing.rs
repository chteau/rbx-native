//! Smooths the wall-clock `dt` used to integrate camera motion.
//!
//! `Viewer::redraw` measures `dt` as CPU wall-clock time between redraw callbacks, but
//! presentation is vsync-paced (`Fifo`, the default from `get_default_config`): the
//! display shows frames on a rock-steady beat while the *measured* interval between
//! callbacks bounces around that beat's mean, contaminated by OS scheduling noise and by
//! how the CPU thread happens to wake up relative to vblank under `ControlFlow::Poll`.
//! Feeding that noisy sample straight into `velocity * dt` position integration turns
//! ordinary scheduling jitter into visible per-frame camera jitter, even though the
//! frames themselves land on an even beat and the FPS counter — which only ever sees an
//! average — never moves.
//!
//! [`SmoothedDt`] is a light exponential low-pass filter over `dt`, with a clamp that
//! keeps one wild outlier (a real stutter or an alt-tab stall) from yanking the estimate
//! around before it can be folded in gradually. A heavier filter would smooth this out
//! too but make the camera feel laggy when the player actually changes speed, so the
//! time constant here is deliberately short.

use std::time::Duration;

/// How far a raw sample may be from the running average before it is clamped to the
/// edge of the band. Wide enough that ordinary vsync-timing noise never engages it
/// (that's the EMA's job), tight enough that one huge stall sample can't drag the
/// estimate to it in a single frame.
const CLAMP_BAND: f32 = 0.25;
/// How fast the running average chases a (possibly clamped) sample. Low enough that a
/// single frame of noise barely moves it, high enough that a sustained rate change
/// (a quality-level switch, a real slowdown) is fully absorbed within ~30 frames
/// rather than lagging behind it visibly.
const EMA_ALPHA: f32 = 0.2;

/// A running low-pass estimate of the true frame interval, used to de-noise `dt`
/// before it drives camera position integration.
pub(super) struct SmoothedDt {
    average_secs: f32,
}

impl SmoothedDt {
    pub(super) fn new() -> Self {
        SmoothedDt { average_secs: 0.0 }
    }

    /// Folds `raw` into the running average (clamped first, guarding against one
    /// outlier sample) and returns the updated average as the `dt` to move by.
    pub(super) fn sample(&mut self, raw: Duration) -> Duration {
        let raw_secs = raw.as_secs_f32();
        if self.average_secs <= 0.0 {
            // Nothing to compare against yet; a zero-width band around zero would
            // clamp every future sample to zero.
            self.average_secs = raw_secs;
            return raw;
        }

        let band = self.average_secs * CLAMP_BAND;
        let clamped_secs = raw_secs.clamp(
            (self.average_secs - band).max(0.0),
            self.average_secs + band,
        );
        self.average_secs += (clamped_secs - self.average_secs) * EMA_ALPHA;
        Duration::from_secs_f32(self.average_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds a smoother and the old raw-`dt` path the same jittery-but-fixed-mean
    /// frame-time sequence a noisy vsync clock actually produces (alternating +/-12%
    /// around a 60Hz mean, not a single outlier), then checks the smoothed path's
    /// per-step displacement varies meaningfully less — a concrete, numeric claim
    /// about jitter, not just that the fix changes some numbers.
    #[test]
    fn smoothing_meaningfully_reduces_displacement_variance_under_vsync_jitter() {
        const VELOCITY: f32 = 30.0; // studs/s, a typical cruise speed
        const MEAN_DT: f32 = 1.0 / 60.0;
        const JITTER: f32 = 0.12;

        let raw_dts: Vec<f32> = (0..120)
            .map(|i| {
                let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
                MEAN_DT * (1.0 + sign * JITTER)
            })
            .collect();
        let raw_displacements: Vec<f32> = raw_dts.iter().map(|dt| VELOCITY * dt).collect();

        let mut smoother = SmoothedDt::new();
        let smoothed_displacements: Vec<f32> = raw_dts
            .iter()
            .map(|&dt| VELOCITY * smoother.sample(Duration::from_secs_f32(dt)).as_secs_f32())
            .collect();

        let raw_variance = variance(&raw_displacements);
        let smoothed_variance = variance(&smoothed_displacements);
        assert!(
            smoothed_variance < raw_variance * 0.5,
            "expected smoothing to meaningfully cut displacement variance: raw={raw_variance}, smoothed={smoothed_variance}"
        );

        // The fix removes noise, it must not quietly slow the camera down: total
        // ground covered should still track the raw total closely.
        let raw_total: f32 = raw_displacements.iter().sum();
        let smoothed_total: f32 = smoothed_displacements.iter().sum();
        assert!(
            (raw_total - smoothed_total).abs() / raw_total < 0.05,
            "raw={raw_total}, smoothed={smoothed_total}"
        );
    }

    /// A held key shouldn't feel like it's dragging a lagging camera forever after a
    /// real, sustained frame-rate change — only vsync-scale jitter should be filtered.
    #[test]
    fn a_sustained_frame_rate_change_is_absorbed_within_a_few_dozen_frames() {
        let mut smoother = SmoothedDt::new();
        for _ in 0..30 {
            smoother.sample(Duration::from_secs_f32(1.0 / 60.0));
        }

        let mut last = Duration::ZERO;
        for _ in 0..30 {
            last = smoother.sample(Duration::from_secs_f32(1.0 / 30.0));
        }

        let last_secs = last.as_secs_f32();
        assert!(
            (last_secs - 1.0 / 30.0).abs() < 0.001,
            "expected the estimate to have caught up to the new rate, got {last_secs}"
        );
    }

    fn variance(values: &[f32]) -> f32 {
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32
    }
}
