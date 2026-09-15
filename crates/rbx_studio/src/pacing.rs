//! How fast the viewport is allowed to redraw.
//!
//! The cap is the display's own refresh rate: frames drawn faster than the screen
//! can show them cost a render and a 5 MB readback for nothing. GPUI exposes no
//! refresh rate at all, so it is read off the hardware instead (see
//! [`crate::display`]), with 60 Hz when that fails and `RBX_STUDIO_FPS` as the
//! override — the way to pin the rate while measuring, or to work around a
//! display the query reads wrong.

use std::env;
use std::ops::RangeInclusive;
use std::time::Duration;

/// What a display runs at when nothing could be asked: the rate almost every
/// panel still has, and the one Studio itself assumes.
const FALLBACK_HZ: f32 = 60.0;
/// Below the bottom a frame budget is a stall rather than a budget, and above the
/// top the cap is not what limits anything. Both ends also keep the arithmetic
/// clear of zero and of infinity.
const HZ: RangeInclusive<f32> = 1.0..=1000.0;
const FPS_VARIABLE: &str = "RBX_STUDIO_FPS";

/// The frame budget the render loop paces itself to, `refresh_hz` being what the
/// hardware reported, if anything.
pub(crate) fn frame_interval(refresh_hz: Option<f32>) -> Duration {
    let overridden = env::var(FPS_VARIABLE)
        .ok()
        .and_then(|raw| parse_hz(&raw))
        .or(refresh_hz);

    interval(overridden.unwrap_or(FALLBACK_HZ))
}

/// The override, `None` for anything unusable — a typo in the variable must slow
/// the view down, never stop it.
fn parse_hz(raw: &str) -> Option<f32> {
    let hz: f32 = raw.trim().parse().ok()?;
    HZ.contains(&hz).then_some(hz)
}

fn interval(hz: f32) -> Duration {
    Duration::from_secs_f32(1.0 / hz.clamp(*HZ.start(), *HZ.end()))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{interval, parse_hz, FALLBACK_HZ};

    fn millis(interval: Duration) -> f32 {
        interval.as_secs_f32() * 1000.0
    }

    #[test]
    fn the_budget_is_one_refresh_long() {
        assert!((millis(interval(60.0)) - 16.667).abs() < 0.01);
        assert!((millis(interval(75.0)) - 13.333).abs() < 0.01);
        assert!((millis(interval(144.0)) - 6.944).abs() < 0.01);
    }

    // A rate of zero would divide by zero, and a negative one would run the
    // timer backwards; both come from a display that answered nonsense.
    #[test]
    fn an_impossible_rate_still_yields_a_usable_budget() {
        assert_eq!(interval(0.0), Duration::from_secs(1));
        assert_eq!(interval(-30.0), Duration::from_secs(1));
        assert!(millis(interval(f32::INFINITY)) > 0.0);
    }

    #[test]
    fn the_override_is_read_as_a_rate_in_hz() {
        assert_eq!(parse_hz("120"), Some(120.0));
        assert_eq!(parse_hz(" 59.94 "), Some(59.94));
    }

    #[test]
    fn an_unusable_override_is_ignored_rather_than_obeyed() {
        assert_eq!(parse_hz(""), None);
        assert_eq!(parse_hz("fast"), None);
        assert_eq!(parse_hz("0"), None);
        assert_eq!(parse_hz("-60"), None);
        assert_eq!(parse_hz("100000"), None);
    }

    #[test]
    fn a_display_that_answers_nothing_is_taken_for_a_60_hz_one() {
        assert_eq!(interval(FALLBACK_HZ), interval(60.0));
    }
}
