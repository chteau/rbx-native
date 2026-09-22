//! What the pointer, the wheel and the keyboard do on the canvas: select,
//! move, resize, rotate, marquee, pan, zoom and nudge.
//!
//! A drag is always measured from where it started against a snapshot of
//! what it grabbed ([`Held`]), never step on step: the boxes the canvas is
//! hit-tested against come back from the render thread a frame after each
//! write, and a gesture that read them mid-drag would chase its own tail.
//! Every step writes through `Shell::write_drag` — one undo entry per
//! gesture, the Properties panel's own commit.

use gpui_kit::*;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_viewer::snap::round_to;
use rbx_viewer::GuiBox;

use super::super::Shell;
use super::is_gui_object;
use crate::ui_canvas::arrange::Member;
use crate::ui_canvas::guides;
use crate::ui_canvas::{
    self, angle_of, box_of, position_shift, resize, rotate, shifted, udim2_text, Handle, Rect,
    Udim2,
};

/// How far the pointer travels, in panel pixels, before a press is a drag.
const DRAG_THRESHOLD: f32 = 3.0;
/// How near, in panel pixels, a handle has to be to be grabbed rather than
/// what is under it — and how near a line has to come to snap onto another.
pub(super) const HANDLE_REACH: f32 = 6.0;
pub(super) const SNAP_REACH: f32 = 6.0;
/// How far above the top edge, in panel pixels, the rotation knob stands.
pub(super) const KNOB_OFFSET: f32 = 20.0;
/// Shift's rotation step, Sketch's and Figma's alike.
const ROTATE_STEP: f32 = 15.0;

/// One element as a gesture grabbed it: its box as laid out, and the
/// properties a drag rewrites, as they stood.
#[derive(Debug, Clone, Copy)]
pub(super) struct Held {
    pub(super) referent: Ref,
    pub(super) rect: Rect,
    /// `AbsoluteRotation`, and how much of it is the element's own
    /// `Rotation` rather than its ancestors'.
    pub(super) rotation: f32,
    own_rotation: f32,
    pub(super) anchor: [f32; 2],
    pub(super) position: Udim2,
    size: Udim2,
}

impl Held {
    fn read(dom: &WeakDom, placed: &GuiBox) -> Option<Held> {
        let properties = dom.get(placed.referent)?.properties();
        let udim2 = |name: &str| match properties.get(name) {
            Some(Variant::UDim2(value)) => [
                (value.x.scale, value.x.offset),
                (value.y.scale, value.y.offset),
            ],
            _ => [(0.0, 0); 2],
        };
        Some(Held {
            referent: placed.referent,
            rect: Rect::of(placed),
            rotation: placed.rotation,
            own_rotation: match properties.get("Rotation") {
                Some(Variant::Float32(degrees)) => *degrees,
                _ => 0.0,
            },
            anchor: match properties.get("AnchorPoint") {
                Some(Variant::Vector2(anchor)) => [anchor.x, anchor.y],
                _ => [0.0, 0.0],
            },
            position: udim2("Position"),
            size: udim2("Size"),
        })
    }

    pub(super) fn parent_rotation(&self) -> f32 {
        self.rotation - self.own_rotation
    }

    /// The same read, as a group takes a member in.
    pub(super) fn read_member(dom: &WeakDom, placed: &GuiBox) -> Option<Member> {
        let held = Held::read(dom, placed)?;
        Some(Member {
            rect: held.rect,
            anchor: held.anchor,
            position: held.position,
        })
    }
}

/// What a press landed on, before it is known to be a click or a drag.
#[derive(Debug, Clone, Copy)]
pub(super) enum Press {
    /// An element: a drag moves the selection. A press outside the
    /// selection has already picked `hit`; one inside it held the selection
    /// for the drag, and so a click there `repick`s what is under it on
    /// release instead — toggling it with `extend`.
    Element {
        hit: Ref,
        repick: bool,
        extend: bool,
    },
    Handle(Handle),
    Knob,
    Empty {
        extend: bool,
    },
}

#[derive(Debug, Clone)]
pub(super) enum Gesture {
    Pressed {
        at: [f32; 2],
        press: Press,
    },
    Move {
        at: [f32; 2],
        held: Vec<Held>,
        shift: [f32; 2],
        first: bool,
    },
    Resize {
        at: [f32; 2],
        held: Held,
        handle: Handle,
        rect: Rect,
        first: bool,
    },
    Rotate {
        held: Held,
        from: f32,
        rotation: f32,
        first: bool,
    },
    Marquee {
        from: [f32; 2],
        to: [f32; 2],
        extend: bool,
    },
    Pan {
        last: [f32; 2],
    },
}

