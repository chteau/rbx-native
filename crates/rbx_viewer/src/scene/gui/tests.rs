//! Unit tests for [`super`], split by what they exercise: [`layout`] for
//! `UDim2` resolution, paint ordering and `UIListLayout` stacking, [`clips`]
//! for `ClipsDescendants` and its incompatibility with `Rotation`,
//! [`properties`] for what is read out of a DOM in the first place.
//!
//! The fixtures every test builds on live here, in the parent.

use rbx_dom::{Color3Data, Ref, UDim, UDim2, Variant, Vector2Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::plan::Screen;
use super::{plan, resolve, Rect};

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
fn screen_gui() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let gui = dom.new_instance("ScreenGui", "ScreenGui", None);
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
    plan(dom, &ReflectionDatabase::embedded())
}

mod clips;
mod layout;
mod properties;
mod style;
