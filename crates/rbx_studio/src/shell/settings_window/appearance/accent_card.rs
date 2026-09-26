//! Appearance › Accent: the presets and Custom, the colour's hex, a strip of
//! the controls it paints drawn in it, and its checks. While the custom
//! popover is open the strip, the hex and the checks show the colour being
//! picked, not the one on screen.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::accent::{self, PRESETS};
use crate::tokens;

use super::super::kit::{icon, mono, text};
use super::super::SettingsWindow;
use super::picker::Target;
use super::preview::{chips, conic, preview};

impl SettingsWindow {
    pub(super) fn accent_card(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let (current, theme_accent) = {
            let shell = self.shell.read(cx);
            (shell.accent(), shell.theme.palette.color("check_on"))
        };
        let picking = self
            .picker
            .as_ref()
            .filter(|picker| picker.target == Target::Accent)
            .map(|picker| picker.candidate());
        let shown = picking.unwrap_or(current);
        let preset = PRESETS
            .iter()
            .position(|(_, value)| accent::hex(accent::rgb(*value)) == accent::hex(current))
            .filter(|_| picking.is_none());

        let swatches = PRESETS
            .iter()
            .enumerate()
            .map(|(i, (name, value))| {
                let color = accent::rgb(*value);
                let selected = preset == Some(i);
                // The theme's own accent is no override at all.
                let choice = (accent::hex(color) != accent::hex(theme_accent)).then_some(color);
                swatch(
                    ("swatch", i),
                    name,
                    selected,
                    div()
                        .size(px(28.))
                        .rounded_full()
                        .bg(color)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(tokens::black())
                        .when(selected, |this| this.child(icon("check", 14.))),
                )
                .on_click(self.set(move |shell, cx| shell.set_accent(choice, cx)))
            })
            .collect::<Vec<_>>();
        let custom_bounds = self.custom_swatch.clone();
        let custom = swatch(
            "swatch-custom",
            "Custom",
            preset.is_none(),
            if preset.is_none() {
                // Chosen: the custom colour itself, checked like a preset.
                div()
                    .size(px(28.))
                    .rounded_full()
                    .bg(shown)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(tokens::black())
                    .child(icon("check", 14.))
            } else {
                div()
                    .relative()
                    .size(px(28.))
                    .child(img(conic()).size(px(28.)).rounded_full())
                    .child(
                        h_flex()
                            .absolute()
                            .top(px(8.))
                            .left(px(8.))
                            .size(px(12.))
                            .rounded_full()
                            .items_center()
                            .justify_center()
                            .bg(tokens::field_select())
                            .text_color(tokens::text())
                            .child(icon("plus", 9.)),
                    )
            },
        )
        .child(
            canvas(
                move |bounds, _, _| custom_bounds.set(bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            let column = this.custom_swatch.get();
            // Right-aligned with the page's cards, just under the swatch's
            // label.
            let right = window.viewport_size().width - px(40.);
            let anchor = point(right - px(280.), column.bottom() + px(3.));
            let start = this.shell.read(cx).accent();
            this.open_picker(Target::Accent, start, anchor, window, cx);
        }));

        let top = h_flex()
            .items_start()
            .gap(px(16.))
            .pt(px(14.))
            .px(px(16.))
            .pb(px(12.))
            .child(
                v_flex()
                    .w(px(190.))
                    .flex_none()
                    .gap(px(2.))
                    .pt(px(2.))
                    .child(
                        text(12.5, 17.)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Accent colour"),
                    )
                    .child(text(11.5, 16.).text_color(tokens::text2()).child(
                        "Selection, focus, the active tab, toggles and primary buttons. \
                         Its soft and line variants follow it.",
                    ))
                    .child(
                        mono(11., 18.)
                            .mt(px(6.))
                            .text_color(tokens::text2())
                            .child(accent::hex(shown)),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .flex_wrap()
                    .gap_x(px(4.))
                    .gap_y(px(6.))
                    .children(swatches)
                    .child(custom),
            );
        let _ = window;
        v_flex()
            .child(top)
            .child(preview(shown))
            .child(chips(shown))
            .into_any_element()
    }
}

/// A preset's column: the 28 px swatch, ringed when chosen, over its name.
fn swatch(
    id: impl Into<ElementId>,
    name: &'static str,
    selected: bool,
    face: Div,
) -> Stateful<Div> {
    v_flex()
        .id(id.into())
        .relative()
        .w(px(52.))
        .gap(px(6.))
        .items_center()
        .cursor_pointer()
        .child(
            div()
                .relative()
                .size(px(28.))
                // The ring: 2 px of the card, then 2 px of text colour, as
                // circles behind the swatch (a shadow's corners don't round
                // with its spread).
                .when(selected, |this| {
                    this.child(ring(tokens::text(), 4.))
                        .child(ring(tokens::field_select(), 2.))
                })
                .child(face),
        )
        .child(
            text(10.5, 14.)
                .text_color(if selected {
                    tokens::text()
                } else {
                    tokens::text2()
                })
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .child(name),
        )
}

fn ring(color: Rgba, spread: f32) -> Div {
    div()
        .absolute()
        .top(px(-spread))
        .left(px(-spread))
        .size(px(28. + 2. * spread))
        .rounded_full()
        .bg(color)
}
