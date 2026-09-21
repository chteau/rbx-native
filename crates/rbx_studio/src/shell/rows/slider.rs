//! The panel's slider, which the design frame does not have.
//!
//! Built from `gpui_base`'s unstyled slider primitives rather than the
//! toolkit's own finished `Slider`, for one reason: that one sizes its
//! rail, its thumb and its target in `rem`, and nothing here sets a rem
//! size, so all three would ignore the UI scale that `UX_GUIDELINES.md` §1
//! makes this app's answer to WCAG 1.4.4. The behaviour — where a click
//! lands, what a drag maps to, when the gesture ends — is still the
//! toolkit's; only the skin is ours.
//!
//! The skin is the field box's, softened: an empty rail carries
//! [`tokens::chrome`] and no border, exactly as `super::field_box` does and
//! for the reason its own comment gives — the surface change is the whole
//! affordance, and every other input in this panel establishes its extent
//! the same way. What the rail is *set* to is then the one thing on it
//! carrying an accent, which is [`tokens::check_on`], this panel's "on"
//! fill wherever it appears.

use gpui_kit::base::{Slider as Behaviour, SliderIndicator, SliderThumb, SliderTrack};
use gpui_kit::component::slider::SliderState;
use gpui_kit::*;

use crate::tokens;

/// The rail, its filled part, and the grip — sized to the row it shares
/// with the property's own number field.
pub(super) fn slider(state: &Entity<SliderState>, cx: &App) -> impl IntoElement {
    // A single value keeps the range's end and leaves its start at zero,
    // so the fill always runs from the left edge.
    let filled = state.read(cx).percentage().end;

    Behaviour::new(state)
        .flex()
        .flex_1()
        .min_w(tokens::slider_min_width())
        .items_center()
        .child(
            SliderTrack::new(state)
                .relative()
                .flex()
                .w_full()
                .items_center()
                // The target is the whole strip, not the grip: 13px of
                // thumb is barely half WCAG 2.5.8's floor, and a click
                // anywhere along a rail is expected to jump to it anyway.
                .h(tokens::input_height())
                .child(
                    // The indicator is what records the geometry a pointer
                    // position is read against, so it is the rail's full
                    // width.
                    SliderIndicator::new(state)
                        .relative()
                        .w_full()
                        .h(tokens::slider_rail())
                        .rounded_full()
                        .bg(tokens::chrome())
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .right(relative(1. - filled))
                                .rounded_full()
                                .bg(tokens::check_on()),
                        ),
                )
                .child(
                    // The grip's travel, a grip narrower than the rail so
                    // it stops flush inside either end rather than hanging
                    // over it. That inset is also what keeps every offset
                    // below positive: GPUI will not place a child left of
                    // its parent's own edge, so a grip centred on the value
                    // by a negative margin loses its outer half at 0.
                    div()
                        .absolute()
                        .inset_0()
                        .right(tokens::slider_thumb())
                        .child(
                            SliderThumb::new(state)
                                .absolute()
                                .left(relative(filled))
                                .top(relative(0.5))
                                .mt(-(tokens::slider_thumb() / 2.))
                                .flex_none()
                                .size(tokens::slider_thumb())
                                .rounded_full()
                                .bg(tokens::text_full()),
                        ),
                ),
        )
}
