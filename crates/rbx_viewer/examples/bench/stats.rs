//! Summarising a set of timings the way a performance claim has to be read.
//!
//! Median and p95, never a mean: one hitch — a driver recompiling a shader, a
//! compositor taking the card for a frame — moves a mean far enough to hide a
//! real regression, and the tail is what a user actually experiences as lag.

use std::time::Duration;

/// Timings for one measured operation, in milliseconds.
///
/// Keeps every sample rather than only its summary: the JSON report carries
/// them so a later diff can re-test the distribution instead of trusting two
/// numbers, and the order they were taken in is what exposes a phase that
/// never reached steady state.
pub(crate) struct Samples {
    taken: Vec<f64>,
    sorted: Vec<f64>,
}

impl Samples {
    pub(crate) fn new(durations: &[Duration]) -> Self {
        let taken: Vec<f64> = durations.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
        let mut sorted = taken.clone();
        // Timings are finite by construction, so a total order needs no NaN case.
        sorted.sort_by(f64::total_cmp);
        Samples { taken, sorted }
    }

    pub(crate) fn len(&self) -> usize {
        self.taken.len()
    }

    /// Whether the run this summarises collected nothing at all.
    ///
    /// Every summary below answers `0.0` for an empty run, which is not a
    /// timing and must never be printed as one — a report asks this first and
    /// says so plainly instead (see `report::stat`).
    pub(crate) fn is_empty(&self) -> bool {
        self.taken.is_empty()
    }

    pub(crate) fn taken(&self) -> &[f64] {
        &self.taken
    }

    pub(crate) fn median(&self) -> f64 {
        self.percentile(0.5)
    }

    pub(crate) fn p95(&self) -> f64 {
        self.percentile(0.95)
    }

    pub(crate) fn min(&self) -> f64 {
        self.sorted.first().copied().unwrap_or(0.0)
    }

    pub(crate) fn max(&self) -> f64 {
        self.sorted.last().copied().unwrap_or(0.0)
    }

    /// How far the run spread, as a percentage of its own median.
    ///
    /// The honesty check on a reported number: a phase whose slowest sample is
    /// far from its median measured the machine's other work as much as it
    /// measured the code.
    pub(crate) fn spread(&self) -> f64 {
        let median = self.median();
        if median <= 0.0 {
            return 0.0;
        }
        (self.max() - self.min()) / median * 100.0
    }

    /// Nearest-rank percentile: the smallest sample at or above `fraction` of
    /// the run. No interpolation, so every number printed is one that was
    /// actually measured rather than an average of two that were not.
    fn percentile(&self, fraction: f64) -> f64 {
        if self.sorted.is_empty() {
            return 0.0;
        }
        let rank = (fraction * self.sorted.len() as f64).ceil() as usize;
        let index = rank.saturating_sub(1).min(self.sorted.len() - 1);
        self.sorted[index]
    }
}

#[cfg(test)]
mod tests {
    use super::Samples;
    use std::time::Duration;

    fn samples(millis: &[u64]) -> Samples {
        let durations: Vec<Duration> = millis.iter().map(|ms| Duration::from_millis(*ms)).collect();
        Samples::new(&durations)
    }

    #[test]
    fn percentiles_are_measured_samples_not_interpolations() {
        let stats = samples(&[10, 20, 30, 40, 100]);
        assert_eq!(stats.median(), 30.0);
        // Nearest rank on five samples puts p95 on the last one.
        assert_eq!(stats.p95(), 100.0);
        assert_eq!(stats.min(), 10.0);
        assert_eq!(stats.max(), 100.0);
    }

    #[test]
    fn a_single_outlier_moves_the_mean_but_not_the_median() {
        let steady = samples(&[10, 10, 10, 10, 10]);
        let hitched = samples(&[10, 10, 10, 10, 500]);
        assert_eq!(steady.median(), hitched.median());
        assert!(hitched.p95() > steady.p95());
    }

    #[test]
    fn spread_is_relative_to_the_median() {
        // 8..12 around a median of 10 is a 40% spread.
        assert!((samples(&[8, 10, 10, 10, 12]).spread() - 40.0).abs() < 1e-9);
        assert_eq!(samples(&[]).spread(), 0.0);
    }

    #[test]
    fn an_empty_run_is_distinguishable_from_a_fast_one() {
        let nothing = samples(&[]);
        let fast = samples(&[0]);
        // Both summarise to zero, so the summaries cannot tell them apart and
        // whatever prints them has to ask.
        assert_eq!(nothing.median(), fast.median());
        assert!(nothing.is_empty());
        assert!(!fast.is_empty());
    }

    #[test]
    fn samples_are_kept_in_the_order_they_were_taken() {
        let stats = samples(&[30, 10, 20]);
        assert_eq!(stats.taken(), [30.0, 10.0, 20.0]);
        assert_eq!(stats.len(), 3);
    }
}
