use super::*;
use crate::tokens::FONT_SCALE_RANGE;

fn ctrl() -> Modifiers {
    Modifiers {
        control: true,
        ..Modifiers::default()
    }
}

/// The scale is this app's whole answer to WCAG 1.4.4, so it has to be
/// reachable from the keyboard on the layouts people actually type on —
/// GPUI reports the unshifted key, so "Ctrl and the plus key" arrives as
/// `ctrl-=` on US and as something else elsewhere.
#[test]
fn every_spelling_of_the_zoom_keys_is_recognised() {
    for key in ["=", "+", "plus", "equal"] {
        assert_eq!(action_for(key, ctrl()), Some(Scale::In), "{key}");
    }
    for key in ["-", "_", "minus"] {
        assert_eq!(action_for(key, ctrl()), Some(Scale::Out), "{key}");
    }
    assert_eq!(action_for("0", ctrl()), Some(Scale::Reset));
}

/// Ctrl is the whole binding: without it these are just characters someone
/// is typing into the Command Bar or a property field.
#[test]
fn an_unmodified_key_is_not_a_zoom_command() {
    assert_eq!(action_for("=", Modifiers::default()), None);
    assert_eq!(action_for("0", Modifiers::default()), None);
    assert_eq!(
        action_for(
            "=",
            Modifiers {
                control: true,
                alt: true,
                ..Modifiers::default()
            }
        ),
        None,
        "ctrl-alt-= belongs to whatever binds it, not to the UI scale"
    );
}

/// Holding the key down must stop at the end of the range rather than
/// running off it — and must still be *at* the end, not one step short.
#[test]
fn zooming_saturates_at_both_ends_of_the_supported_range() {
    let mut scale = 1.;
    for _ in 0..100 {
        scale = Scale::In.apply(scale);
    }
    assert_eq!(scale, FONT_SCALE_RANGE.1);

    for _ in 0..100 {
        scale = Scale::Out.apply(scale);
    }
    assert_eq!(scale, FONT_SCALE_RANGE.0);

    assert_eq!(Scale::Reset.apply(scale), 1.);
}
