//! Deciding when a hover ray gets resolved against the DOM at all, and how
//! often.
//!
//! Both used to be answered the wrong way. Suppression checked
//! `PointerLock::holds` — whether the OS-level pointer lock actually
//! engaged — rather than whether a look gesture was in progress, which is
//! wrong on Wayland: `holds` sits `false` for the whole gesture there (see
//! `pointer_lock`'s own module doc for why), so every move during a Wayland
//! orbit fell through to resolving a new hover instead of being suppressed.
//! And nothing throttled the resolve at all: `Shell::hover_in_viewport` runs
//! `pick::parts_along`, a full-scene walk, and used to pay for it on every
//! reported mouse-move, which can fire far more often than the display
//! refreshes.

use std::time::Duration;

/// Whether hover resolution should be skipped for this move — an actual look
/// gesture in progress, never `PointerLock::holds`. See this module's doc
/// comment for why the two disagree on Wayland.
pub(super) fn suppressed(looking: bool) -> bool {
    looking
}

/// Whether enough time has passed since the last resolve to pay for another
/// one — at most once per rendered frame, so a burst of raw mouse-move
/// events landing between two frames only repeats `pick::parts_along`'s
/// full-scene walk once. `None` (nothing resolved yet) is always due.
pub(super) fn due(elapsed_since_resolved: Option<Duration>, interval: Duration) -> bool {
    elapsed_since_resolved.is_none_or(|elapsed| elapsed >= interval)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_gesture_suppresses_hover_however_the_lock_behaved() {
        // The Wayland fallback: no OS-level pointer capture engages at all,
        // so `PointerLock::holds` sits false for the whole gesture, yet the
        // look is still in progress and hover must still be suppressed.
        assert!(suppressed(true));
    }

    #[test]
    fn hover_resolves_with_nothing_held() {
        assert!(!suppressed(false));
    }

    #[test]
    fn the_first_resolve_is_always_due() {
        assert!(due(None, Duration::from_millis(16)));
    }

    #[test]
    fn a_move_inside_the_frame_budget_waits() {
        assert!(!due(
            Some(Duration::from_millis(5)),
            Duration::from_millis(16)
        ));
    }

    #[test]
    fn a_move_past_the_frame_budget_is_due() {
        assert!(due(
            Some(Duration::from_millis(16)),
            Duration::from_millis(16)
        ));
        assert!(due(
            Some(Duration::from_millis(30)),
            Duration::from_millis(16)
        ));
    }
}
