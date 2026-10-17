//! The drawing's controls: buttons, sticks, triggers and their key
//! captions, each placed in the drawing's own pixel space.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_play::gamepad::{Button, Device, PadState, Side};

use super::super::mapping::{key_label, Bindings, Control, Direction};
use super::super::ControllerEmulator;
use super::{drag_move, Drag, Slot, KNOB, STICK, TRACK, TRAVEL};
use crate::tokens;

pub(super) struct Ctx<'a> {
    pub(super) device: Device,
    pub(super) state: &'a PadState,
    pub(super) keys: Option<&'a Bindings>,
    pub(super) highlight: Option<Control>,
}

impl Ctx<'_> {
    fn key(&self, control: Control) -> Option<String> {
        self.keys?.get(&control).map(|key| key_label(key))
    }

    fn button_marked(&self, button: Button) -> bool {
        self.highlight == Some(Control::Button(button))
    }

    fn stick_marked(&self, side: Side) -> bool {
        matches!(self.highlight, Some(Control::Stick(s, _)) if s == side)
    }

    fn trigger_marked(&self, side: Side) -> bool {
        self.highlight == Some(Control::Trigger(side))
    }
}

pub(super) fn at(x: f32, y: f32, w: f32, h: f32) -> Div {
    div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h))
}

pub(super) fn skin(base: Div, lit: bool, marked: bool) -> Div {
    base.border_1()
        .border_color(tokens::border())
        .bg(if lit {
            tokens::check_on()
        } else {
            tokens::tile()
        })
        .text_color(if lit { tokens::black() } else { tokens::text() })
        .when(marked, |this| {
            this.border_2().border_color(tokens::check_on())
        })
}

pub(super) fn caption(items: &mut Vec<AnyElement>, text: Option<String>, x: f32, y: f32, w: f32) {
    if let Some(text) = text.filter(|text| !text.is_empty()) {
        items.push(
            at(x, y, w, 12.)
                .flex()
                .justify_center()
                .text_size(px(10.))
                .line_height(px(12.))
                .font_family("monospace")
                .text_color(tokens::text2())
                .child(text)
                .into_any_element(),
        );
    }
}

pub(super) fn face_label(device: Device, button: Button) -> &'static str {
    if !matches!(device, Device::Ps4 | Device::Ps5) {
        return device.button_label(button);
    }
    match button {
        Button::South => "×",
        Button::East => "○",
        Button::West => "□",
        Button::North => "△",
        _ => device.button_label(button),
    }
}

/// A clickable button; the pointer holds it while the mouse is down.
pub(super) fn button(
    ctx: &Ctx,
    cx: &mut Context<ControllerEmulator>,
    button: Button,
    label: &str,
    (x, y, w, h): (f32, f32, f32, f32),
    radius: f32,
) -> AnyElement {
    skin(
        at(x, y, w, h),
        ctx.state.pressed(button),
        ctx.button_marked(button),
    )
    .rounded(px(radius))
    .flex()
    .items_center()
    .justify_center()
    .text_size(px(11.))
    .cursor_pointer()
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| this.pointer_button(button, true, cx)),
    )
    .on_mouse_up(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseUpEvent, _, cx| this.pointer_button(button, false, cx)),
    )
    .child(label.to_owned())
    .into_any_element()
}

pub(super) fn pill(
    ctx: &Ctx,
    cx: &mut Context<ControllerEmulator>,
    items: &mut Vec<AnyElement>,
    which: Button,
    (x, y): (f32, f32),
    w: f32,
) {
    let label = ctx.device.button_label(which);
    items.push(button(ctx, cx, which, label, (x, y, w, 18.), 9.));
    caption(
        items,
        ctx.key(Control::Button(which)),
        x + w / 2. - 30.,
        y + 19.,
        60.,
    );
}

