//! A bare Alt tap — Alt pressed and released with nothing in between — is
//! the other half of the desktop convention for reaching a menu bar, next to
//! F10. Recognising it needs a little state, because the *same* Alt press is
//! also a live modifier in this editor: Alt-drag locks a Ball's Scale handle
//! to a round cross-section, and Alt-click cycles the selection under the
//! cursor. Neither may open a menu on the way out.
//!
//! So this is not "Alt went up" but "Alt went down alone, and nothing at all
//! happened before it came back up". Anything else arriving while Alt is
//! held — a keystroke, a mouse button, a second modifier — cancels the tap,
//! and only the next fresh Alt *press* can arm it again. Arming on the press
//! edge rather than on any Alt-alone state is what keeps the tail of a chord
//! (releasing Shift while still holding Alt) from re-arming a tap the chord
//! already cancelled.

use gpui_kit::Modifiers;

#[derive(Default)]
pub(super) struct AltTap {
    /// Whether the Alt press currently in progress is still a candidate.
    armed: bool,
    /// Alt's state as of the last change, so a press edge is distinguishable
    /// from a change that merely left Alt down.
    held: bool,
}

impl AltTap {
    /// Feeds one modifier change in. Returns whether it completed a bare tap,
    /// which is the caller's cue to enter the menu bar.
    pub(super) fn modifiers_changed(&mut self, modifiers: Modifiers) -> bool {
        let alone = modifiers.alt && !modifiers.control && !modifiers.shift && !modifiers.platform;
        let pressed = modifiers.alt && !self.held;
        self.held = modifiers.alt;

        if pressed {
            self.armed = alone;
            return false;
        }
        if alone {
            return false;
        }
        let tapped = self.armed && !modifiers.alt;
        self.armed = false;
        tapped
    }

    /// Any input that is not a modifier change, arriving while Alt is held.
    pub(super) fn interrupt(&mut self) {
        self.armed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alt() -> Modifiers {
        Modifiers {
            alt: true,
            ..Default::default()
        }
    }

    fn with_alt(second: fn(&mut Modifiers)) -> Modifiers {
        let mut modifiers = alt();
        second(&mut modifiers);
        modifiers
    }

    #[test]
    fn alt_down_then_up_with_nothing_between_is_a_tap() {
        let mut tap = AltTap::default();
        assert!(!tap.modifiers_changed(alt()));
        assert!(tap.modifiers_changed(Modifiers::default()));
    }

    /// The case that makes this a state machine rather than a key check:
    /// Alt-drag on a Scale handle ends with Alt coming back up, and must not
    /// leave a menu open behind it. A keystroke under Alt is the same story.
    #[test]
    fn an_interruption_while_alt_is_held_cancels_the_tap() {
        let mut tap = AltTap::default();
        tap.modifiers_changed(alt());
        tap.interrupt();
        assert!(!tap.modifiers_changed(Modifiers::default()));
    }

    /// Alt+Shift and Alt+Ctrl are chords. Releasing the pair releases Alt
    /// too, so without this the tail of every Alt chord would read as a tap.
    #[test]
    fn a_second_modifier_joining_alt_cancels_the_tap() {
        for second in [
            |m: &mut Modifiers| m.shift = true,
            |m: &mut Modifiers| m.control = true,
            |m: &mut Modifiers| m.platform = true,
        ] {
            let mut tap = AltTap::default();
            tap.modifiers_changed(alt());
            tap.modifiers_changed(with_alt(second));
            assert!(!tap.modifiers_changed(Modifiers::default()));
        }
    }

    /// Letting go of the *other* half of a chord leaves Alt alone again, but
    /// the chord already spent this press — only a fresh one re-arms.
    #[test]
    fn releasing_the_other_modifier_does_not_re_arm_the_same_press() {
        let mut tap = AltTap::default();
        tap.modifiers_changed(alt());
        tap.modifiers_changed(with_alt(|m| m.shift = true));
        tap.modifiers_changed(alt());
        assert!(!tap.modifiers_changed(Modifiers::default()));
    }

    /// Alt pressed while Ctrl is already down is a chord from the start.
    #[test]
    fn alt_pressed_into_a_held_modifier_is_not_armed() {
        let mut tap = AltTap::default();
        tap.modifiers_changed(Modifiers {
            control: true,
            ..Default::default()
        });
        tap.modifiers_changed(with_alt(|m| m.control = true));
        assert!(!tap.modifiers_changed(Modifiers::default()));
    }

    /// A modifier going up on its own must not be read as the end of an Alt
    /// tap that never started.
    #[test]
    fn a_release_without_a_press_is_not_a_tap() {
        let mut tap = AltTap::default();
        assert!(!tap.modifiers_changed(Modifiers::default()));
    }

    #[test]
    fn a_fresh_press_after_a_cancelled_one_taps_normally() {
        let mut tap = AltTap::default();
        tap.modifiers_changed(alt());
        tap.interrupt();
        tap.modifiers_changed(Modifiers::default());

        assert!(!tap.modifiers_changed(alt()));
        assert!(tap.modifiers_changed(Modifiers::default()));
    }
}
