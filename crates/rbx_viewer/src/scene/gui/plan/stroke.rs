//! `UIStroke`: an outline around its parent's (rounded) box, or around its
//! parent's glyphs where the parent is a text class.

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::modifiers;
use super::props::{alpha, color, enum_of, flag, float, udim};

const CLASS: &str = "UIStroke";

/// `Enum.LineJoinMode`, in ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Join {
    Round,
    Bevel,
    Miter,
}

/// `Enum.BorderStrokePosition`, in ordinal order: where the band of
/// `Thickness` sits against the parent's edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::scene::gui) enum StrokePosition {
    Outer,
    Center,
    Inner,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::scene::gui) struct Stroke {
    pub(in crate::scene::gui) color: [f32; 3],
    pub(in crate::scene::gui) alpha: f32,
    /// Pixels, or — `StrokeSizingMode.ScaledSize` — a fraction of the
    /// parent's shorter side.
    pub(in crate::scene::gui) thickness: f32,
    pub(in crate::scene::gui) scaled: bool,
    pub(in crate::scene::gui) position: StrokePosition,
    /// `BorderOffset`, a `UDim` whose scale is against the parent's shorter
    /// side; positive pushes the band outward.
    pub(in crate::scene::gui) offset: (f32, f32),
    pub(in crate::scene::gui) join: Join,
    /// The stroke outlines the parent's text rather than its box: a text
    /// class parent left in `ApplyStrokeMode.Contextual`. Glyph outlines are
    /// the text renderer's to draw; the box stroke ignores such a node.
    pub(in crate::scene::gui) on_text: bool,
}

/// `Enum.ApplyStrokeMode.Border`; `Contextual` (the default) is 0.
const APPLY_BORDER: u32 = 1;
/// `Enum.StrokeSizingMode.ScaledSize`; `FixedSize` (the default) is 0.
const SIZING_SCALED: u32 = 1;

/// The first *enabled* `UIStroke` among `children`, `text` saying whether the
/// parent is a text class. The docs make `Enabled = false` a stroke that is
/// not rendered at all, so it is skipped over rather than read as invisible.
///
/// Defaults for a property the file leaves out are the ones Studio's
/// property window shows for a fresh `UIStroke` (1 px, black, opaque); the
/// docs themselves state none.
pub(super) fn read(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    children: &[Ref],
    text: bool,
) -> Option<Stroke> {
    let instance = modifiers(dom, database, children, CLASS)
        .find(|instance| flag(instance.properties(), "Enabled", true))?;
    let properties = instance.properties();
    Some(Stroke {
        color: color(properties, "Color", [0.0, 0.0, 0.0]),
        alpha: alpha(properties, "Transparency"),
        thickness: float(properties, "Thickness", 1.0).max(0.0),
        scaled: enum_of(properties, "StrokeSizingMode", 0) == SIZING_SCALED,
        position: match enum_of(properties, "BorderStrokePosition", 0) {
            1 => StrokePosition::Center,
            2 => StrokePosition::Inner,
            _ => StrokePosition::Outer,
        },
        offset: udim(properties, "BorderOffset").unwrap_or((0.0, 0.0)),
        join: match enum_of(properties, "LineJoinMode", 0) {
            1 => Join::Bevel,
            2 => Join::Miter,
            _ => Join::Round,
        },
        on_text: text && enum_of(properties, "ApplyStrokeMode", 0) != APPLY_BORDER,
    })
}
