//! Appearance › Theme's three cards, each a small window drawn in its
//! theme's palette: the three built-in themes, Dark soft (Default), High
//! contrast and Light.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme;
use crate::tokens;

use super::super::kit::{icon, text};
use super::super::SettingsWindow;

/// A card's small window, read from its theme's palette: `dock`,
/// `black`, `border`, `text2`, `text` and `field_select`.
struct Mini {
    body: Rgba,
    chrome: Rgba,
    /// The window's lines, with their alpha.
    line: Rgba,
    muted: Rgba,
    title: Rgba,
    field: Rgba,
}

fn mini(id: &str) -> Mini {
    let palette = theme::ThemePack::embedded(id)
        .expect("built-in themes always load")
        .palette;
    Mini {
        body: palette.color("dock"),
        chrome: palette.color("black"),
        line: palette.color("border"),
        muted: palette.color("text2"),
        title: palette.color("text"),
        field: palette.color("field_select"),
    }
}

/// Dark soft, High contrast, Light; parsed once rather than every frame.
static CARDS: std::sync::LazyLock<[Mini; 3]> = std::sync::LazyLock::new(|| {
    [
        mini(theme::DEFAULT_ID),
        mini(theme::HIGH_CONTRAST_ID),
        mini(theme::LIGHT_ID),
    ]
});

/// `color` over `under`, both straight.
fn over(color: Rgba, under: Rgba) -> Rgba {
    let mix = |c: f32, u: f32| c * color.a + u * (1. - color.a);
    Rgba {
        r: mix(color.r, under.r),
        g: mix(color.g, under.g),
        b: mix(color.b, under.b),
        a: 1.,
    }
}

impl SettingsWindow {
    pub(super) fn theme_cards(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.shell.read(cx).appearance.theme.clone();
        let current = current.as_deref().unwrap_or(theme::DEFAULT_ID);
        let cards = [
            ("Dark soft", "Built-in", &CARDS[0], theme::DEFAULT_ID),
            (
                "High contrast",
                "7:1 text, AAA",
                &CARDS[1],
                theme::HIGH_CONTRAST_ID,
            ),
            ("Light", "Built-in", &CARDS[2], theme::LIGHT_ID),
        ];
        h_flex()
            .gap(px(10.))
            .py(px(14.))
            .px(px(16.))
            .children(cards.into_iter().map(|(name, sub, mini, id)| {
                card(name, sub, mini, id == current)
                    .id(SharedString::from(format!("theme-{id}")))
                    .cursor_pointer()
                    .on_click(self.set(move |shell, cx| shell.pick_theme(id, cx)))
            }))
            .into_any_element()
    }
}

/// A theme's card.
fn card(name: &'static str, sub: &'static str, mini: &Mini, selected: bool) -> Div {
    let accent = tokens::check_on();
    let body = mini.body;
    let chrome = mini.chrome;
    let bar = |color: Rgba| div().h(px(4.)).rounded(px(2.)).bg(color);
    let muted = over(
        Rgba {
            a: 0.6,
            ..mini.muted
        },
        chrome,
    );
    let window = v_flex()
        .h(px(84.))
        .rounded(px(6.))
        .overflow_hidden()
        .bg(body)
        .border_1()
        .border_color(over(mini.line, body))
        .child(
            div()
                .h(px(12.))
                .flex_none()
                .bg(chrome)
                .border_b_1()
                .border_color(over(mini.line, chrome)),
        )
        .child(
            h_flex()
                .flex_1()
                .items_stretch()
                .child(
                    v_flex()
                        .w(px(36.))
                        .flex_none()
                        .gap(px(4.))
                        .py(px(6.))
                        .px(px(5.))
                        .bg(chrome)
                        .border_r_1()
                        .border_color(over(mini.line, chrome))
                        .child(bar(muted))
                        .child(bar(accent))
                        .child(bar(muted)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .p(px(7.))
                        .gap(px(5.))
                        .child(
                            div()
                                .w(relative(0.6))
                                .h(px(5.))
                                .rounded(px(2.))
                                .bg(mini.title),
                        )
                        .child(
                            div()
                                .h(px(24.))
                                .rounded(px(4.))
                                .bg(mini.field)
                                .border_1()
                                .border_color(over(mini.line, mini.field)),
                        )
                        .child(div().w(px(34.)).h(px(9.)).rounded(px(3.)).bg(accent)),
                ),
        );
    let footer = h_flex()
        .gap(px(6.))
        .px(px(2.))
        .items_center()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    text(12., 16.)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens::text())
                        .child(name),
                )
                .child(text(10.5, 14.).text_color(tokens::text2()).child(sub)),
        )
        .when(selected, |this| {
            this.child(div().text_color(accent).child(icon("check", 14.)))
        });
    v_flex()
        .flex_1()
        .min_w_0()
        .gap(px(8.))
        .p(px(8.))
        .rounded(px(8.))
        .border_1()
        .map(|this| {
            if selected {
                this.border_color(tokens::accent_line())
                    .bg(tokens::accent_soft())
            } else {
                this.border_color(over(tokens::border(), tokens::dock()))
                    .bg(tokens::dock())
            }
        })
        .child(window)
        .child(footer)
}

#[cfg(test)]
mod tests {
    use super::CARDS;
    use crate::theme;

    #[test]
    fn theme_cards_paint_each_built_in_themes_own_palette() {
        let ids = [theme::DEFAULT_ID, theme::HIGH_CONTRAST_ID, theme::LIGHT_ID];
        for (index, id) in ids.into_iter().enumerate() {
            let palette = theme::ThemePack::embedded(id).unwrap().palette;
            assert_eq!(CARDS[index].body, palette.color("dock"), "{id} body");
            assert_eq!(
                CARDS[index].field,
                palette.color("field_select"),
                "{id} field"
            );
            assert_eq!(CARDS[index].title, palette.color("text"), "{id} title");
        }
        assert_ne!(CARDS[2].body, CARDS[0].body);
    }
}
