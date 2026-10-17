//! The on-screen keyboard cover: a picture of the iPhone's dark system
//! keyboard drawn over the bottom of the simulated screen, so a layout can
//! be judged against the space a real keyboard takes. The game is never
//! told it is there, and its keys type nothing; only the page keys (123,
//! #+=, ABC, the emoji key), shift and the globe, which switches between
//! QWERTY and AZERTY, do anything.
//!
//! Everything is laid out in device points, as iOS sizes its keys, then
//! scaled by the cover's pixels per point.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::{ViewportAction, WorkspaceView};
use crate::device_sim::KeyboardLayout;

/// The keyboard's page, kept by the view while the cover shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    /// Letters; `shift` shows them capitalised, as a field's first letter.
    Letters {
        shift: bool,
    },
    /// Digits and common punctuation (the 123 key).
    Numbers,
    /// Brackets and the rest (the #+= key).
    Symbols,
    Emoji,
}

impl Default for Page {
    fn default() -> Self {
        Page::Letters { shift: true }
    }
}

/// Where the cover goes, in the overlay's logical pixels, and the height in
/// device points it stands for.
pub(super) struct Placement {
    pub(super) left: Pixels,
    pub(super) top: Pixels,
    pub(super) width: Pixels,
    pub(super) height: Pixels,
    pub(super) points: u32,
}

// iOS's dark keyboard.
const BACKGROUND: u32 = 0x232425;
const KEY: u32 = 0x646567;
const SPECIAL: u32 = 0x404143;
const SHIFT_ON: u32 = 0xd8d9db;
const SHADOW: u32 = 0x101112;
const LABEL: u32 = 0xffffff;
const FAINT: u32 = 0x8e8e93;

fn letter_rows(layout: KeyboardLayout) -> [&'static [&'static str]; 3] {
    match layout {
        KeyboardLayout::Qwerty => [
            &["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"],
            &["a", "s", "d", "f", "g", "h", "j", "k", "l"],
            &["z", "x", "c", "v", "b", "n", "m"],
        ],
        // iOS's French AZERTY: the apostrophe takes the seventh key of the
        // bottom row, M moves up beside L.
        KeyboardLayout::Azerty => [
            &["a", "z", "e", "r", "t", "y", "u", "i", "o", "p"],
            &["q", "s", "d", "f", "g", "h", "j", "k", "l", "m"],
            &["w", "x", "c", "v", "b", "n", "\u{2019}"],
        ],
    }
}

fn number_rows(layout: KeyboardLayout) -> [&'static [&'static str]; 3] {
    let second: &'static [&'static str] = match layout {
        KeyboardLayout::Qwerty => &["-", "/", ":", ";", "(", ")", "$", "&", "@", "\""],
        KeyboardLayout::Azerty => &["-", "/", ":", ";", "(", ")", "\u{20ac}", "&", "@", "\""],
    };
    [
        &["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
        second,
        &[".", ",", "?", "!", "\u{2019}"],
    ]
}

fn symbol_rows(layout: KeyboardLayout) -> [&'static [&'static str]; 3] {
    let second: &'static [&'static str] = match layout {
        KeyboardLayout::Qwerty => &[
            "_", "\\", "|", "~", "<", ">", "\u{20ac}", "\u{a3}", "\u{a5}", "\u{2022}",
        ],
        KeyboardLayout::Azerty => &[
            "_", "\\", "|", "~", "<", ">", "$", "\u{a3}", "\u{a5}", "\u{2022}",
        ],
    };
    [
        &["[", "]", "{", "}", "#", "%", "^", "*", "+", "="],
        second,
        &[".", ",", "?", "!", "\u{2019}"],
    ]
}

/// The "frequently used" page iOS opens on, as Twemoji file names (the
/// code point in hex): drawn as images, as the system's emoji font is only
/// reached through GPUI's fallback, which takes DejaVu Sans's monochrome
/// faces first on Linux.
const EMOJI: &[&str] = &[
    "1f602", "2764", "1f60d", "1f923", "1f60a", "1f64f", "1f495", "1f62d", "1f618", "1f44d",
    "1f605", "1f44f", "1f601", "1f525", "1f970", "1f494", "1f496", "1f499", "1f622", "1f914",
    "1f606", "1f644", "1f4aa", "1f609", "263a", "1f44c", "1f917", "1f49c", "1f614", "1f60e",
    "1f607", "1f339", "1f926", "1f389", "1f49e", "270c", "2728", "1f937", "1f631", "1f60c",
    "1f338", "1f64c", "1f60b", "1f497", "1f49a", "1f60f", "1f49b", "1f642", "1f493", "1f929",
    "1f604", "1f600", "1f5a4", "1f603", "1f4af", "1f648", "1f447", "1f3b6", "1f612", "1f92d",
];

