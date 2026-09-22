//! The canvas element: the chosen screen as the renderer drew it, placed by
//! the pan and zoom, and every overlay over it — the screen's edge, the
//! hover and selection outlines, the handles, the guides a drag snapped
//! onto, the marquee, and the Alt distance readout.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_viewer::GuiBox;

use super::gesture::{self, Gesture};
use super::Shell;
use crate::tokens;
use crate::ui_canvas::guides::{self, Distance};
use crate::ui_canvas::{box_of, Handle, Rect, View};

/// Room left round the screen when it is fitted to the panel.
const FIT_MARGIN: f32 = 24.0;
/// A resize handle's side, in panel pixels.
const HANDLE_SIZE: f32 = 8.0;

/// One thing the overlay paints, in panel pixels.
enum Shape {
    Outline(Vec<[f32; 2]>, Rgba, f32),
    Fill(Vec<[f32; 2]>, Rgba),
    Line([f32; 2], [f32; 2], Rgba),
    Handle([f32; 2], bool),
}

impl Shell {
    pub(super) fn ui_canvas(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.sync_size_fields(window, cx);
        let panel = self.ui.bounds.get().size;
        let panel = [f32::from(panel.width), f32::from(panel.height)];
        let (width, height) = self.ui.resolution;
        let screen_size = [width as f32, height as f32];
        if self.ui.fitted && panel[0] > 0.0 && panel[1] > 0.0 {
            self.ui.view = View::fit(panel, screen_size, FIT_MARGIN);
        }
        let view = self.ui.view;
        let request = self.canvas_request();
        let image = self
            .viewport
            .read(cx)
            .canvas()
            .filter(|canvas| Some(canvas.request) == request)
            .map(|canvas| canvas.image.clone());
        let (screen_box, boxes) = match self.canvas_boxes(cx) {
            Some((screen, boxes)) => (Some(screen), boxes),
            None => (None, Vec::new()),
        };
        let (shapes, labels) = self.overlay(screen_box, &boxes, view, screen_size);
        let hint = self.canvas_hint(request.is_some());
        let bounds = self.ui.bounds.clone();
        let origin = view.to_view([0.0, 0.0]);

        div()
            .id("ui-canvas")
            .track_focus(&self.ui.focus)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(tokens::black())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|shell, event: &MouseDownEvent, window, cx| {
                    shell.canvas_press(event, window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|shell, event: &MouseDownEvent, _, cx| {
                    shell.canvas_pan_press(event, cx);
                }),
            )
            .on_mouse_move(cx.listener(|shell, event: &MouseMoveEvent, _, cx| {
                shell.canvas_moved(event, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|shell, _: &MouseUpEvent, _, cx| shell.canvas_release(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|shell, _: &MouseUpEvent, _, cx| shell.canvas_release(cx)),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|shell, _: &MouseUpEvent, _, cx| shell.canvas_release(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Middle,
                cx.listener(|shell, _: &MouseUpEvent, _, cx| shell.canvas_release(cx)),
            )
            .on_scroll_wheel(cx.listener(|shell, event: &ScrollWheelEvent, _, cx| {
                shell.canvas_wheel(event, cx);
            }))
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, _, cx| {
                if shell.canvas_key(&event.keystroke, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_modifiers_changed(cx.listener(|shell, event: &ModifiersChangedEvent, _, cx| {
                shell.canvas_modifiers(event.modifiers, cx);
            }))
            .when_some(image, |this, image| {
                this.child(
                    img(image)
                        .absolute()
                        .left(px(origin[0]))
                        .top(px(origin[1]))
                        .w(px(screen_size[0] * view.zoom))
                        .h(px(screen_size[1] * view.zoom))
                        .object_fit(ObjectFit::Fill),
                )
            })
            .child(
                canvas(
                    move |laid_out, _, _| bounds.set(laid_out),
                    move |laid_out, _, window, _| paint(&shapes, laid_out.origin, window),
                )
                .absolute()
                .size_full(),
            )
            .children(labels)
            .children(hint)
            .into_any_element()
    }

    /// Everything drawn over the picture, and the distance labels.
    fn overlay(
        &self,
        screen_box: Option<GuiBox>,
        boxes: &[GuiBox],
        view: View,
        screen: [f32; 2],
    ) -> (Vec<Shape>, Vec<AnyElement>) {
        let accent = tokens::check_on();
        let guide = tokens::tool_scale();
        let corners = |rect: &Rect, degrees: f32| -> Vec<[f32; 2]> {
            rect.corners(degrees)
                .iter()
                .map(|&p| view.to_view(p))
                .collect()
        };
        let mut shapes = vec![Shape::Outline(
            corners(
                &Rect {
                    w: screen[0],
                    h: screen[1],
                    ..Rect::default()
                },
                0.0,
            ),
            tokens::divider(),
            1.0,
        )];

        // What the gesture in flight carries is drawn where it is going,
        // not where the last frame put it.
        let preview = self
            .ui
            .gesture
            .as_ref()
            .map(gesture::preview)
            .unwrap_or_default();
        let placed = |referent| -> Option<(Rect, f32)> {
            preview
                .iter()
                .find(|(r, _, _)| *r == referent)
                .map(|&(_, rect, degrees)| (rect, degrees))
                .or_else(|| box_of(boxes, referent).map(|b| (Rect::of(b), b.rotation)))
        };

        let selected = self.selected_all();
        if !gesture::dragging(self.ui.gesture.as_ref()) {
            if let Some((rect, degrees)) = self
                .ui
                .hovered
                .filter(|hovered| !selected.contains(hovered))
                .and_then(placed)
            {
                shapes.push(Shape::Outline(corners(&rect, degrees), accent, 1.0));
            }
        }
        let outlined: Vec<(Rect, f32)> = selected.iter().filter_map(|&r| placed(r)).collect();
        for (rect, degrees) in &outlined {
            shapes.push(Shape::Outline(corners(rect, *degrees), accent, 1.0));
        }
        match outlined.as_slice() {
            [(rect, degrees)] if selected.len() == 1 => {
                let top = view.to_view(Handle(0, -1).at(rect, *degrees));
                let knob = view.to_view(gesture::knob(rect, *degrees, view.zoom));
                shapes.push(Shape::Line(top, knob, accent));
                shapes.push(Shape::Handle(knob, true));
                for handle in Handle::ALL {
                    shapes.push(Shape::Handle(
                        view.to_view(handle.at(rect, *degrees)),
                        false,
                    ));
                }
            }
            [first, rest @ ..] if !rest.is_empty() => {
                let union = rest
                    .iter()
                    .fold(first.0.turned_bounds(first.1), |u, (r, d)| {
                        u.union(&r.turned_bounds(*d))
                    });
                shapes.push(Shape::Outline(corners(&union, 0.0), accent, 1.0));
            }
            _ => {}
        }

        for g in &self.ui.guides {
            let (a, b) = match g.axis {
                0 => ([g.at, g.from], [g.at, g.to]),
                _ => ([g.from, g.at], [g.to, g.at]),
            };
            shapes.push(Shape::Line(view.to_view(a), view.to_view(b), guide));
        }

        if let Some(Gesture::Marquee { from, to, .. }) = &self.ui.gesture {
            let area = corners(&Rect::spanning(*from, *to), 0.0);
            shapes.push(Shape::Fill(area.clone(), accent.opacity(0.12)));
            shapes.push(Shape::Outline(area, accent, 1.0));
        }

        let mut labels = Vec::new();
        for distance in self.measurement(screen_box, boxes, &outlined) {
            let (a, b) = (view.to_view(distance.a), view.to_view(distance.b));
            shapes.push(Shape::Line(a, b, guide));
            labels.push(distance_label(
                [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5],
                distance.length,
            ));
        }
        (shapes, labels)
    }

    /// Alt held with something selected: the distances from the selection
    /// to what the pointer is over — or, over nothing or over the
    /// selection itself, to the selection's parent.
    fn measurement(
        &self,
        screen_box: Option<GuiBox>,
        boxes: &[GuiBox],
        outlined: &[(Rect, f32)],
    ) -> Vec<Distance> {
        if !self.ui.measuring || self.ui.gesture.is_some() {
            return Vec::new();
        }
        let Some(from) = outlined
            .iter()
            .map(|(r, d)| r.turned_bounds(*d))
            .reduce(|a, b| a.union(&b))
        else {
            return Vec::new();
        };
        let selected = self.selected_all();
        let target = self
            .ui
            .hovered
            .filter(|hovered| !selected.contains(hovered))
            .or_else(|| selected.first().and_then(|&r| self.dom.parent(r)))
            .and_then(|r| box_of(boxes, r).or(screen_box.as_ref().filter(|s| s.referent == r)))
            .map(|placed| Rect::of(placed).turned_bounds(placed.rotation));
        target
            .map(|to| guides::measure(&from, &to))
            .unwrap_or_default()
    }

    /// What the canvas says when there is nothing on it to edit.
    fn canvas_hint(&self, drawing: bool) -> Option<AnyElement> {
        let text = match (drawing, self.ui.screen.is_some()) {
            (true, _) => return None,
            (false, true) => "The ScreenGui on the canvas is gone.",
            (false, false) => {
                "Select a ScreenGui, or anything inside one, to put it on the canvas. \
                 BillboardGui and SurfaceGui are drawn in the 3D view."
            }
        };
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .text_size(tokens::text_md())
                .text_color(tokens::text_muted())
                .child(text)
                .into_any_element(),
        )
    }
}

/// A distance's number, over the middle of its run.
fn distance_label(at: [f32; 2], length: f32) -> AnyElement {
    div()
        .absolute()
        .left(px(at[0] + 4.0))
        .top(px(at[1] + 4.0))
        .px(px(3.))
        .rounded(tokens::RADIUS_TINY)
        .bg(tokens::tool_scale())
        .text_size(tokens::text_xs())
        .line_height(tokens::line_xs())
        .text_color(tokens::black())
        .child(format!("{}", length.round()))
        .into_any_element()
}

fn paint(shapes: &[Shape], origin: Point<Pixels>, window: &mut Window) {
    let at = |p: [f32; 2]| point(origin.x + px(p[0]), origin.y + px(p[1]));
    for shape in shapes {
        match shape {
            Shape::Outline(points, colour, width) => {
                let mut builder = PathBuilder::stroke(px(*width));
                let points: Vec<_> = points.iter().map(|&p| at(p)).collect();
                builder.add_polygon(&points, true);
                if let Ok(path) = builder.build() {
                    window.paint_path(path, *colour);
                }
            }
            Shape::Fill(points, colour) => {
                let mut builder = PathBuilder::fill();
                let points: Vec<_> = points.iter().map(|&p| at(p)).collect();
                builder.add_polygon(&points, true);
                if let Ok(path) = builder.build() {
                    window.paint_path(path, *colour);
                }
            }
            Shape::Line(a, b, colour) => {
                let mut builder = PathBuilder::stroke(px(1.0));
                builder.add_polygon(&[at(*a), at(*b)], false);
                if let Ok(path) = builder.build() {
                    window.paint_path(path, *colour);
                }
            }
            Shape::Handle(centre, round) => {
                let half = HANDLE_SIZE * 0.5;
                let square = |half: f32| Bounds {
                    origin: at([centre[0] - half, centre[1] - half]),
                    size: size(px(half * 2.0), px(half * 2.0)),
                };
                let radius = if *round { px(half) } else { px(0.) };
                window.paint_quad(fill(square(half), tokens::check_on()).corner_radii(radius));
                window
                    .paint_quad(fill(square(half - 1.0), tokens::text_full()).corner_radii(radius));
            }
        }
    }
}
