//! What sits over the canvas rather than on it: the note when graph and
//! code disagree, and the minimap.

use gpui_kit::*;
use rbx_dom::Ref;

use super::super::{style, Gesture, GraphEditor};
use super::{point_at, MINIMAP};
use crate::script_editor::graph::catalog;
use crate::script_editor::graph::layout::{self, Rect};
use crate::script_editor::graph::Graph;
use crate::tokens;
use crate::ui_canvas::View;

use super::super::super::Shell;

/// A warning across the top when the script does not parse and so is shown
/// as one code block.
pub(super) fn banner(editor: &GraphEditor) -> Option<AnyElement> {
    let broken = editor.broken.as_ref()?;
    let text = format!(
        "This script has a syntax error on line {}, so it is shown as one code block. Fix it in Code mode to see it as nodes.",
        broken.line
    );
    Some(
        div()
            .absolute()
            .top(px(10.0))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .id("graph-broken-banner")
                    .role(Role::Alert)
                    .aria_label(format!("{text} {}", broken.message))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .px(px(12.0))
                    .py(px(6.0))
                    .max_w(px(640.0))
                    .rounded(tokens::radius())
                    .bg(tokens::tile())
                    .border_1()
                    .border_color(tokens::border())
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text())
                    .child(text)
                    .child(
                        div()
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text_muted())
                            .child(broken.message.clone()),
                    ),
            )
            .into_any_element(),
    )
}

/// The whole graph in miniature with the visible part outlined; a press
/// or drag in it centres the view there.
pub(super) fn minimap(
    editor: &GraphEditor,
    graph: &Graph,
    reference: Ref,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let panel = editor.panel_size();
    let view = editor.view;
    let seen = {
        let a = view.to_canvas([0.0, 0.0]);
        let b = view.to_canvas(panel);
        Rect::spanning(a, b)
    };
    let world = layout::extent(graph).map_or(seen, |extent| extent.union(&seen));
    let scale = (MINIMAP[0] / world.w.max(1.0)).min(MINIMAP[1] / world.h.max(1.0)) * 0.9;
    let offset = [
        (MINIMAP[0] - world.w * scale) * 0.5,
        (MINIMAP[1] - world.h * scale) * 0.5,
    ];
    let map = move |r: Rect| Rect {
        x: offset[0] + (r.x - world.x) * scale,
        y: offset[1] + (r.y - world.y) * scale,
        w: (r.w * scale).max(2.0),
        h: (r.h * scale).max(2.0),
    };
    let boxes: Vec<(Rect, Rgba)> = graph
        .nodes
        .iter()
        .map(|node| {
            let colour = catalog::kind(&node.kind).map_or(tokens::border(), |kind| {
                style::header(kind.category).1.opacity(0.55)
            });
            (map(layout::rect(graph, node)), colour)
        })
        .collect();
    let window_rect = map(seen);
    let origin = std::rc::Rc::new(std::cell::Cell::new(Point::default()));
    let down = origin.clone();
    div()
        .id("graph-minimap")
        .absolute()
        .right(px(12.0))
        .bottom(px(12.0))
        .w(px(MINIMAP[0]))
        .h(px(MINIMAP[1]))
        .bg(rgba(0x16171aee))
        .border_1()
        .border_color(tokens::border())
        .rounded(tokens::radius())
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |shell, event: &MouseDownEvent, _, cx| {
                let frame = MinimapFrame {
                    origin: down.get(),
                    world,
                    offset,
                    scale,
                };
                if let Some(editor) = shell.graphs.get_mut(&reference) {
                    let size = editor.panel_size();
                    frame.centre(&mut editor.view, size, event.position);
                    editor.fitted = false;
                    // The canvas's own move and release carry the drag on,
                    // with the map as the press found it: the map rescales
                    // as the view moves, and must not shift under the pointer.
                    editor.gesture = Some(Gesture::Minimap(frame));
                }
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .child(
            canvas(
                move |laid_out, _, _| origin.set(laid_out.origin),
                move |laid_out, _, window, _| {
                    let at = |p: [f32; 2]| point_at(laid_out.origin, p);
                    for (rect, colour) in &boxes {
                        window.paint_quad(fill(
                            Bounds::from_corners(
                                at([rect.x, rect.y]),
                                at([rect.x + rect.w, rect.y + rect.h]),
                            ),
                            *colour,
                        ));
                    }
                    let r = window_rect;
                    window.paint_quad(
                        outline(
                            Bounds::from_corners(at([r.x, r.y]), at([r.x + r.w, r.y + r.h])),
                            tokens::check_on(),
                            BorderStyle::Solid,
                        )
                        .border_widths(px(1.0)),
                    );
                },
            )
            .size_full(),
        )
        .into_any_element()
}

/// Where the minimap was, and how it mapped the canvas, when it was pressed.
#[derive(Debug, Clone, Copy)]
pub(in crate::shell) struct MinimapFrame {
    origin: Point<Pixels>,
    world: Rect,
    offset: [f32; 2],
    scale: f32,
}

impl MinimapFrame {
    /// Pans `view` (of a panel `size` big) to centre on the canvas point
    /// under `position` on the map.
    pub(in crate::shell) fn centre(
        &self,
        view: &mut View,
        size: [f32; 2],
        position: Point<Pixels>,
    ) {
        let local = [
            f32::from(position.x - self.origin.x),
            f32::from(position.y - self.origin.y),
        ];
        let target = [
            self.world.x + (local[0] - self.offset[0]) / self.scale,
            self.world.y + (local[1] - self.offset[1]) / self.scale,
        ];
        view.pan = [
            size[0] * 0.5 - target[0] * view.zoom,
            size[1] * 0.5 - target[1] * view.zoom,
        ];
    }
}
