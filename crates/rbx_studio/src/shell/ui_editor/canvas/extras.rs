//! The overlays that come and go with what is selected: the corner radius
//! handles, the auto layout on show (its container, its children's places
//! in it, its gaps and padding), where a reordered child would land, and —
//! with a modifier selected — the element it shapes.

use gpui_kit::*;
use rbx_viewer::GuiBox;

use super::super::gesture::{self, Gesture};
use super::{labels, Shape, Shell};
use crate::tokens;
use crate::ui_canvas::{box_of, Rect, View};

impl Shell {
    pub(super) fn overlay_extras(
        &self,
        root: Option<&GuiBox>,
        boxes: &[GuiBox],
        view: View,
        shapes: &mut Vec<Shape>,
        labels: &mut Vec<AnyElement>,
    ) {
        let accent = tokens::check_on();
        let corners = |rect: &Rect, degrees: f32| -> Vec<[f32; 2]> {
            rect.corners(degrees)
                .iter()
                .map(|&p| view.to_view(p))
                .collect()
        };

        // A modifier has no box of its own: what it shapes is outlined.
        if let [only] = self.selected_all() {
            if box_of(boxes, *only).is_none() {
                if let Some(placed) = self.dom.parent(*only).and_then(|p| box_of(boxes, p)) {
                    shapes.push(Shape::Outline(
                        corners(&Rect::of(placed), placed.rotation),
                        accent,
                        1.0,
                    ));
                }
            }
        }

        if let Some(shown) = root.and_then(|root| self.shown_layout(root, boxes)) {
            let spacing = tokens::tool_scale();
            shapes.push(Shape::Outline(corners(&shown.container, 0.0), spacing, 1.0));
            for band in &shown.bands {
                let alpha = if band.gap { 0.28 } else { 0.14 };
                shapes.push(Shape::Fill(
                    corners(&band.rect, 0.0),
                    spacing.opacity(alpha),
                ));
            }
            for (index, (_, rect)) in shown.children.iter().enumerate() {
                labels.push(labels::tag(
                    view.to_view([rect.x, rect.y]),
                    format!("{}", index + 1),
                ));
            }
        }

        if let Some(Gesture::Reorder {
            others,
            grid,
            axis,
            to,
            ..
        }) = &self.ui.gesture
        {
            let index =
                crate::shell::ui_editor::layout_overlay::drop_index(others, *to, *grid, *axis);
            match (*grid, others.get(index), others.get(index.wrapping_sub(1))) {
                // A grid lands on a cell: that cell lights up.
                (true, Some((_, cell)), _) => {
                    shapes.push(Shape::Outline(corners(cell, 0.0), accent, 2.0));
                }
                // A list lands between two: a bar across the gap.
                (false, next, previous) => {
                    let edge = |rect: &Rect, end: bool| {
                        let (start, length) = rect.along(*axis);
                        if end {
                            start + length
                        } else {
                            start
                        }
                    };
                    let at = match (previous, next) {
                        (Some((_, a)), Some((_, b))) => (edge(a, true) + edge(b, false)) * 0.5,
                        (Some((_, a)), None) => edge(a, true),
                        (None, Some((_, b))) => edge(b, false),
                        (None, None) => return,
                    };
                    let across = previous.or(next).map(|(_, r)| *r).unwrap_or_default();
                    let (from, length) = across.along(1 - *axis);
                    let (a, b) = match axis {
                        0 => ([at, from], [at, from + length]),
                        _ => ([from, at], [from + length, at]),
                    };
                    shapes.push(Shape::Line(view.to_view(a), view.to_view(b), accent));
                }
                _ => {}
            }
        }

        if self.radius_shown() {
            let frame = match &self.ui.gesture {
                Some(Gesture::Radius { frame, .. }) => Some(*frame),
                _ => self.selection_frame(boxes),
            };
            if let Some((rect, degrees)) = frame {
                for (_, at) in gesture::radius_handles(&rect, degrees, self.radii(), view.zoom) {
                    shapes.push(Shape::Dot(view.to_view(at)));
                }
            }
        }
    }
}
