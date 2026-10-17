//! The drawn controller: our own vector sketch of a pad built from divs,
//! lit by the pad's state, with the stick and trigger drags it needs.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::v_flex;
use gpui_kit::*;
use rbx_play::gamepad::{Button, Device, PadState, Side};

use super::mapping::{Bindings, Control};
use super::ControllerEmulator;
use crate::tokens;
use parts::{at, bumper, dpad, faces, pill, stick, trigger, Ctx};

mod parts;

const WIDTH: f32 = 520.;
const HEIGHT: f32 = 290.;
const STICK: f32 = 70.;
const KNOB: f32 = 30.;
/// How far the knob's centre travels from the base's centre.
const TRAVEL: f32 = 25.;
const TRACK: (f32, f32) = (22., 64.);

type Slot = Rc<Cell<Option<Bounds<Pixels>>>>;

#[derive(Clone, Copy)]
pub(super) enum Drag {
    Stick(Side),
    Trigger(Side),
}

/// Where the sticks and triggers were last laid out, in window space,
/// written by the drawing's prepaint and read by the window-wide drag.
#[derive(Clone, Default)]
pub(super) struct Geometry {
    sticks: [Slot; 2],
    triggers: [Slot; 2],
}

struct Layout {
    left: (f32, f32),
    right: (f32, f32),
    right_pill_above: bool,
    left_pill_above: bool,
    dpad: (f32, f32),
    face: (f32, f32),
    /// Select, Home, Start (top-left corners).
    menu: [(f32, f32); 3],
}

fn layout(device: Device) -> Layout {
    let menu = [(188., 100.), (237., 128.), (288., 100.)];
    match device {
        Device::Ps4 | Device::Ps5 => Layout {
            left: (205., 228.),
            right: (315., 228.),
            left_pill_above: true,
            right_pill_above: true,
            dpad: (130., 135.),
            face: (390., 150.),
            menu,
        },
        Device::Handheld => Layout {
            left: (70., 150.),
            right: (395., 240.),
            left_pill_above: false,
            right_pill_above: true,
            dpad: (70., 240.),
            face: (450., 150.),
            menu: [(178., 252.), (237., 248.), (294., 252.)],
        },
        _ => Layout {
            left: (120., 135.),
            right: (335., 232.),
            left_pill_above: false,
            right_pill_above: true,
            dpad: (185., 234.),
            face: (400., 150.),
            menu,
        },
    }
}

pub(super) fn controller(
    device: Device,
    state: &PadState,
    keys: Option<&Bindings>,
    highlight: Option<Control>,
    geometry: &Geometry,
    cx: &mut Context<ControllerEmulator>,
) -> impl IntoElement {
    let ctx = Ctx {
        device,
        state,
        keys,
        highlight,
    };
    let place = layout(device);
    let mut items: Vec<AnyElement> = Vec::new();

    let handheld = device == Device::Handheld;
    let chrome = |div: Div| {
        div.bg(tokens::chrome())
            .border_1()
            .border_color(tokens::border())
    };
    if handheld {
        items.push(
            chrome(at(10., 100., 500., 170.))
                .rounded(px(36.))
                .into_any_element(),
        );
        items.push(
            at(170., 115., 180., 125.)
                .rounded(px(6.))
                .bg(tokens::black())
                .into_any_element(),
        );
    } else {
        for x in [45., 365.] {
            items.push(
                chrome(at(x, 190., 110., 100.))
                    .rounded(px(45.))
                    .into_any_element(),
            );
        }
        items.push(
            chrome(at(20., 80., 480., 200.))
                .rounded(px(60.))
                .into_any_element(),
        );
    }

    for side in [Side::Left, Side::Right] {
        trigger(
            &ctx,
            cx,
            &mut items,
            side,
            geometry.triggers[side as usize].clone(),
        );
        bumper(&ctx, cx, &mut items, side);
    }
    stick(
        &ctx,
        cx,
        &mut items,
        Side::Left,
        place.left,
        place.left_pill_above,
        geometry.sticks[0].clone(),
    );
    stick(
        &ctx,
        cx,
        &mut items,
        Side::Right,
        place.right,
        place.right_pill_above,
        geometry.sticks[1].clone(),
    );
    dpad(&ctx, cx, &mut items, place.dpad);
    faces(&ctx, cx, &mut items, place.face);
    for (which, at_) in [Button::Select, Button::Home, Button::Start]
        .into_iter()
        .zip(place.menu)
    {
        pill(
            &ctx,
            cx,
            &mut items,
            which,
            at_,
            if which == Button::Home { 46. } else { 44. },
        );
    }

    v_flex().flex_1().items_center().justify_center().child(
        div()
            .relative()
            .flex_none()
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .children(items),
    )
}

pub(super) fn drag_move(
    this: &mut ControllerEmulator,
    position: Point<Pixels>,
    cx: &mut Context<ControllerEmulator>,
) {
    let Some(drag) = this.drag else { return };
    match drag {
        Drag::Stick(side) => {
            let Some(bounds) = this.geometry.sticks[side as usize].get() else {
                return;
            };
            let centre = bounds.center();
            let mut x = f32::from(position.x - centre.x) / TRAVEL;
            let mut y = -f32::from(position.y - centre.y) / TRAVEL;
            let length = x.hypot(y);
            if length > 1. {
                x /= length;
                y /= length;
            }
            this.pointer_stick(side, x, y, cx);
        }
        Drag::Trigger(side) => {
            let Some(bounds) = this.geometry.triggers[side as usize].get() else {
                return;
            };
            let value = f32::from(bounds.bottom() - position.y) / f32::from(bounds.size.height);
            this.pointer_trigger(side, value.clamp(0., 1.), cx);
        }
    }
}

pub(super) fn drag_end(this: &mut ControllerEmulator, cx: &mut Context<ControllerEmulator>) {
    match this.drag.take() {
        Some(Drag::Stick(side)) => this.pointer_stick(side, 0., 0., cx),
        Some(Drag::Trigger(side)) => this.pointer_trigger(side, 0., cx),
        None => {}
    }
    for held in Button::ALL {
        if this.pointer.pressed(held) {
            this.pointer_button(held, false, cx);
        }
    }
}
