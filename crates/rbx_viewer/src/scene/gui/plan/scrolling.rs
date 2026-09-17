//! Reads what a `ScrollingFrame` adds to a `Frame`: the canvas its children
//! resolve against, how far it is scrolled, and the scroll bars a still frame
//! shows. `ElasticBehavior` is read nowhere: the docs describe it purely as
//! how far a touch drag may overshoot, which a static frame never does.

use std::collections::BTreeMap;

use rbx_assets::AssetRef;
use rbx_dom::Variant;

use super::props::{alpha, color, enum_of, flag, integer, vector2};
use super::{span, Span};
use crate::textures::asset_uri;

/// `Enum.ScrollBarInset`, straight off `enums/ScrollBarInset.yaml`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::scene::gui) enum Inset {
    #[default]
    None,
    /// "The canvas will only be inset if the respective scroll bar is showing."
    ScrollBar,
    Always,
}

impl Inset {
    /// Whether the canvas gives up `ScrollBarThickness` to a bar that is
    /// `shown`.
    pub(in crate::scene::gui) fn applies(self, shown: bool) -> bool {
        match self {
            Inset::None => false,
            Inset::ScrollBar => shown,
            Inset::Always => true,
        }
    }
}

/// The three images a scroll bar is built from — `TopImage`, `MidImage`,
/// `BottomImage` — the first and last "rotated 90° counterclockwise for a
/// horizontal scroll bar" (docs), so one set serves both axes.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::scene::gui) struct BarImages {
    pub(in crate::scene::gui) top: AssetRef,
    pub(in crate::scene::gui) mid: AssetRef,
    pub(in crate::scene::gui) bottom: AssetRef,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::scene::gui) struct Scrolling {
    /// `CanvasSize`; which box its scale is a fraction of is the layout's
    /// call (see `layout::scrolling`).
    pub(in crate::scene::gui) canvas_size: Span,
    pub(in crate::scene::gui) canvas_position: [f32; 2],
    /// `AutomaticCanvasSize`, per axis, same encoding as `Node::automatic_size`.
    pub(in crate::scene::gui) automatic_canvas: [bool; 2],
    /// `ScrollingDirection`, per axis: "if scrolling is disallowed in a
    /// direction, the associated scroll bar will not appear".
    pub(in crate::scene::gui) direction: [bool; 2],
    /// `ScrollBarThickness` in pixels, already zero where `ScrollingEnabled`
    /// is off — the docs give both the same effect, "no scroll bars will be
    /// rendered".
    pub(in crate::scene::gui) thickness: f32,
    pub(in crate::scene::gui) bar_color: [f32; 3],
    pub(in crate::scene::gui) bar_alpha: f32,
    pub(in crate::scene::gui) images: BarImages,
    /// `VerticalScrollBarPosition.Left`; `Right` is the default.
    pub(in crate::scene::gui) bar_left: bool,
    pub(in crate::scene::gui) vertical_inset: Inset,
    pub(in crate::scene::gui) horizontal_inset: Inset,
}

/// `Enum.ScrollingDirection` ordinals: `X` 1, `Y` 2, `XY` 4 — not a bit
/// mask, unlike `AutomaticSize`.
const DIRECTION_X: u32 = 1;
const DIRECTION_Y: u32 = 2;
const DIRECTION_XY: u32 = 4;

/// `Enum.VerticalScrollBarPosition.Left`.
const LEFT: u32 = 1;

/// The docs name the top cap's default (`TopImageContent`); the other two
/// follow the same naming, as a saved place serializes them.
const DEFAULT_TOP: &str = "rbxasset://textures/ui/Scroll/scroll-top.png";
const DEFAULT_MID: &str = "rbxasset://textures/ui/Scroll/scroll-middle.png";
const DEFAULT_BOTTOM: &str = "rbxasset://textures/ui/Scroll/scroll-bottom.png";

/// The docs state no default for `ScrollBarThickness`; a place file always
/// serializes it, so this only ever applies to a tree built in code.
const DEFAULT_THICKNESS: i32 = 12;

pub(in crate::scene::gui) fn scrolling(properties: &BTreeMap<String, Variant>) -> Scrolling {
    let direction = match enum_of(properties, "ScrollingDirection", DIRECTION_XY) {
        DIRECTION_X => [true, false],
        DIRECTION_Y => [false, true],
        _ => [true, true],
    };
    let thickness = match flag(properties, "ScrollingEnabled", true) {
        true => integer(properties, "ScrollBarThickness", DEFAULT_THICKNESS).max(0) as f32,
        false => 0.0,
    };
    Scrolling {
        canvas_size: span(properties, "CanvasSize"),
        canvas_position: vector2(properties, "CanvasPosition"),
        automatic_canvas: super::constraints::automatic_axes(properties, "AutomaticCanvasSize"),
        direction,
        thickness,
        // The docs state no default; "when set to white, no colorization
        // occurs" is all they say. A Studio-saved place carries black.
        bar_color: color(properties, "ScrollBarImageColor3", [0.0, 0.0, 0.0]),
        bar_alpha: alpha(properties, "ScrollBarImageTransparency"),
        images: BarImages {
            top: image(properties, "TopImage", DEFAULT_TOP),
            mid: image(properties, "MidImage", DEFAULT_MID),
            bottom: image(properties, "BottomImage", DEFAULT_BOTTOM),
        },
        bar_left: enum_of(properties, "VerticalScrollBarPosition", 0) == LEFT,
        vertical_inset: inset(properties, "VerticalScrollBarInset"),
        horizontal_inset: inset(properties, "HorizontalScrollBarInset"),
    }
}

/// One bar image, the built-in one where the property is missing or does
/// not parse — an empty string too, since a bar with no image is not a thing
/// the docs describe.
fn image(properties: &BTreeMap<String, Variant>, name: &str, default: &str) -> AssetRef {
    let parsed = properties
        .get(name)
        .and_then(asset_uri)
        .and_then(|uri| AssetRef::parse(uri).ok())
        .filter(|asset| *asset != AssetRef::Empty);
    parsed.unwrap_or_else(|| AssetRef::parse(default).expect("a literal rbxasset path parses"))
}

fn inset(properties: &BTreeMap<String, Variant>, name: &str) -> Inset {
    match enum_of(properties, name, 0) {
        1 => Inset::ScrollBar,
        2 => Inset::Always,
        _ => Inset::None,
    }
}
