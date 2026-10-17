//! The on-screen keyboard cover: a plain key grid drawn over the bottom of
//! the simulated screen so a layout can be judged against the space a real
//! keyboard takes. It is only a picture; the game is never told it is there.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

/// Keys per row for the three letter rows.
const LETTER_ROWS: [usize; 3] = [10, 9, 9];

fn key() -> Div {
    // `tile` is the dock colour, so it would vanish on `chrome`: a white
    // wash reads as a key on any surface.
    div()
        .flex_1()
        .h_full()
        .bg(tokens::border2())
        .rounded(px(3.))
}

fn row() -> Div {
    div().flex().flex_row().flex_1().min_h_0().gap_0p5()
}

/// The cover placed at `left`/`top` with the given size, in the overlay's
/// logical pixels. `occlude` cuts it out of an embedded Play client window
/// so it shows above it.
pub(super) fn cover(
    left: Pixels,
    top: Pixels,
    width: Pixels,
    height: Pixels,
    occlude: bool,
) -> Div {
    let letters = LETTER_ROWS
        .iter()
        .map(|&count| row().children((0..count).map(|_| key())));
    let bottom = row()
        .child(key().flex_none().w_1_5())
        .child(key())
        .child(key().flex_none().w_1_5());
    div()
        .absolute()
        .left(left)
        .top(top)
        .w(width)
        .h(height)
        .flex()
        .flex_col()
        .gap_0p5()
        .px_1()
        .pb_1()
        .pt(px(12.))
        .bg(tokens::chrome())
        .overflow_hidden()
        .children(letters)
        .child(bottom)
        .child(
            div()
                .absolute()
                .top_0()
                .right_1()
                .text_size(tokens::viewport_text(10.))
                .text_color(tokens::text3())
                .child("Keyboard cover \u{2014} not reported to the game"),
        )
        .when(occlude, |this| this.child(gpui_base::occlusion::probe()))
}