pub(super) fn stick(
    ctx: &Ctx,
    cx: &mut Context<ControllerEmulator>,
    items: &mut Vec<AnyElement>,
    side: Side,
    (centre_x, cy): (f32, f32),
    pill_above: bool,
    cell: Slot,
) {
    let (sx, sy) = ctx.state.stick(side);
    let off_centre = sx != 0. || sy != 0.;
    let half = STICK / 2.;
    let base = at(centre_x - half, cy - half, STICK, STICK)
        .rounded_full()
        .border_1()
        .border_color(if ctx.stick_marked(side) {
            tokens::check_on()
        } else {
            tokens::border()
        })
        .when(ctx.stick_marked(side), |this| this.border_2())
        .bg(tokens::dock())
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.drag = Some(Drag::Stick(side));
                drag_move(this, event.position, cx);
            }),
        )
        .child(
            canvas(move |bounds, _, _| cell.set(Some(bounds)), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .child(
            skin(
                at(
                    half - KNOB / 2. + sx * TRAVEL - 1.,
                    half - KNOB / 2. - sy * TRAVEL - 1.,
                    KNOB,
                    KNOB,
                ),
                off_centre,
                false,
            )
            .rounded_full(),
        );
    items.push(base.into_any_element());

    let directions = [
        Direction::Up,
        Direction::Left,
        Direction::Down,
        Direction::Right,
    ]
    .map(|direction| ctx.key(Control::Stick(side, direction)))
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let (click_y, caption_y) = if pill_above {
        (cy - half - 24., cy + half + 2.)
    } else {
        (cy + half + 4., cy - half - 13.)
    };
    let click = if side == Side::Left {
        Button::LeftStick
    } else {
        Button::RightStick
    };
    pill(ctx, cx, items, click, (centre_x - 25., click_y), 50.);
    caption(items, Some(directions), centre_x - 40., caption_y, 80.);
}

pub(super) fn trigger(
    ctx: &Ctx,
    cx: &mut Context<ControllerEmulator>,
    items: &mut Vec<AnyElement>,
    side: Side,
    cell: Slot,
) {
    let x = if side == Side::Left { 94. } else { 404. };
    let value = ctx.state.trigger(side);
    let track = at(x, 4., TRACK.0, TRACK.1)
        .rounded(px(6.))
        .overflow_hidden()
        .border_1()
        .border_color(if ctx.trigger_marked(side) {
            tokens::check_on()
        } else {
            tokens::border()
        })
        .when(ctx.trigger_marked(side), |this| this.border_2())
        .bg(tokens::track())
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.drag = Some(Drag::Trigger(side));
                drag_move(this, event.position, cx);
            }),
        )
        .child(
            canvas(move |bounds, _, _| cell.set(Some(bounds)), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .bottom_0()
                .w_full()
                .h(relative(value))
                .bg(tokens::check_on()),
        );
    items.push(track.into_any_element());
    let outward = if side == Side::Left { x - 40. } else { x + 24. };
    caption(
        items,
        Some(ctx.device.trigger_label(side).to_owned()),
        outward,
        20.,
        38.,
    );
    caption(items, ctx.key(Control::Trigger(side)), outward, 34., 38.);
}

pub(super) fn bumper(
    ctx: &Ctx,
    cx: &mut Context<ControllerEmulator>,
    items: &mut Vec<AnyElement>,
    side: Side,
) {
    let (which, x, caption_x) = if side == Side::Left {
        (Button::LeftBumper, 60., 20.)
    } else {
        (Button::RightBumper, 370., 462.)
    };
    items.push(button(
        ctx,
        cx,
        which,
        ctx.device.button_label(which),
        (x, 70., 90., 20.),
        8.,
    ));
    caption(items, ctx.key(Control::Button(which)), caption_x, 74., 38.);
}

pub(super) fn dpad(
    ctx: &Ctx,
    cx: &mut Context<ControllerEmulator>,
    items: &mut Vec<AnyElement>,
    (x, y): (f32, f32),
) {
    let cells = [
        (Button::DpadUp, "▲", (x - 13., y - 39.), (x - 30., y - 52.)),
        (
            Button::DpadDown,
            "▼",
            (x - 13., y + 13.),
            (x - 30., y + 40.),
        ),
        (Button::DpadLeft, "◀", (x - 39., y - 13.), (x - 72., y - 6.)),
        (
            Button::DpadRight,
            "▶",
            (x + 13., y - 13.),
            (x + 42., y - 6.),
        ),
    ];
    for (which, glyph, (bx, by), (kx, ky)) in cells {
        items.push(button(ctx, cx, which, glyph, (bx, by, 26., 26.), 5.));
        caption(items, ctx.key(Control::Button(which)), kx, ky, 60.);
    }
}

pub(super) fn faces(
    ctx: &Ctx,
    cx: &mut Context<ControllerEmulator>,
    items: &mut Vec<AnyElement>,
    (x, y): (f32, f32),
) {
    let diamond = [
        (Button::North, (0., -40.)),
        (Button::West, (-40., 0.)),
        (Button::East, (40., 0.)),
        (Button::South, (0., 40.)),
    ];
    for (which, (dx, dy)) in diamond {
        let rect = (x + dx - 15., y + dy - 15., 30., 30.);
        items.push(button(
            ctx,
            cx,
            which,
            face_label(ctx.device, which),
            rect,
            15.,
        ));
        caption(
            items,
            ctx.key(Control::Button(which)),
            rect.0 - 15.,
            rect.1 + 31.,
            60.,
        );
    }
}
