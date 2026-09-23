//! The "?" button and the "Getting started" popover it opens above
//! itself, caret and all.

use gpui_kit::assets::IconName;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::super::{chrome, Shell};
use super::Layout;

const ARGON_WIKI: &str = "https://argon.wiki";

impl Shell {
    /// The 34×34 "?" and the popover it opens above itself.
    pub(super) fn help_button(&mut self, layout: Layout) -> impl IntoElement {
        let dock_width = self.argon_ui.width.get();
        let geometry = HelpGeometry::for_layout(layout, dock_width);
        let tab = self.tab_order.next();
        Popover::new("argon-help")
            .anchor(Anchor::BottomLeft)
            .appearance(false)
            .trigger(chrome::Trigger::with_open(move |open| {
                h_flex()
                    .id("argon-help-button")
                    .tab_index(tab)
                    .flex_none()
                    .size(px(34.))
                    .items_center()
                    .justify_center()
                    .rounded(tokens::RADIUS)
                    .border_1()
                    .cursor_pointer()
                    .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
                    .tooltip(|window, cx| {
                        super::super::tooltip::text("How to use Argon", window, cx)
                    })
                    .map(|this| {
                        if open {
                            this.bg(tokens::accent_soft())
                                .border_color(tokens::accent_line())
                                .text_color(tokens::check_on())
                        } else {
                            this.border_color(tokens::border())
                                .text_color(tokens::text2())
                                .hover(|this| this.bg(tokens::hover()).text_color(tokens::text()))
                        }
                    })
                    .child(Icon::new(IconName::CircleQuestionMark).size(px(15.)))
            }))
            .content(move |_, _, cx| help_popover(geometry, cx))
    }
}

/// Where the help popover sits relative to its trigger: the popover's
/// left edge is 23px left of the "?" in the wide layout (its 400px fit
/// beside the 440px column), and 12px in from the dock's edge below it;
/// the caret always points at the "?". The dock's own width sets the
/// stacked offsets, since the field before the "?" stretches with it.
#[derive(Clone, Copy)]
struct HelpGeometry {
    width: f32,
    left_offset: f32,
    caret_left: f32,
}

impl HelpGeometry {
    fn for_layout(layout: Layout, dock_width: f32) -> Self {
        let (width, button_left, popover_left) = if layout.wide {
            (400., 20. + 294. + 8., 299.)
        } else {
            let button_left = if layout.wrap_actions {
                dock_width - 14. - 34.
            } else {
                dock_width - 14. - 96. - 8. - 34.
            };
            (400f32.min(dock_width - 24.), button_left, 12.)
        };
        HelpGeometry {
            width,
            left_offset: popover_left - button_left,
            caret_left: button_left + 17. - popover_left - 7.,
        }
    }
}

