//! The on-screen keyboard cover: a picture of the device's own dark system
//! keyboard (the iPhone's, the iPad's or Gboard) drawn over the bottom of
//! the simulated screen, so a layout can be judged against the space a real
//! keyboard takes. The game is never told it is there.
//!
//! The page keys (123, #+=, ABC, the emoji key), shift and the globe, which
//! switches between QWERTY and AZERTY, work in Edit too. During Play the
//! character keys, space, delete and return also type into the client
//! (`ViewportAction::KeyboardTap`).
//!
//! Everything is laid out in device points, as the systems size their keys,
//! then scaled by the cover's pixels per point.

mod emoji;
mod fitted;
mod iphone;
mod rows;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::{ViewportAction, WorkspaceView};
use crate::device_sim::KeyboardLayout;

/// The keyboard's page, kept by the view while the cover shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    /// Letters; `shift` capitalises the next one, as for a field's first
    /// letter.
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

/// Which system keyboard the cover draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Style {
    Iphone,
    Ipad,
    /// Gboard, Android's stock keyboard.
    Android,
}

/// What a key typed, for `Shell` to send to the Play client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KeyTap {
    Text(String),
    Backspace,
    Return,
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

/// A keyboard's colours and key shape.
struct Palette {
    background: u32,
    key: u32,
    /// Shift, delete, the page keys, return.
    special: u32,
    shift_on: u32,
    shift_on_label: u32,
    label: u32,
    faint: u32,
    /// A shadow under each key (iOS); Gboard's keys are flat.
    shadow: Option<u32>,
    /// Return drawn as an accent pill with this fill and icon colour
    /// (Gboard), instead of a labelled special key.
    accent: Option<(u32, u32)>,
    radius: f32,
}

/// iOS's dark keyboard.
const IOS: Palette = Palette {
    background: 0x232425,
    key: 0x646567,
    special: 0x404143,
    shift_on: 0xd8d9db,
    shift_on_label: 0x000000,
    label: 0xffffff,
    faint: 0x8e8e93,
    shadow: Some(0x101112),
    accent: None,
    radius: 5.0,
};

/// Gboard's dark theme.
const GBOARD: Palette = Palette {
    background: 0x1e1f22,
    key: 0x3b3c40,
    special: 0x4b4d52,
    shift_on: 0xa8c7fa,
    shift_on_label: 0x062e6f,
    label: 0xe3e3e3,
    faint: 0x9aa0a6,
    shadow: None,
    accent: Some((0xa8c7fa, 0x062e6f)),
    radius: 6.0,
};

/// Key sizes in device points, from the cover's height and the style.
struct Metrics {
    style: Style,
    palette: &'static Palette,
    /// Pixels per point.
    scale: f32,
    /// The screen's width in points.
    width: f32,
    /// The suggestion bar (Gboard: the toolbar) above the keys.
    suggest: f32,
    top_pad: f32,
    key_h: f32,
    row_gap: f32,
    key_gap: f32,
    /// Left and right of the key rows.
    margin: f32,
    /// A letter key's width on a ten-key row.
    key_w: f32,
    /// What is left below the keys: iOS's globe and dictation row and home
    /// indicator, or Android's navigation bar.
    strip: f32,
    tall: bool,
}

impl Metrics {
    fn new(place: &Placement, style: Style) -> Self {
        let points = place.points.max(1) as f32;
        let scale = f32::from(place.height) / points;
        let width = f32::from(place.width) / scale.max(f32::EPSILON);
        let tall = points >= 280.0;
        // (suggestion bar, top pad, row gap, key gap, strip below)
        let (suggest, top_pad, row_gap, key_gap, strip) = match (style, tall) {
            // iOS's portrait keyboard is 336 pt with 42 pt keys; the strip
            // takes what the keys leave.
            (Style::Iphone, true) => (44.0, 8.0, 12.0, 6.0, None),
            (Style::Iphone, false) => (36.0, 5.0, 6.0, 6.0, Some(13.0)),
            // The iPad's home indicator sits over the keyboard's bottom
            // edge; its rows fill the rest.
            (Style::Ipad, _) => (50.0, 6.0, 8.0, 7.0, Some(14.0)),
            // Gboard: a toolbar above and the gesture bar below.
            (Style::Android, true) => (44.0, 4.0, 8.0, 5.0, Some(40.0)),
            (Style::Android, false) => (36.0, 3.0, 5.0, 5.0, Some(20.0)),
        };
        let key_h = match strip {
            Some(strip) => (points - suggest - top_pad - 3.0 * row_gap - strip) / 4.0,
            None => 42f32.min((points - suggest - top_pad - 3.0 * row_gap - 44.0) / 4.0),
        }
        .max(1.0);
        let strip = points - suggest - top_pad - 4.0 * key_h - 3.0 * row_gap;
        let margin = match (style, tall) {
            // Landscape iPhones keep the keys clear of the rounded corners,
            // and put the globe and dictation keys beside them.
            (Style::Iphone, false) => 3.0 + width * 0.06,
            (Style::Ipad, _) => 6.0,
            _ => 3.0,
        };
        let key_w = ((width - 2.0 * margin - 9.0 * key_gap) / 10.0).max(1.0);
        Metrics {
            style,
            palette: match style {
                Style::Android => &GBOARD,
                _ => &IOS,
            },
            scale,
            width,
            suggest,
            top_pad,
            key_h,
            row_gap,
            key_gap,
            margin,
            key_w,
            strip,
            tall,
        }
    }

