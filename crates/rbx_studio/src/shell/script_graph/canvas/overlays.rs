//! What sits over the canvas rather than on it: the note when graph and
//! code disagree, and the minimap.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use super::super::{style, GraphEditor};
use super::{point_at, MINIMAP};
use crate::script_editor::graph::catalog;
use crate::script_editor::graph::layout::{self, Rect};
use crate::script_editor::graph::{codegen, Graph};
use crate::tokens;

use super::super::super::Shell;

/// A note across the top when the graph and the code disagree: code edited
/// since the graph was saved, or a script that is still code alone.
pub(super) fn banner(
    editor: &GraphEditor,
    graph: &Graph,
    code: &str,
    reference: Ref,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let compiled = codegen::compile(graph).ok();
    let (text, action) = match (&editor.synced, compiled) {
        (None, _) if !code.trim().is_empty() => (
            "This script is written as code. A graph built here replaces its code once it compiles.",
            false,
        ),
        (Some(_), Some(compiled)) if !graph.nodes.is_empty() && compiled != code => {
            ("The code was edited after this graph was saved.", true)
        }
        _ => return None,
    };
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
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(12.0))
                    .py(px(6.0))
                    .rounded(tokens::radius())
                    .bg(tokens::tile())
                    .border_1()
                    .border_color(tokens::border())
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text())
                    .child(text)
                    .when(action, |this| {
                        this.child(
                            Button::new("graph-replace-code")
                                .label("Replace code with graph")
                                .small()
                                .primary()
                                .on_click(cx.listener(move |shell, _, _, cx| {
                                    shell.graph_to_code(reference, cx);
                                })),
                        )
                    }),
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
    let centre_at = move |shell: &mut Shell, position: Point<Pixels>, origin: Point<Pixels>| {
        let local = [
            f32::from(position.x - origin.x),
            f32::from(position.y - origin.y),
        ];
        let target = [
            world.x + (local[0] - offset[0]) / scale,
            world.y + (local[1] - offset[1]) / scale,
        ];
        if let Some(editor) = shell.graphs.get_mut(&reference) {
            let panel = editor.panel_size();
            editor.view.pan = [
                panel[0] * 0.5 - target[0] * editor.view.zoom,
                panel[1] * 0.5 - target[1] * editor.view.zoom,
            ];
            editor.fitted = false;
        }
    };
    let origin = std::rc::Rc::new(std::cell::Cell::new(Point::default()));
    let (down, moved) = (origin.clone(), origin.clone());
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
                centre_at(shell, event.position, down.get());
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(move |shell, event: &MouseMoveEvent, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                centre_at(shell, event.position, moved.get());
                cx.notify();
            }
        }))
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
