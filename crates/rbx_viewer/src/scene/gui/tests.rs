//! Unit tests for [`super`], split by what they exercise: [`layout`] for
//! `UDim2` resolution, paint ordering and plain `UIListLayout` stacking,
//! [`flex`] for that layout's flex family, [`grid`] and [`table`] for the
//! other two layout classes, [`clips`] for `ClipsDescendants` and its
//! incompatibility with `Rotation`, [`properties`] for what is read out of a
//! DOM in the first place, [`constraints`] for the modifiers that change how
//! big a box is before any of that (`UIPadding`, `UIScale`, the
//! `UIConstraint` family, `AutomaticSize` and `SizeConstraint`), [`placement`]
//! for the rest of what decides where a box lands (nested rotation,
//! `BorderMode`, `ScreenInsets` and `ZIndexBehavior`), [`modifiers`] for
//! `UICorner`/`UIStroke`/`UIGradient`, [`containers`] for the non-`GuiObject`
//! instances a GUI tree is allowed to hold, [`scrolling`] for `ScrollingFrame`,
//! [`group`] for `CanvasGroup` and [`style`] for the `StyleSheet`
//! engine.
//!
//! The fixtures every test builds on live here, in the parent.

use rbx_dom::{Color3Data, Ref, UDim, UDim2, Variant, Vector2Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::plan::Screen;
use super::plan::{ViewCamera, Viewport};
use super::{plan, resolve, Element, ImageScale, PixelRect, Rect};
use crate::scene::Catalog;

const VIEWPORT: [f32; 2] = [800.0, 600.0];

fn udim2(sx: f32, ox: i32, sy: f32, oy: i32) -> Variant {
    Variant::UDim2(UDim2 {
        x: UDim {
            scale: sx,
            offset: ox,
        },
        y: UDim {
            scale: sy,
            offset: oy,
        },
    })
}

/// A DOM holding one enabled `ScreenGui`, ready for children to be hung off
/// the returned referent.
///
/// `ScreenInsets` is pinned to `None` so the canvas is the bare viewport: the
/// property's own default reserves the top bar's pixels, which is right for a
/// place but only obscures what these tests are measuring. The tests that care
/// set it themselves.
fn screen_gui() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let gui = dom.new_instance("ScreenGui", "ScreenGui", None);
    dom.set_property(gui, "ScreenInsets", Variant::Enum(0))
        .unwrap();
    (dom, gui)
}

fn frame(dom: &mut WeakDom, parent: Ref, position: Variant, size: Variant) -> Ref {
    let referent = dom.new_instance("Frame", "Frame", Some(parent));
    dom.set_property(referent, "Position", position).unwrap();
    dom.set_property(referent, "Size", size).unwrap();
    dom.set_property(referent, "BorderSizePixel", Variant::Int32(0))
        .unwrap();
    referent
}

fn screens(dom: &WeakDom) -> Vec<Screen> {
    let database = ReflectionDatabase::embedded();
    plan(dom, &database, &mut Catalog::new(dom, &database))
}

/// Every element of every screen, reduced to the rect it came to.
fn rects(dom: &WeakDom) -> Vec<Rect> {
    resolve(&screens(dom), VIEWPORT)
        .into_iter()
        .map(|element| element.rect)
        .collect()
}

mod clips;
mod constraints;
mod containers;
mod flex;
mod grid;
mod group;
mod layout;
mod modifiers;
mod pages;
mod placement;
mod properties;
mod scrolling;
mod style;
mod table;
mod text;
mod viewport;