/// The emoji page's category bar, after ABC.
const CATEGORIES: &[&str] = &[
    "1f552", "1f600", "1f43b", "1f354", "26bd", "1f697", "1f4a1", "1f523", "1f3f3",
];

#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../assets/twemoji"]
#[include = "*.svg"]
struct Twemoji;

/// Rasterized once per name, at a size that stays sharp on the largest
/// cover (an iPad's, on a 2x display).
fn twemoji(name: &str) -> Option<Arc<RenderImage>> {
    thread_local! {
        static CACHE: RefCell<HashMap<String, Option<Arc<RenderImage>>>> =
            RefCell::new(HashMap::new());
    }
    CACHE.with_borrow_mut(|cache| {
        cache
            .entry(name.to_owned())
            .or_insert_with(|| {
                let file = Twemoji::get(&format!("{name}.svg"))?;
                crate::class_icons::rasterize_at(&file.data, 96)
            })
            .clone()
    })
}

fn emoji(name: &str, size: Pixels) -> Div {
    div()
        .size(size)
        .flex_none()
        .when_some(twemoji(name), |this, image| {
            this.child(img(image).size_full())
        })
}

/// Key sizes in device points, from the cover's height: iOS's portrait
/// keyboard (336 pt with the suggestion bar, the globe and dictation row and
/// the home indicator) keeps 42 pt keys; a short landscape one shrinks them.
struct Metrics {
    /// Pixels per point.
    scale: f32,
    /// The screen's width in points.
    width: f32,
    /// The suggestion bar above the keys.
    suggest: f32,
    top_pad: f32,
    key_h: f32,
    row_gap: f32,
    key_gap: f32,
    /// Left and right of the key rows.
    margin: f32,
    /// A letter key's width.
    key_w: f32,
    /// Shift, delete, 123, the emoji key.
    side_w: f32,
    /// Return.
    return_w: f32,
    /// The globe and dictation row with the home indicator, below the keys
    /// in portrait; only the home indicator in landscape.
    strip: f32,
    tall: bool,
}

impl Metrics {
    fn new(place: &Placement) -> Self {
        let points = place.points.max(1) as f32;
        let scale = f32::from(place.height) / points;
        let width = f32::from(place.width) / scale.max(f32::EPSILON);
        let tall = points >= 280.0;
        let (suggest, top_pad, row_gap) = match tall {
            true => (44.0, 8.0, 12.0),
            false => (36.0, 5.0, 6.0),
        };
        let key_h = match tall {
            true => 42f32.min((points - suggest - top_pad - 3.0 * row_gap - 44.0) / 4.0),
            false => (points - suggest - top_pad - 3.0 * row_gap - 13.0) / 4.0,
        }
        .max(1.0);
        let strip = points - suggest - top_pad - 4.0 * key_h - 3.0 * row_gap;
        let key_gap = 6.0;
        // Landscape iPhones keep the keys clear of the rounded corners.
        let margin = match tall {
            true => 3.0,
            false => 3.0 + width * 0.06,
        };
        let key_w = ((width - 2.0 * margin - 9.0 * key_gap) / 10.0).max(1.0);
        Metrics {
            scale,
            width,
            suggest,
            top_pad,
            key_h,
            row_gap,
            key_gap,
            margin,
            key_w,
            side_w: key_w * 1.3,
            return_w: key_w * 2.75,
            strip,
            tall,
        }
    }

    fn px(&self, points: f32) -> Pixels {
        px(points * self.scale)
    }

    /// The width a row of five wide middle keys gets on the 123 and #+=
    /// pages, where shift's and delete's places frame them as on the
    /// letters page.
    fn wide_key_w(&self) -> f32 {
        let inner = self.width - 2.0 * self.margin - 2.0 * self.side_w;
        let letters_gap = (inner - 7.0 * self.key_w - 6.0 * self.key_gap) / 2.0;
        (inner - 2.0 * letters_gap - 4.0 * self.key_gap) / 5.0
    }
}

fn hex(color: u32) -> Hsla {
    rgb(color).into()
}

/// A key with the bottom shadow iOS draws under each.
fn key(m: &Metrics, width: Option<f32>, fill: u32) -> Div {
    div()
        .h(m.px(m.key_h))
        .when_some(width, |this, w| this.w(m.px(w)).flex_none())
        .when(width.is_none(), |this| this.flex_1())
        .flex()
        .items_center()
        .justify_center()
        .bg(hex(fill))
        .rounded(m.px(5.0))
        .shadow(vec![BoxShadow {
            color: hex(SHADOW).opacity(0.9),
            offset: point(px(0.), m.px(1.0)),
            blur_radius: px(0.),
            spread_radius: px(0.),
            inset: false,
        }])
        .text_color(hex(LABEL))
}

