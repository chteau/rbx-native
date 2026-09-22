//! `ScreenGui` support: reading 2D overlay trees out of a DOM ([`plan`]) and
//! resolving their `UDim2` boxes against a viewport ([`layout`]) — pure
//! geometry, independent of any GPU state.
//!
//! [`Scene::gui_screens`](super::Scene::gui_screens) is the only door in. The
//! split exists because the two halves change at different rates: a tree is
//! read once when the place is opened, while its pixel rects have to be
//! recomputed whenever the window is resized.
//!
//! [`space`] adds the two containers that draw the same tree somewhere other
//! than the screen: a `BillboardGui`'s canvas faces the camera in the world, a
//! `SurfaceGui`'s lies on one face of a part. Both reuse [`plan`] and
//! [`layout`] whole — the only thing that differs is the pixel rect the tree
//! resolves against, and where that rect ends up.
//!
//! [`style`] runs before all of it: a `StyleLink` applies a `StyleSheet`'s
//! rules to the tree it sits in, so [`plan`] reads each instance's properties
//! through that view rather than straight off the DOM.
//!
//! Text (`TextLabel`/`TextButton`/`TextBox`) is read into [`plan::Text`] and
//! measured through [`layout::TextMeasure`], which the renderer's typesetter
//! implements: shaping needs a font system, and none lives here. Of the
//! `UIComponent` family the layouts are read (see [`plan::Layout`]) along
//! with `UIFlexItem`, everything that changes an element's size —
//! `UIPadding`, `UIScale`, the `UIConstraint` classes — and the appearance
//! modifiers `UICorner`, `UIStroke` and `UIGradient`.

mod layout;
mod plan;
mod space;
mod style;
mod wheel;

#[cfg(test)]
pub(crate) use layout::StrokePx;
#[cfg(test)]
pub(crate) use layout::{resolve, resolve_canvas};
pub(crate) use layout::{
    resolve_canvas_with, resolve_with, screen_frame, Element, GradientPx, Grouped, ImageScale,
    Painted, PixelRect, Rect, TextMeasure, Typeset,
};
pub(crate) use plan::image_placeholder as gui_image_placeholder;
#[cfg(test)]
pub(crate) use plan::GroupTint;
#[cfg(test)]
pub(crate) use plan::TextSpan;
pub(crate) use plan::{
    plan, span_face, Align, GradientKind, Join, Screen, Text, Tile, ViewCamera, Viewport,
};
pub(crate) use space::{plan as plan_space, Anchor, SpaceGui};
pub use wheel::ScrollTarget;
pub(crate) use wheel::{scroll_target, ScrollWindow};

#[cfg(test)]
mod tests;