    fn px(&self, points: f32) -> Pixels {
        px(points * self.scale)
    }

    /// The key rows' height with the gaps between them.
    fn rows_height(&self) -> f32 {
        self.top_pad + 4.0 * self.key_h + 3.0 * self.row_gap
    }
}

fn hex(color: u32) -> Hsla {
    rgb(color).into()
}

/// What a key is, which decides how it looks and what a press does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Cap {
    /// Types its label, capitalised while shift is on.
    Char(&'static str),
    Shift,
    Delete,
    Return,
    Space,
    /// Turns to a page: the label and the page.
    Pages(&'static str, Page),
    /// Turns to the emoji page.
    Emoji,
    /// Switches between QWERTY and AZERTY.
    Globe,
    Mic,
    /// The iPad's hide-the-keyboard key; drawn only.
    Dismiss,
}

/// What every key needs to know beyond its cap.
struct Ctx {
    page: Page,
    layout: KeyboardLayout,
}

impl Ctx {
    fn shift(&self) -> bool {
        matches!(self.page, Page::Letters { shift: true })
    }

    fn label(&self, text: &str) -> String {
        match self.shift() {
            true => text.to_uppercase(),
            false => text.to_owned(),
        }
    }
}

/// A plain key: `width` in points, or the rest of the row when `None`.
fn key(m: &Metrics, width: Option<f32>, fill: u32) -> Div {
    div()
        .h(m.px(m.key_h))
        .when_some(width, |this, w| this.w(m.px(w)).flex_none())
        .when(width.is_none(), |this| this.flex_1())
        .flex()
        .items_center()
        .justify_center()
        .bg(hex(fill))
        .rounded(m.px(m.palette.radius))
        .when_some(m.palette.shadow, |this, shadow| {
            this.shadow(vec![BoxShadow {
                color: hex(shadow).opacity(0.9),
                offset: point(px(0.), m.px(1.0)),
                blur_radius: px(0.),
                spread_radius: px(0.),
                inset: false,
            }])
        })
        .text_color(hex(m.palette.label))
        .cursor_pointer()
}

fn icon(m: &Metrics, name: IconName, size: f32, color: u32) -> Icon {
    Icon::new(name).size(m.px(size)).text_color(hex(color))
}

/// Switches the cover's page on a press.
fn turns_to(this: Div, page: Page, cx: &mut Context<WorkspaceView>) -> Div {
    this.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
            view.keyboard_page = page;
            cx.stop_propagation();
            cx.notify();
        }),
    )
}

/// Sends `tap` on a press; a typed character spends a one-shot shift.
fn types(this: Div, tap: KeyTap, cx: &mut Context<WorkspaceView>) -> Div {
    this.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            if matches!(tap, KeyTap::Text(_))
                && view.keyboard_page == (Page::Letters { shift: true })
            {
                view.keyboard_page = Page::Letters { shift: false };
                cx.notify();
            }
            cx.emit(ViewportAction::KeyboardTap(tap.clone()));
        }),
    )
}