fn text_key(m: &Metrics, width: Option<f32>, fill: u32, label: &str, size: f32) -> Div {
    key(m, width, fill)
        .text_size(m.px(size))
        .child(SharedString::from(label.to_owned()))
}

fn icon_key(m: &Metrics, width: f32, fill: u32, icon: IconName, tint: u32) -> Div {
    key(m, Some(width), fill).child(
        Icon::new(icon)
            .size(m.px(m.key_h * 0.5))
            .text_color(hex(tint)),
    )
}

fn row(m: &Metrics) -> Div {
    div()
        .flex()
        .flex_row()
        .flex_none()
        .w_full()
        .h(m.px(m.key_h))
        .px(m.px(m.margin))
        .gap(m.px(m.key_gap))
}

/// Switches the cover's page on a press.
fn turns_to(this: Div, page: Page, cx: &mut Context<WorkspaceView>) -> Div {
    this.cursor_pointer().on_mouse_down(
        MouseButton::Left,
        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
            view.keyboard_page = page;
            cx.stop_propagation();
            cx.notify();
        }),
    )
}

pub(super) fn cover(
    place: Placement,
    layout: KeyboardLayout,
    page: Page,
    occlude: bool,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let m = Metrics::new(&place);
    let body = match page {
        Page::Emoji => emoji_page(&m, layout, cx),
        _ => key_page(&m, layout, page, cx),
    };
    let note = div()
        .h(m.px(m.suggest))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .text_size(m.px(11.0))
        .text_color(hex(FAINT))
        .child("Keyboard cover \u{2014} not reported to the game");
    div()
        .absolute()
        .left(place.left)
        .top(place.top)
        .w(place.width)
        .h(place.height)
        .flex()
        .flex_col()
        .bg(hex(BACKGROUND))
        .overflow_hidden()
        // Nothing under the cover is the scene's to pick.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .when(page != Page::Emoji, |this| this.child(note))
        .child(body)
        .child(strip(&m, layout, cx))
        .when(occlude, |this| this.child(gpui_base::occlusion::probe()))
}

fn key_page(
    m: &Metrics,
    layout: KeyboardLayout,
    page: Page,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let letter = m.key_h * 0.54;
    let word = m.key_h * 0.38;
    let shift = match page {
        Page::Letters { shift } => Some(shift),
        _ => None,
    };
    let rows = match page {
        Page::Letters { .. } => letter_rows(layout),
        Page::Numbers => number_rows(layout),
        _ => symbol_rows(layout),
    };
    let cap = |label: &str| match shift {
        Some(true) => label.to_uppercase(),
        _ => label.to_owned(),
    };
    let first = row(m).children(
        rows[0]
            .iter()
            .map(|&label| text_key(m, Some(m.key_w), KEY, &cap(label), letter)),
    );
    let second = row(m).justify_center().children(
        rows[1]
            .iter()
            .map(|&label| text_key(m, Some(m.key_w), KEY, &cap(label), letter)),
    );
    let middle_w = match shift {
        Some(_) => m.key_w,
        None => m.wide_key_w(),
    };
    let toggle = match page {
        Page::Letters { shift: on } => turns_to(
            icon_key(
                m,
                m.side_w,
                if on { SHIFT_ON } else { SPECIAL },
                IconName::ArrowBigUp,
                if on { 0x000000 } else { LABEL },
            ),
            Page::Letters { shift: !on },
            cx,
        ),
        Page::Numbers => turns_to(
            text_key(m, Some(m.side_w), SPECIAL, "#+=", word),
            Page::Symbols,
            cx,
        ),
        _ => turns_to(
            text_key(m, Some(m.side_w), SPECIAL, "123", word),
            Page::Numbers,
            cx,
        ),
    };
    let third = row(m)
        .child(toggle)
        .child(
            div()
                .flex_1()
                .flex()
                .flex_row()
                .justify_center()
                .gap(m.px(m.key_gap))
                .children(
                    rows[2]
                        .iter()
                        .map(|&label| text_key(m, Some(middle_w), KEY, &cap(label), letter)),
                ),
        )
        .child(icon_key(m, m.side_w, SPECIAL, IconName::Delete, LABEL));
    let (pages_label, pages_to) = match page {
        Page::Letters { .. } => ("123", Page::Numbers),
        _ => ("ABC", Page::Letters { shift: false }),
    };
    let (space, enter) = match layout {
        KeyboardLayout::Qwerty => ("space", "return"),
        KeyboardLayout::Azerty => ("espace", "retour"),
    };
    let bottom = row(m)
        .child(turns_to(
            text_key(m, Some(m.side_w), SPECIAL, pages_label, word),
            pages_to,
            cx,
        ))
        .child(turns_to(
            icon_key(m, m.side_w, SPECIAL, IconName::FaceGrinning, LABEL),
            Page::Emoji,
            cx,
        ))
        .child(text_key(m, None, KEY, space, word))
        .child(text_key(m, Some(m.return_w), SPECIAL, enter, word));
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(m.px(m.top_pad))
        .gap(m.px(m.row_gap))
        .child(first)
        .child(second)
        .child(third)
        .child(bottom)
}

