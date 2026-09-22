//! A gesture's two ends: what a press that has turned into a drag takes
//! hold of, and what letting go does — a click's pick, a marquee's
//! selection, a drawn element, a reorder landing.

use gpui_kit::*;
use rbx_dom::Ref;

use super::super::super::Shell;
use super::super::inspector::Key;
use super::super::layout_overlay::drop_index;
use super::handles::{corner_key, radius_at};
use super::{Gesture, Press};
use crate::ui_canvas::{self, angle_of, Rect};

impl Shell {
    /// A press that has moved far enough to be a drag: what it grabs.
    pub(super) fn begin_drag(
        &mut self,
        at: [f32; 2],
        press: Press,
        cx: &mut Context<Self>,
    ) -> Option<Gesture> {
        let (root, boxes) = self.canvas_boxes(cx)?;
        let held = self.held_selection(&root, &boxes);
        let frame = self.selection_frame(&boxes);
        Some(match press {
            Press::Element { .. } => {
                let to = self.ui.view.to_canvas(at);
                match self.reorder_of(&held, &root, &boxes, to) {
                    Some(reorder) => reorder,
                    None => Gesture::Move {
                        at,
                        held,
                        shift: [0.0, 0.0],
                        first: true,
                    },
                }
            }
            Press::Radius(corner, alone) => {
                let frame = frame?;
                let start = self.begin_value_drag(match alone {
                    true => corner_key(corner),
                    false => Key::Radius,
                });
                let grab = radius_at(&frame.0, frame.1, corner, self.ui.view.to_canvas(at));
                Gesture::Radius {
                    corner,
                    frame,
                    start,
                    grab,
                }
            }
            Press::Band(band) => Gesture::Band {
                band,
                at,
                start: self.begin_value_drag(band.key),
            },
            Press::Handle(handle) => {
                let frame = frame.filter(|_| !held.is_empty())?;
                Gesture::Resize {
                    at,
                    held,
                    handle,
                    frame,
                    shown: frame,
                    preview: Vec::new(),
                    first: true,
                }
            }
            Press::Knob => {
                let frame = frame.filter(|_| !held.is_empty())?;
                Gesture::Rotate {
                    held,
                    frame,
                    from: angle_of(frame.0.centre(), self.ui.view.to_canvas(at)),
                    delta: 0.0,
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
            Press::Draw(class) => {
                let from = self.ui.view.to_canvas(at);
                Gesture::Draw {
                    class,
                    from,
                    to: from,
                }
            }
        })
    }

    pub(in crate::shell::ui_editor) fn canvas_release(&mut self, cx: &mut Context<Self>) {
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
                Press::Draw(class) => {
                    let point = self.ui.view.to_canvas(at);
                    self.finish_draw(class, point, None, cx);
                }
                _ => {}
            },
            Gesture::Draw { class, from, to } => self.finish_draw(class, from, Some(to), cx),
            Gesture::Radius { .. } | Gesture::Band { .. } => self.end_inspector_drag(),
            Gesture::Reorder {
                referent,
                layout,
                others,
                grid,
                axis,
                to,
            } => {
                let index = drop_index(&others, to, grid, axis);
                let mut order: Vec<Ref> = others.iter().map(|(r, _)| *r).collect();
                order.push(referent);
                self.reorder_gui(layout, order, referent, index, cx);
            }
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
