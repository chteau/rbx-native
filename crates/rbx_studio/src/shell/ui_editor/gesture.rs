//! What the pointer, the wheel and the keyboard do on the canvas: select,
//! move, resize, rotate, marquee, pan, zoom and nudge.
//!
//! A drag is always measured from where it started against a snapshot of
//! what it grabbed ([`Held`]), never step on step: the boxes the canvas is
//! hit-tested against come back from the render thread a frame after each
//! write, and a gesture that read them mid-drag would chase its own tail.
//! Every step writes through `Shell::write_drag` — one undo entry per
//! gesture, the Properties panel's own commit.

mod frame;
mod handles;
pub(super) mod held;
mod input;
mod lifecycle;
mod step;

pub(super) use frame::{frame, frame_of, preview};
pub(super) use handles::{radius_at, radius_handles};
pub(super) use held::Held;

use gpui_kit::*;
use rbx_dom::Ref;
use rbx_viewer::GuiBox;

use super::super::Shell;
use super::layout_overlay::Band;
use crate::ui_canvas::{self, box_of, rotate, Handle, Rect};

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
    /// With a tool armed: a drag draws that class, a click puts one down.
    Draw(&'static str),
    /// A corner's radius handle, by which side of the box it is on — that
    /// corner alone with Alt held, Sketch's convention, all four without.
    Radius([i8; 2], bool),
    /// A gap or a side of the padding in the auto layout on show.
    Band(Band),
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
    /// The selection's frame — its one element's own box, or the box
    /// round several — dragged by a handle: `frame` as grabbed, `shown` as
    /// it stands now, and each element's box as it stands now.
    Resize {
        at: [f32; 2],
        held: Vec<Held>,
        handle: Handle,
        frame: (Rect, f32),
        shown: (Rect, f32),
        preview: Vec<(Ref, Rect, f32)>,
        first: bool,
    },
    /// The selection turned about its frame's centre by `delta` degrees.
    Rotate {
        held: Vec<Held>,
        frame: (Rect, f32),
        from: f32,
        delta: f32,
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
    /// An element being drawn, from corner to corner in canvas pixels.
    Draw {
        class: &'static str,
        from: [f32; 2],
        to: [f32; 2],
    },
    /// The corner radius dragged by `corner`'s handle on `frame`: `start`
    /// is what it was, `grab` what the pointer read where it took hold —
    /// the handle stands clear of a square corner, and taking hold of it
    /// must not round the corner by that much.
    Radius {
        corner: [i8; 2],
        frame: (Rect, f32),
        start: f32,
        grab: f32,
    },
    /// A gap or padding band dragged from `at`, which read `start` then.
    Band {
        band: Band,
        at: [f32; 2],
        start: f32,
    },
    /// A child of a list or grid dragged to a new place in it: `others`
    /// are the rest in the layout's order, `to` where it is now.
    Reorder {
        referent: Ref,
        layout: Ref,
        others: Vec<(Ref, Rect)>,
        grid: bool,
        axis: usize,
        to: [f32; 2],
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

    /// The frame the handles are drawn on: the one selected element's own
    /// box, turned as it is, or the box round several — see [`frame_of`].
    pub(super) fn selection_frame(&self, boxes: &[GuiBox]) -> Option<(Rect, f32)> {
        frame_of(
            self.selected_all()
                .iter()
                .filter_map(|&referent| box_of(boxes, referent))
                .map(|placed| (Rect::of(placed), placed.rotation)),
        )
    }

    pub(super) fn canvas_press(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.ui.focus, cx);
        if self.ui.panning {
            self.canvas_pan_press(event, cx);
            return;
        }
        let at = self.panel_point(event.position);
        let Some((_, boxes)) = self.canvas_boxes(cx) else {
            return;
        };
        let view = self.ui.view;
        let point = view.to_canvas(at);
        if self.ui.text_edit.is_some() {
            self.end_text_edit(true, cx);
        }
        if let Some(class) = self.ui.tool {
            self.ui.gesture = Some(Gesture::Pressed {
                at,
                press: Press::Draw(class),
            });
            cx.notify();
            return;
        }
        let frame = self.selection_frame(&boxes);
        let extend = event.modifiers.shift || event.modifiers.control || event.modifiers.platform;
        // A double-click on a selected text element types into it.
        if event.click_count >= 2 {
            if let Some(hit) = ui_canvas::hit(&boxes, point)
                .filter(|&hit| self.selected_all() == [hit] && self.is_text(hit))
            {
                self.begin_text_edit(hit, window, cx);
                return;
            }
        }
        let near = |p: [f32; 2]| {
            let q = view.to_view(p);
            (q[0] - at[0]).hypot(q[1] - at[1]) <= HANDLE_REACH
        };

        // A handle wins over the body it sits on, except inside an element
        // too small on screen to tell the two apart: there the body wins,
        // and the handles are reached from just outside it.
        let press = if let Some((rect, turn)) = frame.filter(|(rect, turn)| {
            !ui_canvas::covers_turned(rect, *turn, point) || roomy(rect, view.zoom)
        }) {
            let grabbed = Handle::ALL
                .into_iter()
                .find(|handle| near(handle.at(&rect, turn)));
            match grabbed {
                Some(handle) => Some(Press::Handle(handle)),
                None => near(knob(&rect, turn, view.zoom)).then_some(Press::Knob),
            }
        } else {
            None
        };
        let press = press
            .or_else(|| {
                let (rect, turn) = frame.filter(|_| self.radius_shown())?;
                radius_handles(&rect, turn, self.radii(), view.zoom)
                    .into_iter()
                    .find(|&(_, at)| near(at))
                    .map(|(corner, _)| Press::Radius(corner, event.modifiers.alt))
            })
            .or_else(|| {
                let (root, _) = self.canvas_boxes(cx)?;
                let shown = self.shown_layout(&root, &boxes)?;
                let reach = HANDLE_REACH / view.zoom;
                shown
                    .bands
                    .into_iter()
                    .find(|band| {
                        let r = band.rect;
                        point[0] >= r.x - reach
                            && point[0] <= r.x + r.w + reach
                            && point[1] >= r.y - reach
                            && point[1] <= r.y + r.h + reach
                            // A band as thin as its reach would swallow
                            // the edge of every child it runs along.
                            && (band.gap || r.along(band.axis).1 > 0.0 || !ui_canvas::covers_turned(&shown.container, 0.0, point))
                    })
                    .map(Press::Band)
            });
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
}

/// Whether a frame is big enough on screen for its handles to be told apart
/// from its body.
fn roomy(rect: &Rect, zoom: f32) -> bool {
    rect.w.min(rect.h) * zoom > HANDLE_REACH * 4.0
}

/// Where the rotation knob stands: above the middle of the top edge, as
/// far out as `KNOB_OFFSET` panel pixels, turned with the box.
pub(super) fn knob(rect: &Rect, degrees: f32, zoom: f32) -> [f32; 2] {
    let c = rect.centre();
    let [x, y] = rotate([0.0, -(rect.h * 0.5 + KNOB_OFFSET / zoom)], degrees);
    [c[0] + x, c[1] + y]
}

/// The drag threshold, for the canvas to tell whether a gesture has begun.
pub(super) fn dragging(gesture: Option<&Gesture>) -> bool {
    !matches!(gesture, None | Some(Gesture::Pressed { .. }))
}
