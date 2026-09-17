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
//! # What is not read here
//! Text itself: a `TextLabel`/`TextButton`/`TextBox` draws its background and
//! border like a `Frame`, but its glyphs need font loading and shaping, which
//! is a dependency decision this viewer has not made. Of the `UIComponent`
//! family the layouts are read (see [`plan::Layout`]) along with `UIFlexItem`,
//! everything that changes an element's size — `UIPadding`, `UIScale`, the
//! `UIConstraint` classes — and the appearance modifiers `UICorner`,
//! `UIStroke` and `UIGradient`.

mod layout;
mod plan;
mod space;
mod style;

#[cfg(test)]
pub(crate) use layout::Painted;
#[cfg(test)]
pub(crate) use layout::StrokePx;
pub(crate) use layout::{resolve, resolve_canvas, Element, GradientPx, Rect};
pub(crate) use plan::{plan, GradientKind, Join, Screen, Tile};
pub(crate) use space::{plan as plan_space, Anchor, SpaceGui};

#[cfg(test)]
mod tests;
