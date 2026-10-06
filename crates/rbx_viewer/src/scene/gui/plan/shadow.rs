//! `UIShadow`: a drop shadow under its parent, in the parent's (rounded)
//! shape.

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::props::{alpha, color, flag, integer, span, udim};
use super::{modifiers, Span};
use crate::scene::gui::style::Styled;

const CLASS: &str = "UIShadow";

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::scene::gui) struct Shadow {
    pub(in crate::scene::gui) color: [f32; 3],
    pub(in crate::scene::gui) alpha: f32,
    /// `Offset`: x scale against the parent's width, y against its height.
    pub(in crate::scene::gui) offset: Span,
    /// `Spread`, against the parent's size the same way.
    pub(in crate::scene::gui) spread: Span,
    /// `BlurRadius` as `(scale, offset)`, the scale against the parent's
    /// shorter side.
    pub(in crate::scene::gui) blur: (f32, f32),
}

/// Every *enabled* `UIShadow` among `children`, lowest `ZIndex` first: the
/// docs render several "in increasing `ZIndex` order", all of them below
/// the parent; ties keep tree order.
///
/// `Mode`, `Inset` and `ShowBehindParent` are not read. The docs list text
/// and inset shadows as unsupported (a text object's shadow is its box's,
/// which `Mode.Shape` already is) and do not describe `ShowBehindParent`,
/// so every shadow is drawn as the default one is: whole, behind the
/// parent. Defaults are rbx-dom's for a fresh `UIShadow` (black, opaque,
/// no offset, spread or blur).
pub(super) fn read(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    styles: &Styled,
    children: &[Ref],
) -> Vec<Shadow> {
    let mut shadows: Vec<(i32, Shadow)> = modifiers(dom, database, children, CLASS)
        .filter(|instance| flag(styles.properties_of(instance), "Enabled", true))
        .map(|instance| {
            let properties = styles.properties_of(instance);
            let shadow = Shadow {
                color: color(properties, "Color", [0.0, 0.0, 0.0]),
                alpha: alpha(properties, "Transparency"),
                offset: span(properties, "Offset"),
                spread: span(properties, "Spread"),
                blur: udim(properties, "BlurRadius").unwrap_or((0.0, 0.0)),
            };
            (integer(properties, "ZIndex", -1), shadow)
        })
        .collect();
    shadows.sort_by_key(|(z_index, _)| *z_index);
    shadows.into_iter().map(|(_, shadow)| shadow).collect()
}
