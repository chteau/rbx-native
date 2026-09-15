//! Whether the 3D view is still on screen.
//!
//! The dock renders only the active tab of a group, so a viewport switched
//! away from simply stops having [`super::WorkspaceView::render`] called —
//! there is no event for it, and the panel's last known size just sits there
//! unchanged. The only signal available is therefore indirect: did GPUI
//! repaint this panel since the last tick?
//!
//! That inference is only sound while *something* keeps causing repaints, and
//! ordinarily the only thing that does is a finished frame landing (see
//! `WorkspaceView::show_frame`, which is what calls `cx.notify()`). A long
//! stall on the render thread — a Command Bar script rebuilds the whole scene,
//! textures and all, which takes seconds — looks identical to a tab switched
//! away: no frames, so no repaints, so no paint to observe. Concluding
//! "hidden" from that stops the render thread, which stops the frames, which
//! stops the repaints, and the state sustains itself: the viewport stays blank
//! until some unrelated notification happens to repaint the window.
//!
//! So the tick that would otherwise give up asks for a repaint instead of
//! concluding anything. A mounted panel answers it and stays visible; one the
//! dock is not showing cannot answer, and is declared hidden on the following
//! tick exactly as before.

/// How many consecutive `advance` ticks may go by with `render` not called
/// before the panel is declared hidden. One frame budget's worth of misses:
/// enough to absorb the ordinary gap between a frame landing and GPUI actually
/// repainting for it, short enough that switching tabs away stops the render
/// thread within about a frame.
pub(super) const HIDDEN_AFTER_MISSES: u32 = super::POLLS_PER_FRAME;

/// What one tick's paint check means for the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Presence {
    pub(super) missed: u32,
    pub(super) visible: bool,
    /// Force one repaint, so a panel that really is mounted gets the chance to
    /// prove it before being declared hidden — see this module's doc comment
    /// for why it would otherwise never get another.
    pub(super) probe: bool,
}

pub(super) fn presence(painted: bool, missed: u32, visible: bool) -> Presence {
    if painted {
        return Presence {
            missed: 0,
            visible: true,
            probe: false,
        };
    }

    let missed = missed.saturating_add(1);
    Presence {
        missed,
        visible: visible && missed <= HIDDEN_AFTER_MISSES,
        // Exactly once, on the last tick before giving up: repainting on every
        // tick of a genuinely hidden panel would be a busy loop over a window
        // nobody is looking at.
        probe: visible && missed == HIDDEN_AFTER_MISSES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_painted_panel_is_visible_and_forgets_its_misses() {
        let seen = presence(true, 7, false);
        assert_eq!(
            seen,
            Presence {
                missed: 0,
                visible: true,
                probe: false,
            }
        );
    }

    #[test]
    fn a_panel_the_dock_stopped_showing_is_declared_hidden() {
        let mut state = Presence {
            missed: 0,
            visible: true,
            probe: false,
        };
        for _ in 0..=HIDDEN_AFTER_MISSES {
            state = presence(false, state.missed, state.visible);
        }
        assert!(
            !state.visible,
            "a hidden panel keeps the render thread busy"
        );
    }

    #[test]
    fn the_last_tick_before_giving_up_asks_for_a_repaint() {
        // The whole point: a render thread stalled on a scene rebuild looks
        // exactly like a switched-away tab, and without this probe the panel
        // would be declared hidden, which stops the very frames whose absence
        // was the only evidence for it.
        let mut state = Presence {
            missed: 0,
            visible: true,
            probe: false,
        };
        let mut probes = 0;
        for _ in 0..HIDDEN_AFTER_MISSES {
            state = presence(false, state.missed, state.visible);
            probes += u32::from(state.probe);
        }

        assert_eq!(probes, 1);
        assert!(state.visible, "nothing is concluded on the probing tick");
    }

    #[test]
    fn a_mounted_panel_that_answers_the_probe_never_goes_hidden() {
        // The stalled-reload case: no frames are landing, so nothing paints on
        // its own, but the panel is really there and answers every probe.
        let mut state = Presence {
            missed: 0,
            visible: true,
            probe: false,
        };
        for tick in 0..200 {
            let painted = std::mem::take(&mut state.probe);
            state = presence(painted, state.missed, state.visible);
            assert!(state.visible, "went hidden while mounted on tick {tick}");
        }
    }

    #[test]
    fn a_hidden_panel_is_not_probed_over_and_over() {
        let mut state = presence(false, HIDDEN_AFTER_MISSES, true);
        assert!(!state.visible);
        for _ in 0..50 {
            state = presence(false, state.missed, state.visible);
            assert!(!state.probe, "repainting a window nobody is looking at");
        }
    }

    #[test]
    fn a_hidden_panel_comes_back_as_soon_as_it_paints_again() {
        let hidden = presence(false, HIDDEN_AFTER_MISSES, true);
        assert!(!hidden.visible);

        let back = presence(true, hidden.missed, hidden.visible);
        assert!(back.visible);
        assert_eq!(back.missed, 0);
    }

    #[test]
    fn a_miss_count_left_running_for_days_does_not_wrap() {
        let state = presence(false, u32::MAX, false);
        assert_eq!(state.missed, u32::MAX);
        assert!(!state.visible);
    }
}