/// The popover's surface with the four "Getting started" steps and a
/// caret under it pointing at "?".
fn help_popover(geometry: HelpGeometry, cx: &mut App) -> AnyElement {
    // A step's body: a wrapping row of words and command chips.
    let paragraph = |pieces: Vec<AnyElement>| {
        div()
            .w_full()
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .text_color(tokens::text2())
            .flex()
            .flex_wrap()
            .items_center()
            .children(pieces)
    };
    let step = |number: &'static str, title: &'static str, body: Vec<AnyElement>| {
        h_flex()
            .items_start()
            .gap(px(10.))
            .child(
                div()
                    .flex_none()
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(tokens::RADIUS)
                    .bg(tokens::dock())
                    .border_1()
                    .border_color(tokens::border2())
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_xs())
                    .line_height(tokens::line_xs())
                    .text_color(tokens::text2())
                    .child(number),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .pt(px(2.))
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(tokens::text_md())
                            .line_height(tokens::line_md())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                            .text_color(tokens::text())
                            .child(title),
                    )
                    .child(paragraph(body)),
            )
    };
    // Text goes in as one element per word (each keeping its trailing
    // space), so a wrapping row breaks between words and never starts a
    // line with a space.
    let words = |s: &'static str, color: Option<Rgba>| -> Vec<AnyElement> {
        let mut out = Vec::new();
        let mut rest = s;
        while !rest.is_empty() {
            let end = rest.find(' ').map_or(rest.len(), |i| i + 1);
            let (word, tail) = rest.split_at(end);
            out.push(
                div()
                    .when_some(color, |this, color| this.text_color(color))
                    .child(word)
                    .into_any_element(),
            );
            rest = tail;
        }
        out
    };
    let text = |s: &'static str| words(s, None);
    let strong = |s: &'static str| words(s, Some(tokens::text()));
    // The chip's borders sit outside the 16px line, as an inline box's
    // would: 18 tall on screen, 16 in the layout.
    let chip = |s: &'static str| {
        h_flex()
            .my(px(-1.))
            .px(px(5.))
            .rounded(tokens::RADIUS_BADGE)
            .bg(tokens::dock())
            .border_1()
            .border_color(tokens::border())
            .font_family(tokens::FONT_FAMILY_MONO)
            .text_size(tokens::text_badge())
            .line_height(tokens::line_badge())
            .text_color(tokens::text())
            .child(s)
            .into_any_element()
    };
    let _ = cx;

    let surface = v_flex()
        .w(px(geometry.width))
        .p(px(16.))
        .gap(px(14.))
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border2())
        .rounded(tokens::RADIUS_CONTAINER)
        .shadow(vec![tokens::floating_shadow()])
        .child(
            h_flex()
                .h(px(18.))
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(tokens::text_lg())
                        .line_height(tokens::line_lg())
                        .font_weight(tokens::WEIGHT_BOLD)
                        .text_color(tokens::text())
                        .child("Getting started"),
                )
                .child(
                    h_flex()
                        .id("argon-wiki")
                        .items_center()
                        .gap(px(5.))
                        .cursor_pointer()
                        .text_size(tokens::text_sm())
                        .line_height(tokens::line_sm())
                        .font_weight(tokens::WEIGHT_SEMIBOLD)
                        .text_color(tokens::check_on())
                        .hover(|this| this.text_color(tokens::text()))
                        .on_click(|_, _, cx| cx.open_url(ARGON_WIKI))
                        .child("argon.wiki")
                        .child(Icon::new(IconName::ExternalLink).size(px(12.))),
                ),
        )
        .child(
            v_flex()
                .gap(px(12.))
                .child(step(
                    "1",
                    "Set up the project",
                    [
                        text("Run "),
                        vec![chip("argon init")],
                        text(", or "),
                        strong("Argon: Initialize Project "),
                        text("from the VS Code command palette."),
                    ]
                    .into_iter()
                    .flatten()
                    .collect(),
                ))
                .child(step(
                    "2",
                    "Start the server",
                    [
                        text("Run "),
                        vec![chip("argon run")],
                        text(", or "),
                        strong("Argon: Start Server "),
                        text("in VS Code."),
                    ]
                    .into_iter()
                    .flatten()
                    .collect(),
                ))
                .child(step(
                    "3",
                    "Connect",
                    text("Check that the host and port match the server, then press Connect."),
                ))
                .child(step(
                    "4",
                    "Sync",
                    text(
                        "Save your files to see changes in Studio. Turn on Two-Way Sync to send Studio edits back to disk.",
                    ),
                )),
        );

    div()
        .relative()
        .ml(px(geometry.left_offset))
        .pb(px(4.))
        .child(surface)
        .child(caret(geometry.caret_left))
        .into_any_element()
}

/// A down-pointing caret under the popover's bottom edge: a rotated-square
/// caret, drawn as a `border2` triangle with a `panel2` one inside it,
/// because gpui can't rotate a div.
fn caret(left: f32) -> Div {
    const OUTER: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 7" width="14" height="7"><path d="M0 0h14L7 7z" fill="currentColor"/></svg>"#;
    const INNER: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 6" width="12" height="6"><path d="M0 0h12L6 6z" fill="currentColor"/></svg>"#;
    div()
        .absolute()
        .left(px(left))
        .bottom(px(-3.))
        .w(px(14.))
        .h(px(7.))
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .text_color(tokens::border2())
                .child(Icon::empty().data(OUTER).size(px(14.))),
        )
        .child(
            div()
                .absolute()
                .left(px(1.))
                .top_0()
                .text_color(tokens::field_select())
                .child(Icon::empty().data(INNER).size(px(12.))),
        )
}