impl Shell {
    /// The canvas's boxes, if the frame on hand is the one the canvas asks
    /// for — the screen's own frame first, then every element (see
    /// `rbx_viewer::GuiCanvas::boxes`).
    pub(super) fn canvas_boxes(&self, cx: &App) -> Option<(GuiBox, Vec<GuiBox>)> {
        let request = self.canvas_request()?;
        let canvas = self.viewport.read(cx).canvas()?;
        if canvas.request != request {
            return None;
        }
        let (screen, elements) = canvas.boxes.split_first()?;
        Some((*screen, elements.to_vec()))
    }

    /// A point in window pixels, in the canvas panel's own.
    fn panel_point(&self, position: Point<Pixels>) -> [f32; 2] {
        let origin = self.ui.bounds.get().origin;
        [
            f32::from(position.x - origin.x),
            f32::from(position.y - origin.y),
        ]
    }

    /// The one selected element the handles are drawn on: exactly one
    /// `GuiObject` selected, and laid out.
    pub(super) fn handled(&self, boxes: &[GuiBox]) -> Option<GuiBox> {
        match self.selected_all() {
            [only] => box_of(boxes, *only).copied(),
            _ => None,
        }
    }

    pub(super) fn canvas_press(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.ui.focus, cx);
        let at = self.panel_point(event.position);
        let Some((_, boxes)) = self.canvas_boxes(cx) else {
            return;
        };
        let view = self.ui.view;
        let point = view.to_canvas(at);
        let extend = event.modifiers.shift || event.modifiers.control || event.modifiers.platform;
        let near = |p: [f32; 2]| {
            let q = view.to_view(p);
            (q[0] - at[0]).hypot(q[1] - at[1]) <= HANDLE_REACH
        };

