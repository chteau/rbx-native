//! The Beta features page: nothing ships behind a beta switch yet, so an
//! empty state, and the restart banner a change would bring, faded.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::tokens;

use super::kit::{icon, soon_pill, text};
use super::SettingsWindow;

impl SettingsWindow {
    /// Beta features: an empty state, and the restart banner a change
    /// would bring, drawn faded as a preview.
    pub(super) fn beta_body(&self) -> AnyElement {
        v_flex()
            .gap(px(22.))
            .child(
                v_flex()
                    .items_center()
                    .justify_center()
                    .gap(px(10.))
                    .py(px(56.))
                    .px(px(24.))
                    .border_1()
                    .border_dashed()
                    .border_color(tokens::border2())
                    .rounded(px(10.))
                    .child(
                        h_flex()
                            .size(px(44.))
                            .rounded(px(12.))
                            .items_center()
                            .justify_center()
                            .bg(tokens::field_select())
                            .border_1()
                            .border_color(tokens::border())
                            .text_color(tokens::text2())
                            .child(icon("flask-conical", 20.)),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                text(14., 19.)
                                    .font_weight(FontWeight::BOLD)
                                    .child("No beta features yet"),
                            )
                            .child(soon_pill("beta-soon")),
                    )
                    .child(
                        text(12.5, 19.)
                            .max_w(px(420.))
                            .text_center()
                            .text_color(tokens::text2())
                            .child(
                                "Experimental features will be listed here, each with its own switch. \
                                 They apply after a restart, and a banner offers to restart once you \
                                 change one.",
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_weight(FontWeight::BOLD)
                            .text_color(tokens::text3())
                            .child("WHEN ONE CHANGES"),
                    )
                    .child(
                        h_flex()
                            .gap(px(12.))
                            .pt(px(10.))
                            .pb(px(10.))
                            .pl(px(14.))
                            .pr(px(12.))
                            .items_center()
                            .border_1()
                            .border_color(banner(tokens::accent_line()))
                            .rounded(px(8.))
                            .bg(banner(tokens::accent_soft()))
                            .child(
                                div()
                                    .text_color(banner(tokens::check_on()))
                                    .child(icon("rotate-cw", 15.)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.))
                                    .text_color(banner(tokens::text()))
                                    .child("1 change applies after a restart."),
                            )
                            .child(
                                h_flex()
                                    .flex_none()
                                    .h(px(28.))
                                    .px(px(12.))
                                    .items_center()
                                    .rounded(px(5.))
                                    .bg(banner(tokens::check_on()))
                                    .text_color(banner(tokens::black()))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::BOLD)
                                    .child("Restart now"),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// `color` as it shows in the banner drawn at 45% over the page: mixed
/// here because GPUI fades each shape on its own, which would let the
/// banner's wash show through its button (see `kit::faded`).
fn banner(color: Rgba) -> Rgba {
    let page = tokens::dock();
    let over = |c: f32, under: f32| c * color.a + under * (1. - color.a);
    let mix = |c: f32, under: f32| 0.45 * over(c, under) + 0.55 * under;
    Rgba {
        r: mix(color.r, page.r),
        g: mix(color.g, page.g),
        b: mix(color.b, page.b),
        a: 1.,
    }
}