/// One key: how `cap` looks in this style, at `width` points (`None`: the
/// rest of the row), and what it does.
fn cap_key(
    m: &Metrics,
    cap: Cap,
    width: Option<f32>,
    ctx: &Ctx,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let p = m.palette;
    let letter = m.key_h * if m.style == Style::Ipad { 0.42 } else { 0.54 };
    let word = m.key_h * 0.38;
    let glyph = m.key_h * 0.5;
    let text = |fill: u32, label: String, size: f32| {
        key(m, width, fill)
            .text_size(m.px(size))
            .child(SharedString::from(label))
    };
    match cap {
        Cap::Char(c) => {
            let label = ctx.label(c);
            types(text(p.key, label.clone(), letter), KeyTap::Text(label), cx)
        }
        Cap::Shift => {
            let on = ctx.shift();
            let (fill, tint) = match on {
                true => (p.shift_on, p.shift_on_label),
                false => (p.special, p.label),
            };
            let shift = key(m, width, fill).child(icon(m, IconName::ArrowBigUp, glyph, tint));
            match ctx.page {
                Page::Letters { .. } => turns_to(shift, Page::Letters { shift: !on }, cx),
                _ => shift,
            }
        }
        Cap::Delete => types(
            key(m, width, p.special).child(icon(m, IconName::Delete, glyph, p.label)),
            KeyTap::Backspace,
            cx,
        ),
        Cap::Return => {
            let pressed = match p.accent {
                Some((fill, tint)) => key(m, width, fill).rounded_full().child(icon(
                    m,
                    IconName::CornerDownLeft,
                    glyph,
                    tint,
                )),
                None => {
                    let label = match ctx.layout {
                        KeyboardLayout::Qwerty => "return",
                        KeyboardLayout::Azerty => "retour",
                    };
                    text(p.special, label.to_owned(), word)
                }
            };
            types(pressed, KeyTap::Return, cx)
        }
        Cap::Space => {
            let label = match (m.style, ctx.layout) {
                (Style::Android, KeyboardLayout::Qwerty) => "English",
                (Style::Android, KeyboardLayout::Azerty) => "Fran\u{e7}ais",
                (_, KeyboardLayout::Qwerty) => "space",
                (_, KeyboardLayout::Azerty) => "espace",
            };
            types(
                text(p.key, label.to_owned(), word),
                KeyTap::Text(" ".to_owned()),
                cx,
            )
        }
        Cap::Pages(label, page) => turns_to(text(p.special, label.to_owned(), word), page, cx),
        Cap::Emoji => turns_to(
            key(m, width, p.special).child(icon(m, IconName::FaceGrinning, glyph, p.label)),
            Page::Emoji,
            cx,
        ),
        Cap::Globe => {
            let next = ctx.layout.next();
            key(m, width, p.special)
                .child(icon(m, IconName::Globe, glyph, p.label))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        cx.emit(ViewportAction::KeyboardLayout(next));
                    }),
                )
        }
        Cap::Mic => key(m, width, p.special).child(icon(m, IconName::Mic, glyph, p.label)),
        Cap::Dismiss => {
            key(m, width, p.special).child(icon(m, IconName::KeyboardOff, glyph, p.label))
        }
    }
}

/// A bare icon that switches layout: the globe below an iPhone's keys, and
/// Gboard's keyboard switcher in the navigation bar.
fn layout_switch(
    m: &Metrics,
    name: IconName,
    size: f32,
    layout: KeyboardLayout,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let next = layout.next();
    div()
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                cx.emit(ViewportAction::KeyboardLayout(next));
            }),
        )
        .child(icon(m, name, size, m.palette.label))
}

pub(super) fn cover(
    place: Placement,
    style: Style,
    layout: KeyboardLayout,
    page: Page,
    occlude: bool,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let m = Metrics::new(&place, style);
    let ctx = Ctx { page, layout };
    let body = match (page, style) {
        (Page::Emoji, _) => emoji::page(&m, &ctx, cx),
        (_, Style::Iphone) => iphone::page(&m, &ctx, cx),
        (_, Style::Ipad) => fitted::ipad(&m, &ctx, cx),
        (_, Style::Android) => fitted::android(&m, &ctx, cx),
    };
    let strip = match style {
        Style::Android => fitted::nav_bar(&m, &ctx, cx),
        _ => iphone::strip(&m, &ctx, cx),
    };
    let note = div()
        .h(m.px(m.suggest))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .text_size(m.px(11.0))
        .text_color(hex(m.palette.faint))
        .child("Keyboard cover \u{2014} not reported to the game");
    div()
        .absolute()
        .left(place.left)
        .top(place.top)
        .w(place.width)
        .h(place.height)
        .flex()
        .flex_col()
        .bg(hex(m.palette.background))
        .overflow_hidden()
        // Nothing under the cover is the scene's to pick.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .when(page != Page::Emoji, |this| this.child(note))
        .child(body)
        .child(strip)
        .when(occlude, |this| this.child(gpui_base::occlusion::probe()))
}

#[cfg(test)]
mod tests;
