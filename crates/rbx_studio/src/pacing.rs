//! How fast the viewport is allowed to redraw.
//!
//! The cap is the display's own refresh rate: frames drawn faster than the screen
//! can show them cost a render and a 5 MB readback for nothing. GPUI exposes no
//! refresh rate at all, so it is read off the hardware instead (see
//! [`crate::display`]), with 60 Hz when that fails and `RBX_STUDIO_FPS` as the
//! override — the way to pin the rate while measuring, or to work around a
//! display the query reads wrong.
//!
//! Losing OS focus lowers the target further, to one of two user-chosen
//! presets ([`UnfocusedFps`]) — nobody is watching a backgrounded editor
//! render at the display's full rate, and Studio itself doesn't either.
//! [`FocusPacing`] holds that state and the transitions in and out of it;
//! `workspace_view` feeds it window activation and input events, then pushes
//! the resulting interval down to the render thread itself (see
//! `workspace_view::pump::Pump::set_interval`), which is what actually stops
//! the GPU work, not just the UI thread's own poll rate.

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

/// The render rate while the window has lost OS focus — a setting, not a
/// fixed constant: this is *how much* to throttle by, never *whether* to
/// (the roadmap item this exists for always throttles once unfocused).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnfocusedFps {
    Fps25,
    Fps30,
}

impl UnfocusedFps {
    /// Matches real Studio closer than the more aggressive preset while
    /// still cutting the common case (an editor sitting behind a browser)
    /// most of the way to idle.
    pub(crate) const DEFAULT: UnfocusedFps = UnfocusedFps::Fps30;

    pub(crate) const fn fps(self) -> u16 {
        match self {
            UnfocusedFps::Fps25 => 25,
            UnfocusedFps::Fps30 => 30,
        }
    }
}

/// The render loop's target interval given the full-focus budget `full` (see
/// [`frame_interval`]) and whether the window is currently `focused`.
///
/// Never *faster* than `full`: an `RBX_STUDIO_FPS` override or a slow display
/// already asks for something slower than either unfocused preset, and
/// losing focus must not speed the loop back up past whatever that already
/// capped it to.
fn target_interval(full: Duration, unfocused: UnfocusedFps, focused: bool) -> Duration {
    if focused {
        return full;
    }
    full.max(interval(f32::from(unfocused.fps())))
}

/// Tracks whether the render loop should currently use the full-focus budget
/// or the unfocused cap, and the two ways back out of the cap: the window
/// regaining OS activation (`set_active`), or input reaching the viewport
/// before that activation event lands (`mark_input`) — either is "the first
/// focus/input event" the throttle restores on.
///
/// Starts focused: a window that has just opened has the user's attention by
/// definition, nothing to throttle yet.
pub(crate) struct FocusPacing {
    focused: bool,
    unfocused: UnfocusedFps,
}

impl FocusPacing {
    pub(crate) fn new(unfocused: UnfocusedFps) -> Self {
        FocusPacing {
            focused: true,
            unfocused,
        }
    }

    /// The window's own OS activation state changed. Returns whether the
    /// pacing state actually flipped, so a caller only has to push a new
    /// interval down to the render thread when it did — most activation
    /// observers fire on every focus-followed-by-blur pair a click produces,
    /// not just the ones that cross the focused/unfocused line.
    pub(crate) fn set_active(&mut self, active: bool) -> bool {
        if self.focused == active {
            return false;
        }
        self.focused = active;
        true
    }

    /// Input reached the viewport. A no-op once the window already counts as
    /// focused — the common case, so an ordinary mouse move need not
    /// recompute anything — but restores the full rate immediately while
    /// unfocused, ahead of whatever activation event may still be in flight.
    pub(crate) fn mark_input(&mut self) -> bool {
        if self.focused {
            return false;
        }
        self.focused = true;
        true
    }

    /// Switches the unfocused preset itself (the user's setting changed).
    /// Returns whether the *effective* interval changed as a result — only
    /// while actually unfocused; a change made while focused takes effect
    /// the next time focus is lost.
    pub(crate) fn set_unfocused(&mut self, unfocused: UnfocusedFps) -> bool {
        if self.unfocused == unfocused {
            return false;
        }
        self.unfocused = unfocused;
        !self.focused
    }

