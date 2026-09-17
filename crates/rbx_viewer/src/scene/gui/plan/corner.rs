//! `UICorner`: the rounding of its parent's four corners, kept as `UDim`s
//! until the parent's pixel box is known.

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::modifiers;
use super::props::udim;
use crate::scene::gui::style::Styled;

const CLASS: &str = "UICorner";

/// The four radii as `(scale, offset)` pairs in the order top-left,
/// top-right, bottom-right, bottom-left. Roblox's docs make `Scale` a
/// fraction of the parent's *shorter* side, which is what `super::layout`
/// resolves it against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::scene::gui) struct Corner {
    pub(in crate::scene::gui) radii: [(f32, f32); 4],
}

/// The first `UICorner` among `children`. `UICorner` has no `Enabled`, so
/// every one counts; which of several wins is undocumented, and tree order is
/// the same tie-break `UIListLayout` gets.
///
/// The per-corner properties are the source of truth: `CornerRadius` is
/// documented as a `NotReplicated` shorthand that merely reads back
/// `TopLeftRadius`. It is still honoured where the individual radii are all
/// absent or zero — a place saved before they existed, or a tree built in
/// code, where a serializer filling in a class default it does not know
/// leaves the four at zero beside a real `CornerRadius`; Roblox itself never
/// writes the two disagreeing. A `UICorner` carrying neither rounds nothing,
/// since the docs give no default.
pub(super) fn read(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    styles: &Styled,
    children: &[Ref],
) -> Option<Corner> {
    let instance = modifiers(dom, database, children, CLASS).next()?;
    let properties = styles.properties_of(instance);
    let individual = [
        "TopLeftRadius",
        "TopRightRadius",
        "BottomRightRadius",
        "BottomLeftRadius",
    ]
    .map(|name| udim(properties, name).unwrap_or((0.0, 0.0)));
    let radii = match individual.iter().any(|&radius| radius != (0.0, 0.0)) {
        true => individual,
        false => [udim(properties, "CornerRadius").unwrap_or((0.0, 0.0)); 4],
    };
    Some(Corner { radii })
}
