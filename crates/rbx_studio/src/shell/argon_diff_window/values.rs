//! A property value as the detail pane shows it: a chip in the Properties
//! panel's own words, with a swatch before a colour, and the class icons
//! the rows and the detail tile carry.

use gpui_kit::component::{h_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Variant;

use crate::class_icons::IconPack;
use crate::explorer::{self, ClassIcon};
use crate::shell::argon_sync::DiffNode;
use crate::shell::Shell;
use crate::tokens;

/// A value as the table shows it: the Properties panel's words for it,
/// and a swatch colour for a `Color3`, `Color3uint8` or `BrickColor`.
#[derive(Debug, Clone)]
pub(super) struct Cell {
    pub(super) text: SharedString,
    pub(super) swatch: Option<Rgba>,
}

/// One table row, both sides already formatted, so the pane needs
/// nothing of the shell while it draws.
#[derive(Debug, Clone)]
pub(super) struct FormattedProperty {
    pub(super) name: String,
    pub(super) before: Option<Cell>,
    pub(super) after: Option<Cell>,
}

/// Every property of `node` in the Properties panel's own words — the
/// one formatter, not a second one.
pub(super) fn format_properties(shell: &Shell, node: &DiffNode) -> Vec<FormattedProperty> {
    node.properties
        .iter()
        .map(|property| FormattedProperty {
            name: property.name.clone(),
            before: property.before.as_ref().map(|value| Cell {
                text: SharedString::from(shell.format_property(&node.class, &property.name, value)),
                swatch: swatch_colour(value),
            }),
            after: property.after.as_ref().map(|value| Cell {
                text: SharedString::from(shell.format_property(&node.class, &property.name, value)),
                swatch: swatch_colour(value),
            }),
        })
        .collect()
}

/// Which side of a change a chip stands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    Before,
    After,
}

/// 22 tall, `padding:0 7px`, radius 4, mono 11.5/16, the full text in
/// the tooltip; `diff_remove_soft` in `text2` before, `diff_add_soft` in
/// `text` after. A colour gets a 10×10 swatch first.
pub(super) fn chip(id: impl Into<ElementId>, cell: &Cell, side: Side) -> AnyElement {
    let tooltip = cell.text.clone();
    let (bg, ink) = match side {
        Side::Before => (tokens::diff_remove_soft(), tokens::text2()),
        Side::After => (tokens::diff_add_soft(), tokens::text()),
    };
    h_flex()
        .id(id.into())
        .max_w_full()
        .h(px(22.))
        .px(px(7.))
        .gap(px(6.))
        .items_center()
        .rounded(tokens::RADIUS_BADGE)
        .bg(bg)
        .font_family(tokens::FONT_FAMILY_MONO)
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .text_color(ink)
        .tooltip(move |window, cx| crate::shell::tooltip::text(tooltip.clone(), window, cx))
        .children(cell.swatch.map(|colour| {
            div()
                .flex_none()
                .size(px(10.))
                .rounded(px(2.))
                .border_1()
                .border_color(tokens::border2())
                .bg(colour)
        }))
        .child(div().min_w_0().truncate().child(cell.text.clone()))
        .into_any_element()
}

/// A plain `—` in `text3`: a before value the DOM doesn't have.
pub(super) fn unknown() -> AnyElement {
    div()
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .text_color(tokens::text3())
        .child("\u{2014}")
        .into_any_element()
}

fn swatch_colour(value: &Variant) -> Option<Rgba> {
    Some(match value {
        Variant::Color3(c) => Rgba {
            r: c.r,
            g: c.g,
            b: c.b,
            a: 1.,
        },
        Variant::Color3uint8 { r, g, b } => Rgba {
            r: f32::from(*r) / 255.,
            g: f32::from(*g) / 255.,
            b: f32::from(*b) / 255.,
            a: 1.,
        },
        Variant::BrickColor(number) => {
            let brick = rbx_dom::BrickColor::from_number(*number)?;
            Rgba {
                r: f32::from(brick.rgb[0]) / 255.,
                g: f32::from(brick.rgb[1]) / 255.,
                b: f32::from(brick.rgb[2]) / 255.,
                a: 1.,
            }
        }
        _ => return None,
    })
}

/// The Explorer's icon for `class` at `size`: its sprite, or the Lucide
/// stand-in for a class the icon kit doesn't cover.
pub(super) fn class_icon(class: &str, pack: IconPack, size: f32) -> AnyElement {
    match explorer::resolve_icon(class, pack) {
        ClassIcon::Sprite(image) => img(image).size(px(size)).flex_none().into_any_element(),
        ClassIcon::Lucide(name) => Icon::new(name).size(px(size)).into_any_element(),
    }
}

/// The 6 px dot and colour a kind is marked with.
pub(super) fn kind_colour(kind: crate::shell::argon_sync::ChangeKind) -> Rgba {
    use crate::shell::argon_sync::ChangeKind;
    match kind {
        ChangeKind::Added => tokens::diff_add(),
        ChangeKind::Updated => tokens::check_on(),
        ChangeKind::Removed => tokens::text_error(),
    }
}

pub(super) fn dot(colour: Rgba) -> Div {
    div().flex_none().size(px(6.)).rounded_full().bg(colour)
}

/// A mono count run: `+13` in green, `−10` in red, 6 apart, a zero left out.
pub(super) fn plus_minus(added: usize, removed: usize, size: Pixels, line: Pixels) -> Div {
    h_flex()
        .flex_none()
        .gap(px(6.))
        .font_family(tokens::FONT_FAMILY_MONO)
        .text_size(size)
        .line_height(line)
        .when(added > 0, |this| {
            this.child(
                div()
                    .text_color(tokens::diff_add())
                    .child(format!("+{added}")),
            )
        })
        .when(removed > 0, |this| {
            this.child(
                div()
                    .text_color(tokens::text_error())
                    .child(format!("\u{2212}{removed}")),
            )
        })
}