    /// The interval the render loop should use right now, given the
    /// full-focus budget.
    pub(crate) fn interval(&self, full: Duration) -> Duration {
        target_interval(full, self.unfocused, self.focused)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{interval, parse_hz, target_interval, FocusPacing, UnfocusedFps, FALLBACK_HZ};

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

    #[test]
    fn focused_ignores_the_unfocused_preset_entirely() {
        let full = interval(144.0);
        assert_eq!(target_interval(full, UnfocusedFps::Fps25, true), full);
        assert_eq!(target_interval(full, UnfocusedFps::Fps30, true), full);
    }

    #[test]
    fn unfocused_caps_at_the_chosen_preset() {
        let full = interval(144.0);
        assert_eq!(
            target_interval(full, UnfocusedFps::Fps25, false),
            interval(25.0)
        );
        assert_eq!(
            target_interval(full, UnfocusedFps::Fps30, false),
            interval(30.0)
        );
    }

    #[test]
    fn unfocused_never_speeds_up_an_already_slower_full_rate() {
        // RBX_STUDIO_FPS=10 (or a very slow display) already asks for a
        // budget looser than either preset — losing focus must not claw
        // that back to something faster.
        let full = interval(10.0);
        assert_eq!(target_interval(full, UnfocusedFps::Fps30, false), full);
    }

    #[test]
    fn focus_pacing_starts_focused_regardless_of_preset() {
        let full = interval(60.0);
        let pacing = FocusPacing::new(UnfocusedFps::Fps25);
        assert_eq!(pacing.interval(full), full);
    }

    #[test]
    fn losing_focus_caps_the_rate() {
        let full = interval(60.0);
        let mut pacing = FocusPacing::new(UnfocusedFps::Fps30);
        assert!(pacing.set_active(false));
        assert_eq!(pacing.interval(full), interval(30.0));
    }

    #[test]
    fn regaining_activation_restores_the_full_rate() {
        let full = interval(60.0);
        let mut pacing = FocusPacing::new(UnfocusedFps::Fps30);
        pacing.set_active(false);
        assert!(pacing.set_active(true));
        assert_eq!(pacing.interval(full), full);
    }

    #[test]
    fn an_activation_call_that_does_not_change_anything_reports_so() {
        let mut pacing = FocusPacing::new(UnfocusedFps::Fps30);
        // Already focused: a redundant "still active" observation.
        assert!(!pacing.set_active(true));
        pacing.set_active(false);
        // Already unfocused: a redundant "still inactive" observation.
        assert!(!pacing.set_active(false));
    }

    #[test]
    fn input_restores_the_full_rate_ahead_of_any_activation_event() {
        let full = interval(60.0);
        let mut pacing = FocusPacing::new(UnfocusedFps::Fps25);
        pacing.set_active(false);
        assert_eq!(pacing.interval(full), interval(25.0));

        assert!(pacing.mark_input());
        assert_eq!(pacing.interval(full), full);
    }

    #[test]
    fn input_while_already_focused_is_a_no_op() {
        let mut pacing = FocusPacing::new(UnfocusedFps::Fps30);
        assert!(!pacing.mark_input());
    }

    #[test]
    fn changing_the_preset_takes_effect_immediately_while_unfocused() {
        let full = interval(60.0);
        let mut pacing = FocusPacing::new(UnfocusedFps::Fps30);
        pacing.set_active(false);

        assert!(pacing.set_unfocused(UnfocusedFps::Fps25));
        assert_eq!(pacing.interval(full), interval(25.0));
    }

    #[test]
    fn changing_the_preset_while_focused_only_reports_a_future_change() {
        let mut pacing = FocusPacing::new(UnfocusedFps::Fps30);
        // Focused: the interval in effect right now doesn't change...
        assert!(!pacing.set_unfocused(UnfocusedFps::Fps25));
        // ...but the next time focus is lost, the new preset applies.
        pacing.set_active(false);
        assert_eq!(pacing.interval(interval(60.0)), interval(25.0));
    }
}
