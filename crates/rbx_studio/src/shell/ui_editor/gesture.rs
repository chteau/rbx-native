//! What the pointer, the wheel and the keyboard do on the canvas: select,
//! move, resize, rotate, marquee, pan, zoom and nudge.
//!
//! A drag is always measured from where it started against a snapshot of
//! what it grabbed ([`Held`]), never step on step: the boxes the canvas is
//! hit-tested against come back from the render thread a frame after each
//! write, and a gesture that read them mid-drag would chase its own tail.
//! Every step writes through `Shell::write_drag` — one undo entry per
//! gesture, the Properties panel's own commit.

mod held;
mod input;
mod step;

pub(super) use held::Held;

use gpui_kit::*;
use rbx_dom::Ref;
use rbx_viewer::GuiBox;

use super::super::Shell;
use crate::ui_canvas::{self, angle_of, box_of, rotate, Handle, Rect};

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
