//! The UI scale and the reduce-motion preference — the two accessibility
//! settings that have to be resolved before the first frame, and that the
//! rest of the editor reads through [`crate::tokens`].
//!
//! Both live as atomics in `tokens` rather than on `Shell`, because the code
//! that reads them is a styling callback with no `App` in reach. This module
//! owns the other half: where their values come from, and what changes them.

use gpui_kit::{App, Keystroke, Modifiers};

use crate::tokens;

/// `RBX_STUDIO_REDUCE_MOTION=1` forces the preference on, `=0` forces it
/// off. Without it the desktop is asked (see [`detect_reduced_motion`]) —
/// the variable exists so a screenshot run can exercise both branches,
/// which is otherwise impossible without changing the tester's own desktop.
pub(crate) const REDUCE_MOTION_VARIABLE: &str = "RBX_STUDIO_REDUCE_MOTION";

/// What a UI-scale keystroke asks for. Ctrl+= / Ctrl+- / Ctrl+0, matching
/// VS Code's own zoom bindings, because that is the shortcut people already
/// have in their fingers for exactly this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scale {
    In,
    Out,
    Reset,
}

/// One step. Small enough that the scale is a dial rather than three
/// presets, large enough that a single press is visibly a change.
const STEP: f32 = 0.1;

impl Scale {
    /// The scale this keystroke moves `current` to, or `current` itself
    /// when the range is already exhausted.
    pub(crate) fn apply(self, current: f32) -> f32 {
        let (low, high) = tokens::FONT_SCALE_RANGE;
        match self {
            Scale::In => (current + STEP).min(high),
            Scale::Out => (current - STEP).max(low),
            Scale::Reset => 1.,
        }
    }
}

/// Which scale command a keystroke is, if any.
///
/// `equal` and `minus` rather than `plus`/`underscore`: GPUI reports the
/// *unshifted* key, so Ctrl and the `+` key arrive as `ctrl-=` on a US
/// layout. Both spellings are accepted anyway, since a layout that puts `+`
/// on its own key (or an AZERTY numeric row) reports the other one.
pub(crate) fn action_for(key: &str, modifiers: Modifiers) -> Option<Scale> {
    if !modifiers.control || modifiers.alt || modifiers.function {
        return None;
    }

    match key {
        "=" | "+" | "plus" | "equal" => Some(Scale::In),
        "-" | "_" | "minus" => Some(Scale::Out),
        "0" => Some(Scale::Reset),
        _ => None,
    }
}

pub(crate) fn action_for_keystroke(keystroke: &Keystroke) -> Option<Scale> {
    action_for(&keystroke.key, keystroke.modifiers)
}

/// Resolves the desktop's "reduce motion" preference once, at startup, and
/// sets both copies of it: GPUI's own (which `with_animation` already
/// honours) and `tokens`' mirror (which the hover styles read).
///
/// GPUI never learns this by itself — no backend calls `set_reduce_motion`
/// — so somebody has to ask, and this is that somebody.
/// Runs before the window exists, so a `Settings::reduce_motion` of `None`
/// simply leaves this answer standing — the desktop is the default, and an
/// explicit choice in the View menu overrides it.
pub(crate) fn install(cx: &mut App) {
    let reduced = detect_reduced_motion();
    cx.set_reduce_motion(reduced);
    tokens::set_reduced_motion(reduced);
}

/// The environment variable if it is set, else the desktop's own setting,
/// else `false`.
///
/// On Linux the setting lives in GSettings as
/// `org.gnome.desktop.interface.enable-animations`, which GTK, GNOME and
/// (through the XDG settings portal) most other desktops read. A machine
/// without `gsettings`, or a desktop that doesn't publish the key, answers
/// "no preference" rather than failing — which is the right default: a
/// wrongly-suppressed animation is a smaller harm than a wrongly-played
/// one only in the other direction, and guessing "reduce" for everybody
/// would be its own accessibility problem.
pub(crate) fn detect_reduced_motion() -> bool {
    if let Ok(forced) = std::env::var(REDUCE_MOTION_VARIABLE) {
        return matches!(forced.trim(), "1" | "true" | "yes" | "on");
    }

    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "enable-animations"])
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                return String::from_utf8_lossy(&output.stdout).trim() == "false";
            }
        }
    }

    false
}

#[cfg(test)]
#[path = "scale/tests.rs"]
mod tests;
