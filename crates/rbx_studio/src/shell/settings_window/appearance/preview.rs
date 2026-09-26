//! The accent card's lower half: the controls the accent paints, drawn in
//! it, and its checks as chips; and the Custom swatch's colour wheel.

use std::sync::{Arc, OnceLock};

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::accent;
use crate::tokens;

use super::super::kit::{icon, mono, text};

/// `color` at `alpha`.
fn at(color: Rgba, alpha: f32) -> Rgba {
    Rgba { a: alpha, ..color }
}

fn glow(color: Rgba, spread: f32) -> BoxShadow {
    BoxShadow {
        color: color.into(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(spread),
        inset: false,
    }
}

/// The controls the accent paints, drawn in `accent`, each over its name.
pub(super) fn preview(accent: Rgba) -> Div {
    let soft = at(accent, 0.12);
    let line = at(accent, 0.55);
    let cell = |stage: AnyElement, caption: &'static str| {
        v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(8.))
            .child(h_flex().h(px(44.)).items_center().child(stage))
            .child(text(10.5, 14.).text_color(tokens::text3()).child(caption))
    };
    let row = |label: &'static str, selected: bool| {
        h_flex()
            .relative()
            .h(px(20.))
            .px(px(8.))
            .items_center()
            .rounded(px(4.))
            .text_size(px(11.))
            .map(|this| {
                if selected {
                    this.bg(soft).text_color(tokens::text()).child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(2.))
                            .bg(accent),
                    )
                } else {
                    this.text_color(tokens::text2())
                }
            })
            .child(label)
    };
    h_flex()
        .gap(px(18.))
        .py(px(12.))
        .px(px(16.))
        .bg(tokens::dock())
        .child(cell(
            h_flex()
                .h(px(28.))
                .px(px(12.))
                .items_center()
                .rounded(px(5.))
                .bg(accent)
                .text_color(tokens::black())
                .text_size(px(12.))
                .font_weight(FontWeight::BOLD)
                .child("Publish")
                .into_any_element(),
            "Primary button",
        ))
        .child(cell(
            v_flex()
                .w(px(120.))
                .gap(px(2.))
                .child(row("Workspace", false))
                .child(row("Baseplate", true))
                .into_any_element(),
            "Selection",
        ))
        .child(cell(
            h_flex()
                .gap(px(14.))
                .text_size(px(11.5))
                .child(
                    div()
                        .pb(px(6.))
                        .border_b_2()
                        .border_color(accent)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Home"),
                )
                .child(div().pb(px(6.)).text_color(tokens::text2()).child("Model"))
                .into_any_element(),
            "Active tab",
        ))
        .child(cell(
            h_flex()
                .w(px(96.))
                .h(px(26.))
                .px(px(8.))
                .items_center()
                .border_1()
                .border_color(line)
                .rounded(px(5.))
                .shadow(vec![glow(soft, 3.)])
                .child(mono(11., 14.).child("1.00"))
                .into_any_element(),
            "Focus",
        ))
        .child(cell(
            div()
                .relative()
                .w(px(32.))
                .h(px(18.))
                .rounded(px(9.))
                .bg(accent)
                .child(
                    div()
                        .absolute()
                        .top(px(3.))
                        .left(px(17.))
                        .size(px(12.))
                        .rounded_full()
                        .bg(tokens::black()),
                )
                .into_any_element(),
            "Toggle",
        ))
        .child(cell(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(accent)
                .child("Manage key")
                .into_any_element(),
            "Link",
        ))
}

/// The three bars as chips, and whether the hue keeps clear of the status
/// colours.
pub(super) fn chips(accent_color: Rgba) -> Div {
    let chip = |glyph: &'static str,
                glyph_color: Rgba,
                label: String,
                ratio: Option<(String, Rgba)>| {
        h_flex()
            .h(px(24.))
            .px(px(8.))
            .gap(px(6.))
            .items_center()
            .border_1()
            .border_color(tokens::border())
            .rounded(px(12.))
            .bg(tokens::dock())
            .text_size(px(11.))
            .line_height(px(14.))
            .text_color(tokens::text2())
            .child(div().text_color(glyph_color).child(icon(glyph, 11.)))
            .child(label)
            .children(ratio.map(|(ratio, color)| mono(10.5, 14.).text_color(color).child(ratio)))
    };
    let checks = accent::checks(accent_color).map(|check| {
        let color = if check.passes() {
            tokens::diff_add()
        } else {
            tokens::text_error()
        };
        chip(
            if check.passes() { "check" } else { "x" },
            color,
            check.label.to_owned(),
            Some((format!("{:.1}:1", check.ratio), color)),
        )
    });
    let status = match accent::near_status(accent_color) {
        Some(status) => chip(
            "triangle-alert",
            tokens::warning(),
            format!("Close to the {}", status.name()),
            None,
        ),
        None => chip(
            "check",
            tokens::diff_add(),
            "Apart from status colours".to_owned(),
            None,
        ),
    };
    h_flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.))
        .pt(px(10.))
        .px(px(16.))
        .pb(px(12.))
        .border_t_1()
        .border_color(tokens::border())
        .children(checks)
        .child(status)
}

/// The Custom swatch's colour wheel: a conic gradient through the status
/// and accent hues, which GPUI has no fill for, so it is drawn once into an
/// image at twice its size.
pub(super) fn conic() -> Arc<RenderImage> {
    static IMAGE: OnceLock<Arc<RenderImage>> = OnceLock::new();
    IMAGE
        .get_or_init(|| {
            const STOPS: [u32; 7] = [
                0xE06C6C, 0xD9A55B, 0x74C98F, 0x3FB3C9, 0x6C7FDB, 0xC877D6, 0xE06C6C,
            ];
            const SIZE: u32 = 56;
            let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
            let centre = SIZE as f32 / 2.;
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let (dx, dy) = (x as f32 + 0.5 - centre, y as f32 + 0.5 - centre);
                    // Clockwise from the top, as CSS's `from 0deg`.
                    let turn = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.);
                    let at = turn * (STOPS.len() - 1) as f32;
                    let i = (at.floor() as usize).min(STOPS.len() - 2);
                    let (a, b) = (accent::rgb(STOPS[i]), accent::rgb(STOPS[i + 1]));
                    let t = at - i as f32;
                    let mix = |a: f32, b: f32| ((a + (b - a) * t) * 255.).round() as u8;
                    // An antialiased disc.
                    let edge = (centre - (dx * dx + dy * dy).sqrt()).clamp(0., 1.);
                    pixels.extend([
                        mix(a.r, b.r),
                        mix(a.g, b.g),
                        mix(a.b, b.b),
                        (edge * 255.) as u8,
                    ]);
                }
            }
            crate::render_image::to_render_image(pixels, SIZE, SIZE)
                .expect("a 56 px image always converts")
        })
        .clone()
}