fn emoji_page(m: &Metrics, layout: KeyboardLayout, cx: &mut Context<WorkspaceView>) -> Div {
    let header = match layout {
        KeyboardLayout::Qwerty => "FREQUENTLY USED",
        KeyboardLayout::Azerty => "UTILIS\u{c9}S FR\u{c9}QUEMMENT",
    };
    // The page fills what the suggestion bar and the four key rows take.
    let height = m.suggest + m.top_pad + 4.0 * m.key_h + 3.0 * m.row_gap;
    let bar = m.key_h.min(36.0);
    let title = 24.0;
    let cell = 42f32.min((height - bar - title) / 5.0).max(1.0);
    let rows = ((height - bar - title) / cell).floor().max(1.0) as usize;
    let columns = ((m.width - 2.0 * m.margin) / cell).floor().max(1.0) as usize;
    // iOS fills the grid column by column, scrolling sideways.
    let grid = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .px(m.px(m.margin))
        .children((0..rows).map(|r| {
            div()
                .flex()
                .flex_row()
                .h(m.px(cell))
                .children((0..columns).map(move |c| {
                    let name = EMOJI.get(c * rows + r).copied();
                    div()
                        .w(m.px(cell))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when_some(name, |this, name| {
                            this.child(emoji(name, m.px(cell * 0.78)))
                        })
                }))
        }));
    let categories = div()
        .h(m.px(bar))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .px(m.px(m.margin + 6.0))
        .gap(m.px(4.0))
        .child(turns_to(
            div()
                .flex_none()
                .pr(m.px(6.0))
                .text_size(m.px(15.0))
                .text_color(hex(LABEL))
                .child("ABC"),
            Page::Letters { shift: false },
            cx,
        ))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_row()
                .justify_between()
                .opacity(0.55)
                .children(CATEGORIES.iter().map(|&name| emoji(name, m.px(bar * 0.5)))),
        )
        .child(
            div().pl(m.px(6.0)).child(
                Icon::new(IconName::Delete)
                    .size(m.px(bar * 0.6))
                    .text_color(hex(LABEL)),
            ),
        );
    div()
        .h(m.px(height))
        .flex_none()
        .flex()
        .flex_col()
        .child(
            div()
                .h(m.px(title))
                .flex_none()
                .flex()
                .items_end()
                .px(m.px(m.margin + 6.0))
                .text_size(m.px(12.0))
                .text_color(hex(FAINT))
                .child(header),
        )
        .child(grid)
        .child(categories)
}

/// The row under the keys: the globe, which switches layout, and dictation
/// in portrait; the home indicator in either.
fn strip(m: &Metrics, layout: KeyboardLayout, cx: &mut Context<WorkspaceView>) -> Div {
    let indicator = div()
        .absolute()
        .bottom(m.px(8.0))
        .left(m.px((m.width - 134.0) / 2.0))
        .w(m.px(134.0))
        .h(m.px(5.0))
        .rounded(m.px(2.5))
        .bg(hex(LABEL));
    let icon = |name: IconName| Icon::new(name).size(m.px(28.0)).text_color(hex(LABEL));
    let next = layout.next();
    div()
        .relative()
        .flex_1()
        .min_h(m.px(m.strip))
        .when(m.tall, |this| {
            this.child(
                div()
                    .absolute()
                    .top(m.px((m.strip - 13.0 - 28.0) / 2.0))
                    .left(m.px(m.margin + m.key_w * 0.35))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            cx.emit(ViewportAction::KeyboardLayout(next));
                        }),
                    )
                    .child(icon(IconName::Globe)),
            )
            .child(
                div()
                    .absolute()
                    .top(m.px((m.strip - 13.0 - 28.0) / 2.0))
                    .right(m.px(m.margin + m.key_w * 0.35))
                    .child(icon(IconName::Mic)),
            )
        })
        .child(indicator)
}

#[cfg(test)]
mod tests;