        // A handle wins over the body it sits on, except inside an element
        // too small on screen to tell the two apart: there the body wins,
        // and the handles are reached from just outside it.
        let press = if let Some(placed) = self
            .handled(&boxes)
            .filter(|placed| !ui_canvas::covers(placed, point) || roomy(placed, view.zoom))
        {
            let rect = Rect::of(&placed);
            let grabbed = Handle::ALL
                .into_iter()
                .find(|handle| near(handle.at(&rect, placed.rotation)));
            match grabbed {
                Some(handle) => Some(Press::Handle(handle)),
                None => near(knob(&rect, placed.rotation, view.zoom)).then_some(Press::Knob),
            }
        } else {
            None
        };
        let press = press.unwrap_or_else(|| match ui_canvas::hit(&boxes, point) {
            Some(hit) => {
                // Inside what is already selected, a press holds the
                // selection so a drag carries it; a click then re-picks.
                let inside = self.selected_all().iter().any(|&selected| {
                    box_of(&boxes, selected).is_some_and(|placed| ui_canvas::covers(placed, point))
                });
                if !inside {
                    match extend {
                        true => self.extend_selection(hit, cx),
                        false => self.select(hit, cx),
                    }
                }
                Press::Element {
                    hit,
                    repick: inside,
                    extend,
                }
            }
            None => Press::Empty { extend },
        });
        self.ui.gesture = Some(Gesture::Pressed { at, press });
        cx.notify();
    }

    pub(super) fn canvas_pan_press(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.ui.gesture = Some(Gesture::Pan {
            last: self.panel_point(event.position),
        });
        cx.notify();
    }

    pub(super) fn canvas_moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let at = self.panel_point(event.position);
        let modifiers = event.modifiers;
        self.ui.measuring = modifiers.alt;
        let Some(gesture) = self.ui.gesture.take() else {
            self.canvas_hover(at, cx);
            return;
        };
        let gesture = match gesture {
            Gesture::Pressed { at: start, press } => {
                if (at[0] - start[0]).hypot(at[1] - start[1]) < DRAG_THRESHOLD {
                    Some(Gesture::Pressed { at: start, press })
                } else {
                    self.begin_drag(start, press, cx)
                }
            }
            other => Some(other),
        };
        self.ui.gesture = gesture.map(|gesture| self.drag_to(gesture, at, modifiers, cx));
        cx.notify();
    }

    fn canvas_hover(&mut self, at: [f32; 2], cx: &mut Context<Self>) {
        let hovered = self
            .canvas_boxes(cx)
            .and_then(|(_, boxes)| ui_canvas::hit(&boxes, self.ui.view.to_canvas(at)));
        if hovered != self.ui.hovered {
            self.ui.hovered = hovered;
            cx.notify();
        }
        if self.ui.measuring && hovered.is_some() {
            // Alt is a live modifier here, not a tap into the menu bar.
            self.menu_bar.update(cx, |bar, _| bar.interrupt_alt_tap());
        }
    }

    /// A press that has moved far enough to be a drag: what it grabs.
    fn begin_drag(
        &mut self,
        at: [f32; 2],
        press: Press,
        cx: &mut Context<Self>,
    ) -> Option<Gesture> {
        let (_, boxes) = self.canvas_boxes(cx)?;
        let held_one = || {
            self.handled(&boxes)
                .and_then(|placed| Held::read(&self.dom, &placed))
        };
        Some(match press {
            Press::Element { .. } => Gesture::Move {
                at,
                held: self.held_selection(&boxes),
                shift: [0.0, 0.0],
                first: true,
            },
            Press::Handle(handle) => {
                let held = held_one()?;
                Gesture::Resize {
                    at,
                    held,
                    handle,
                    rect: held.rect,
                    first: true,
                }
            }
            Press::Knob => {
                let held = held_one()?;
                Gesture::Rotate {
                    held,
                    from: angle_of(held.rect.centre(), self.ui.view.to_canvas(at)),
                    rotation: held.own_rotation,
                    first: true,
                }
            }
            Press::Empty { extend } => {
                let from = self.ui.view.to_canvas(at);
                Gesture::Marquee {
                    from,
                    to: from,
                    extend,
                }
            }
        })
    }

    /// Every selected element a move carries: the laid-out `GuiObject`s,
    /// less any whose ancestor is carried too — moving the parent already
    /// moves it.
    pub(super) fn held_selection(&self, boxes: &[GuiBox]) -> Vec<Held> {
        let selected = self.selected_all();
        selected
            .iter()
            .filter(|&&referent| is_gui_object(&self.dom, &self.database, referent))
            .filter(|&&referent| {
                let mut up = self.dom.parent(referent);
                while let Some(ancestor) = up {
                    if selected.contains(&ancestor) {
                        return false;
                    }
                    up = self.dom.parent(ancestor);
                }
                true
            })
            .filter_map(|&referent| box_of(boxes, referent))
            .filter_map(|placed| Held::read(&self.dom, placed))
            .collect()
    }

    /// The boxes a drag of `held` snaps onto: its siblings and its parent —
    /// the screen's frame when the parent is the screen.
    pub(super) fn snap_targets(&self, held: &[Held], cx: &App) -> Vec<Rect> {
        let Some((screen, boxes)) = self.canvas_boxes(cx) else {
            return Vec::new();
        };
        let Some(parent) = held
            .first()
            .and_then(|first| self.dom.parent(first.referent))
        else {
            return Vec::new();
        };
        let carried = |referent: Ref| held.iter().any(|h| h.referent == referent);
        let mut targets: Vec<Rect> = self
            .dom
            .get(parent)
            .map(|instance| instance.children().to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter(|&child| !carried(child))
            .filter_map(|child| box_of(&boxes, child))
            .map(|placed| Rect::of(placed).turned_bounds(placed.rotation))
            .collect();
        let parent_box = match parent == screen.referent {
            true => Some(screen),
            false => box_of(&boxes, parent).copied(),
        };
        targets.extend(parent_box.map(|placed| Rect::of(&placed)));
        targets
    }

    /// One step of a drag in flight, to the pointer at `at`.
    fn drag_to(
        &mut self,
        gesture: Gesture,
        at: [f32; 2],
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) -> Gesture {
        let view = self.ui.view;
        // Ctrl held lets go of every snap, Sketch's convention.
        let snapping = !modifiers.control;
        let reach = SNAP_REACH / view.zoom;
        match gesture {
            Gesture::Move {
                at: start,
                held,
                first,
                ..
            } => {
                let mut shift = [0, 1].map(|axis| (at[axis] - start[axis]) / view.zoom);
                let bounds = held
                    .iter()
                    .map(|h| h.rect.turned_bounds(h.rotation))
                    .reduce(|a, b| a.union(&b));
                self.ui.guides.clear();
                if let (Some(bounds), true) = (bounds, snapping) {
                    let targets = self.snap_targets(&held, cx);
                    let snap = guides::snap_move(&bounds.shifted(shift), &targets, reach);
                    shift = [0, 1].map(|axis| shift[axis] + snap[axis]);
                    self.ui.guides = guides::guides(&bounds.shifted(shift), &targets);
                }
                let writes: Vec<(Ref, &str, String)> = held
                    .iter()
                    .map(|h| {
                        let moved = position_shift(shift, h.parent_rotation(), h.anchor, [0.0; 2]);
                        (
                            h.referent,
                            "Position",
                            udim2_text(shifted(h.position, moved)),
                        )
                    })
                    .collect();
                self.write_drag(first, &writes, cx);
                Gesture::Move {
                    at: start,
                    held,
                    shift,
                    first: false,
                }
            }
            Gesture::Resize {
                at: start,
                held,
                handle,
                first,
                ..
            } => {
                let travel = [0, 1].map(|axis| (at[axis] - start[axis]) / view.zoom);
                let size = [held.rect.w, held.rect.h];
                let mut local = rotate(travel, -held.rotation);
                self.ui.guides.clear();
                // Snapping an edge is only meaningful while the box is
                // square to the screen; a turned one has no edge on a line.
                let targets = match snapping && held.rotation == 0.0 {
                    true => self.snap_targets(std::slice::from_ref(&held), cx),
                    false => Vec::new(),
                };
                let sides = [handle.0, handle.1];
                for axis in [0, 1] {
                    if sides[axis] == 0 || targets.is_empty() {
                        continue;
                    }
                    let step = resize(handle, size, local, false);
                    let (start_edge, length) = held.rect.along(axis);
                    let centre = start_edge + length * 0.5 + step.centre[axis];
                    let half = (length + step.grow[axis]) * 0.5;
                    let edge = centre + f32::from(sides[axis]) * half;
                    if let Some(snap) =
                        guides::snap_axis(&[edge], &guides::lines_of(&targets, axis), reach)
                    {
                        local[axis] += snap;
                    }
                }
                let step = resize(handle, size, local, modifiers.shift);
                let centre = rotate(step.centre, held.rotation);
                let rect = Rect {
                    x: held.rect.x + centre[0] - step.grow[0] * 0.5,
                    y: held.rect.y + centre[1] - step.grow[1] * 0.5,
                    w: held.rect.w + step.grow[0],
                    h: held.rect.h + step.grow[1],
                };
                if !targets.is_empty() {
                    self.ui.guides = guides::guides(&rect, &targets);
                }
                let moved = position_shift(centre, held.parent_rotation(), held.anchor, step.grow);
                let writes = [
                    (
                        held.referent,
                        "Size",
                        udim2_text(shifted(held.size, step.grow)),
                    ),
                    (
                        held.referent,
                        "Position",
                        udim2_text(shifted(held.position, moved)),
                    ),
                ];
                self.write_drag(first, &writes, cx);
                Gesture::Resize {
                    at: start,
                    held,
                    handle,
                    rect,
                    first: false,
                }
            }
            Gesture::Rotate {
                held, from, first, ..
            } => {
                let now = angle_of(held.rect.centre(), view.to_canvas(at));
                let mut rotation = held.own_rotation + (now - from);
                if modifiers.shift {
                    rotation = round_to(rotation, ROTATE_STEP);
                }
                let text = format!("{}", (rotation * 100.0).round() / 100.0);
                self.write_drag(first, &[(held.referent, "Rotation", text)], cx);
                Gesture::Rotate {
                    held,
                    from,
                    rotation,
                    first: false,
                }
            }
            Gesture::Marquee { from, extend, .. } => Gesture::Marquee {
                from,
                to: view.to_canvas(at),
                extend,
            },
            Gesture::Pan { last } => {
                self.ui.view.pan =
                    [0, 1].map(|axis| self.ui.view.pan[axis] + at[axis] - last[axis]);
                self.ui.fitted = false;
                Gesture::Pan { last: at }
            }
            pressed @ Gesture::Pressed { .. } => pressed,
        }
    }

    pub(super) fn canvas_release(&mut self, cx: &mut Context<Self>) {
        let Some(gesture) = self.ui.gesture.take() else {
            return;
        };
        self.ui.guides.clear();
        match gesture {
            // A click, not a drag: re-pick what is under the pointer inside
            // a selection the press held, or clear on empty canvas.
            Gesture::Pressed { at, press } => match press {
                Press::Element {
                    hit,
                    repick: true,
                    extend,
                } => {
                    let point = self.ui.view.to_canvas(at);
                    let topmost = self
                        .canvas_boxes(cx)
                        .and_then(|(_, boxes)| ui_canvas::hit(&boxes, point))
                        .unwrap_or(hit);
                    match extend {
                        true => self.extend_selection(topmost, cx),
                        false if self.selected_all() != [topmost] => self.select(topmost, cx),
                        false => {}
                    }
                }
                Press::Empty { extend: false } => self.deselect(cx),
                _ => {}
            },
            Gesture::Marquee { from, to, extend } => {
                let Some((_, boxes)) = self.canvas_boxes(cx) else {
                    return;
                };
                let taken =
                    ui_canvas::marquee(&boxes, Rect::spanning(from, to), |r| self.dom.parent(r));
                let mut selection = match extend {
                    true => self.selected_all().to_vec(),
                    false => Vec::new(),
                };
                for referent in taken {
                    if !selection.contains(&referent) {
                        selection.push(referent);
                    }
                }
                self.reselect(selection, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    /// The wheel: pan, or with Ctrl (Cmd) held zoom about the pointer.
    pub(super) fn canvas_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(16.));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if event.modifiers.control || event.modifiers.platform {
            let at = self.panel_point(event.position);
            self.ui.view = self.ui.view.zoomed((dy * 0.0025).exp(), at);
        } else {
            // Shift turns a plain wheel sideways, as it does in a browser.
            let (dx, dy) = match event.modifiers.shift && dx == 0.0 {
                true => (dy, 0.0),
                false => (dx, dy),
            };
            self.ui.view.pan = [self.ui.view.pan[0] + dx, self.ui.view.pan[1] + dy];
        }
        self.ui.fitted = false;
        cx.notify();
    }

    /// Keys typed with the canvas focused: the arrows nudge the selection a
    /// pixel (ten with Shift), Delete removes it — through the Explorer's
    /// own delete. Undo, redo, save and group are the window's, and reach
    /// here like anywhere else.
    pub(super) fn canvas_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let step = if keystroke.modifiers.shift { 10.0 } else { 1.0 };
        let nudge = match keystroke.key.as_str() {
            "left" => [-step, 0.0],
            "right" => [step, 0.0],
            "up" => [0.0, -step],
            "down" => [0.0, step],
            "delete" | "backspace" => {
                self.delete_selected(cx);
                return true;
            }
            _ => return false,
        };
        let Some((_, boxes)) = self.canvas_boxes(cx) else {
            return false;
        };
        let writes: Vec<(Ref, &str, String)> = self
            .held_selection(&boxes)
            .iter()
            .map(|h| {
                let moved = position_shift(nudge, h.parent_rotation(), h.anchor, [0.0; 2]);
                (
                    h.referent,
                    "Position",
                    udim2_text(shifted(h.position, moved)),
                )
            })
            .collect();
        if writes.is_empty() {
            return false;
        }
        self.write_drag(true, &writes, cx);
        true
    }

    pub(super) fn canvas_modifiers(&mut self, modifiers: Modifiers, cx: &mut Context<Self>) {
        if modifiers.alt != self.ui.measuring {
            self.ui.measuring = modifiers.alt;
            cx.notify();
        }
    }
}

/// Whether an element is big enough on screen for its handles to be told
/// apart from its body.
fn roomy(placed: &GuiBox, zoom: f32) -> bool {
    let [_, _, w, h] = placed.rect;
    w.min(h) * zoom > HANDLE_REACH * 4.0
}

/// Where the rotation knob stands: above the middle of the top edge, as
/// far out as `KNOB_OFFSET` panel pixels, turned with the box.
pub(super) fn knob(rect: &Rect, degrees: f32, zoom: f32) -> [f32; 2] {
    let c = rect.centre();
    let [x, y] = rotate([0.0, -(rect.h * 0.5 + KNOB_OFFSET / zoom)], degrees);
    [c[0] + x, c[1] + y]
}

/// What a gesture in flight shows for what it carries, ahead of the canvas
/// catching up with the writes: each element's box and turn.
pub(super) fn preview(gesture: &Gesture) -> Vec<(Ref, Rect, f32)> {
    match gesture {
        Gesture::Move { held, shift, .. } => held
            .iter()
            .map(|h| (h.referent, h.rect.shifted(*shift), h.rotation))
            .collect(),
        Gesture::Resize { held, rect, .. } => vec![(held.referent, *rect, held.rotation)],
        Gesture::Rotate { held, rotation, .. } => {
            vec![(held.referent, held.rect, held.parent_rotation() + rotation)]
        }
        _ => Vec::new(),
    }
}

/// The drag threshold, for the canvas to tell whether a gesture has begun.
pub(super) fn dragging(gesture: Option<&Gesture>) -> bool {
    !matches!(gesture, None | Some(Gesture::Pressed { .. }))
}
