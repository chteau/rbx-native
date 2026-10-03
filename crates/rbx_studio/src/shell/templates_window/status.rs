//! The pieces around the editor: the banner over it, the status line under
//! it (encoding, save state, size against the limit) and the "Shows up as"
//! strip with the menu labels a template gets.

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, icon, mono, text};
use crate::script_templates::MAX_BYTES;
use crate::tokens;

use super::editor::SaveState;

/// One stretch of a banner's sentence.
pub(super) enum Part {
    /// The lead, in text colour at weight 600.
    Lead(&'static str),
    Plain(String),
    /// A file name, in mono 11.5 text colour.
    File(&'static str),
}

/// A 10/12-padded banner: a 14 px glyph beside a sentence. `danger` is the
/// red one the size limit raises; otherwise it's the neutral panel2 note.
pub(super) fn banner(glyph: &'static str, danger: bool, parts: Vec<Part>, window: &Window) -> Div {
    let base = window.text_style().font();
    let mono_font = font(tokens::FONT_FAMILY_MONO);
    let mut sentence = String::new();
    let mut runs = Vec::new();
    for part in parts {
        let (s, font, color) = match part {
            Part::Lead(s) => (
                s.to_owned(),
                Font {
                    weight: FontWeight::SEMIBOLD,
                    ..base.clone()
                },
                tokens::text(),
            ),
            Part::Plain(s) => (s, base.clone(), tokens::text2()),
            Part::File(s) => (s.to_owned(), mono_font.clone(), tokens::text()),
        };
        runs.push(TextRun {
            len: s.len(),
            font,
            color: color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
        sentence.push_str(&s);
    }
    h_flex()
        .flex_none()
        .items_start()
        .gap(px(10.))
        .py(px(10.))
        .px(px(12.))
        .rounded(px(6.))
        .border_1()
        .map(|this| {
            if danger {
                this.bg(Rgba {
                    a: 0.08,
                    ..ui::red()
                })
                .border_color(Rgba {
                    a: 0.22,
                    ..ui::red()
                })
            } else {
                this.bg(ui::panel2()).border_color(tokens::border())
            }
        })
        .child(
            div()
                .flex_none()
                .pt(px(1.5))
                .text_color(if danger { ui::red() } else { tokens::text2() })
                .child(icon(if danger { "circle-alert" } else { glyph }, 14.)),
        )
        .child(
            text(12., 17.)
                .flex_1()
                .min_w_0()
                .child(StyledText::new(sentence).with_runs(runs)),
        )
}

/// `502 B`, or whole KiB from 1 KiB up (rounded up, so a file one byte over
/// the limit never reads as exactly the limit).
pub(super) fn size_label(len: u64) -> String {
    if len < 1024 {
        format!("{len} B")
    } else {
        format!("{} KiB", len.div_ceil(1024))
    }
}

/// The 30 px line under the editor.
pub(super) fn status_line(save: &SaveState, len: u64) -> Div {
    let full = len >= MAX_BYTES;
    let (glyph, label, color): (&'static str, SharedString, Rgba) = match save {
        SaveState::BuiltIn => (
            "info",
            "Built in, nothing on disk yet".into(),
            tokens::text3(),
        ),
        SaveState::Saved => ("check", "Saved".into(), ui::green()),
        SaveState::Saving => ("refresh-cw", "Saving\u{2026}".into(), tokens::text2()),
        SaveState::TooLarge => ("circle-alert", "Not saved".into(), ui::red()),
        SaveState::Failed(err) => (
            "circle-alert",
            format!("Not saved: {err}").into(),
            ui::red(),
        ),
    };
    // The bar never vanishes for a short file: 1.5% is its least.
    let fill = (len as f32 / MAX_BYTES as f32).clamp(0.015, 1.);
    h_flex()
        .flex_none()
        .h(px(30.))
        .gap(px(14.))
        .px(px(4.))
        .items_center()
        .child(
            mono(10.5, 14.)
                .text_color(tokens::text3())
                .child("Luau \u{b7} UTF-8 \u{b7} LF"),
        )
        .child(
            h_flex()
                .min_w_0()
                .gap(px(6.))
                .items_center()
                .text_size(px(11.5))
                .line_height(px(16.))
                .text_color(color)
                .child(div().flex_none().child(icon(glyph, 12.)))
                .child(div().truncate().child(label)),
        )
        .child(div().flex_1())
        .child(
            mono(10.5, 14.)
                .flex_none()
                .text_color(if full { ui::red() } else { tokens::text2() })
                .child(format!("{} of {} KiB", size_label(len), MAX_BYTES / 1024)),
        )
        .child(
            div()
                .flex_none()
                .relative()
                .w(px(72.))
                .h(px(3.))
                .rounded(px(2.))
                .bg(tokens::track())
                .overflow_hidden()
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(72. * fill))
                        .bg(if full { ui::red() } else { ui::accent() }),
                ),
        )
}

/// "Shows up as": the two menu labels an extra template gets, exactly as
/// `menu_bar` and the ribbon's Script menu write them.
pub(super) fn shows_up_as(name: &str, class: &str) -> Div {
    let chip = |label: String| {
        h_flex()
            .flex_none()
            .h(px(24.))
            .px(px(9.))
            .items_center()
            .border_1()
            .border_color(tokens::border2())
            .rounded(px(5.))
            .bg(ui::panel())
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(tokens::text())
            .child(label)
    };
    h_flex()
        .flex_none()
        .gap(px(10.))
        .py(px(10.))
        .px(px(12.))
        .items_center()
        .border_1()
        .border_color(tokens::border())
        .rounded(px(8.))
        .bg(ui::panel2())
        .child(
            text(10.5, 14.)
                .font_weight(FontWeight::BOLD)
                .text_color(tokens::text3())
                .child("SHOWS UP AS"),
        )
        .child(chip(format!("Model \u{203a} Insert {name} ({class})")))
        .child(chip(format!("Script \u{25be} \u{203a} {name} ({class})")))
}

#[cfg(test)]
mod tests {
    use super::size_label;

    #[test]
    fn sizes_read_in_bytes_then_whole_kib_rounded_up() {
        assert_eq!(size_label(502), "502 B");
        assert_eq!(size_label(1024), "1 KiB");
        assert_eq!(size_label(312 * 1024), "312 KiB");
        assert_eq!(size_label(256 * 1024 + 1), "257 KiB");
    }
}
