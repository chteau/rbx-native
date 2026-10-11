//! The graph canvas: the dot grid and wires painted, nodes laid out as boxes
//! over them. Drag a node to move it, drag from a node's output (right) to
//! another's input (left) to wire them, drag a wired input away to unwire
//! it, drag the background to pan, and use the wheel to zoom. The pan and
//! zoom are the UI Editor canvas's own `View`, so both canvases move alike.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_animation::graph::{Graph, NodeKind};

use super::{GraphGesture, Shell};
use crate::anim_editor::graph_canvas::{self, Hit, HEADER, PIN_ROW};
use crate::timeline::paint;
use crate::tokens;
use crate::ui_canvas::View;

const ZOOM: (f32, f32) = (0.35, 2.0);

/// A node kind's header tint, by family: sources, pickers, layering, flow.
fn tint(kind: NodeKind) -> Rgba {
    match kind {
        NodeKind::Clip => tokens::tool_move(),
        NodeKind::StateMachine | NodeKind::Select | NodeKind::PrioritySelect => {
            tokens::tool_rotate()
        }
        NodeKind::Sequence | NodeKind::RandomSequence | NodeKind::OneShot => tokens::tool_scale(),
        NodeKind::Blend1D | NodeKind::Blend2D => tokens::tool_transform(),
        NodeKind::Over | NodeKind::Add | NodeKind::Subtract | NodeKind::Mask => {
            tokens::tool_pivot()
        }
        NodeKind::Speed => tokens::tool_align(),
        NodeKind::GraphOutput => tokens::check_on(),
    }
}

fn to_screen(view: View, origin: Point<Pixels>, p: [f32; 2]) -> (f32, f32) {
    (
        f32::from(origin.x) + view.pan[0] + p[0] * view.zoom,
        f32::from(origin.y) + view.pan[1] + p[1] * view.zoom,
    )
}

impl Shell {
    fn anim_graph_point(&self, at: Point<Pixels>) -> Option<[f32; 2]> {
        let b = self.anim.graph.bounds.get()?;
        let v = self.anim.graph.view;
        Some([
            (f32::from(at.x - b.origin.x) - v.pan[0]) / v.zoom,
            (f32::from(at.y - b.origin.y) - v.pan[1]) / v.zoom,
        ])
    }

    pub(in crate::shell::animation) fn anim_graph_canvas(
        &mut self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let doc = &mut self.anim.graph;
        if !doc.fitted {
            if let (Some(b), Some([x, y, w, h])) =
                (doc.bounds.get(), graph_canvas::bounds(&doc.graph))
            {
                let panel = [f32::from(b.size.width), f32::from(b.size.height)];
                doc.view = View::framing(panel, &crate::ui_canvas::Rect { x, y, w, h }, 40.0);
                doc.view.zoom = doc.view.zoom.clamp(ZOOM.0, 1.0);
                doc.fitted = true;
            }
        }
        let view = doc.view;
        let graph = doc.graph.clone();
        let selected = doc.selected;
        let pending = match doc.gesture {
            Some(GraphGesture::Wire { from, to }) => {
                graph_canvas::output_pin(&graph.nodes[from]).map(|o| (o, to))
            }
            _ => None,
        };
        let active: Vec<Option<usize>> = (0..graph.nodes.len())
            .map(|i| {
                doc.player
                    .as_ref()
                    .filter(|_| doc.playing)
                    .and_then(|p| p.active(i))
            })
            .collect();
        let report = doc.bounds.clone();
        let painted = graph.clone();

        let nodes = graph.nodes.iter().enumerate().map(|(i, node)| {
            let [w, h] = graph_canvas::node_size(node);
            let z = view.zoom;
            let header = tint(node.kind);
            div()
                .absolute()
                .left(px(view.pan[0] + node.position.0 * z))
                .top(px(view.pan[1] + node.position.1 * z))
                .w(px(w * z))
                .h(px(h * z))
                .rounded(px(6. * z))
                .bg(tokens::chrome())
                .border(px(if selected == Some(i) { 2. } else { 1. }))
                .border_color(if selected == Some(i) {
                    tokens::check_on()
                } else {
                    tokens::border2()
                })
                .overflow_hidden()
                .child(
                    h_flex()
                        .h(px(HEADER * z))
                        .px(px(8. * z))
                        .gap(px(6. * z))
                        .items_center()
                        .bg(tokens::tool_wash(header))
                        .border_b(px(1.))
                        .border_color(header)
                        .child(div().w(px(6. * z)).h(px(6. * z)).rounded_full().bg(header))
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_size(px(12. * z))
                                .text_color(tokens::text_strong())
                                .child(node.name.clone()),
                        )
                        .when(node.name != node.kind.label(), |this| {
                            this.child(
                                div()
                                    .text_size(px(10. * z))
                                    .text_color(tokens::text_muted())
                                    .child(node.kind.label()),
                            )
                        }),
                )
                .child(
                    v_flex().children(node.inputs.iter().enumerate().map(|(k, input)| {
                        let source = input.node.map(|n| graph.nodes[n].name.clone());
                        let lit = active[i] == Some(k);
                        h_flex()
                            .h(px(PIN_ROW * z))
                            .pl(px(12. * z))
                            .pr(px(8. * z))
                            .gap(px(6. * z))
                            .items_center()
                            .when(lit, |this| this.bg(tokens::accent_soft()))
                            .child(
                                div()
                                    .text_size(px(11. * z))
                                    .text_color(if lit {
                                        tokens::text_full()
                                    } else {
                                        tokens::text_label()
                                    })
                                    .child(input.pin.clone()),
                            )
                            .child(div().flex_1())
                            .children(source.map(|s| {
                                div()
                                    .truncate()
                                    .text_size(px(10. * z))
                                    .text_color(tokens::text_muted())
                                    .child(s)
                            }))
                    })),
                )
        });

        div()
            .id("anim-graph")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(tokens::black())
            .child(
                canvas(
                    move |b, _, _| report.set(Some(b)),
                    move |b, _, window, _| paint_graph(window, b, view, &painted, pending),
                )
                .absolute()
                .size_full(),
            )
            .children(nodes)
            .child(
                canvas(
                    |_, _, _| {},
                    move |b, _, window, _| paint_pins(window, b, view, &graph),
                )
                .absolute()
                .size_full(),
            )
            .when(self.anim.graph.graph.nodes.len() <= 1, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(12.))
                        .left(px(12.))
                        .text_size(tokens::text_sm())
                        .text_color(tokens::text_muted())
                        .child("Add a Clip or a State Machine from the panel, then drag its output (right) into the Output's Pose."),
                )
            })
            .on_mouse_down(MouseButton::Left, cx.listener(|shell, e: &MouseDownEvent, window, cx| {
                window.focus(&shell.anim.focus, cx);
                shell.anim_graph_press(e.position, cx);
            }))
            .on_mouse_down(MouseButton::Middle, cx.listener(|shell, e: &MouseDownEvent, _, _| {
                shell.anim.graph.gesture = Some(GraphGesture::Pan {
                    last: [f32::from(e.position.x), f32::from(e.position.y)],
                });
            }))
            .on_mouse_move(cx.listener(|shell, e: &MouseMoveEvent, _, cx| shell.anim_graph_drag(e.position, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|shell, e: &MouseUpEvent, _, cx| shell.anim_graph_release(e.position, cx)))
            .on_mouse_up(MouseButton::Middle, cx.listener(|shell, e: &MouseUpEvent, _, cx| shell.anim_graph_release(e.position, cx)))
            .on_scroll_wheel(cx.listener(|shell, e: &ScrollWheelEvent, window, cx| {
                let Some(b) = shell.anim.graph.bounds.get() else { return };
                let dy = f32::from(e.delta.pixel_delta(window.line_height()).y);
                let v = &mut shell.anim.graph.view;
                let at = [f32::from(e.position.x - b.origin.x), f32::from(e.position.y - b.origin.y)];
                let zoom = (v.zoom * (dy * 0.002).exp()).clamp(ZOOM.0, ZOOM.1);
                let old = v.zoom;
                for (pan, at) in v.pan.iter_mut().zip(at) {
                    *pan = at - (at - *pan) * zoom / old;
                }
                v.zoom = zoom;
                cx.stop_propagation();
                cx.notify();
            }))
            .on_key_down(cx.listener(|shell, e: &KeyDownEvent, _, cx| {
                if matches!(e.keystroke.key.as_str(), "delete" | "backspace" | "x") {
                    shell.anim_delete_graph_node(cx);
                    cx.stop_propagation();
                } else if e.keystroke.key == "space" {
                    shell.toggle_animation_playback(cx);
                    cx.stop_propagation();
                }
            }))
            .into_any_element()
    }

    fn anim_graph_press(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(p) = self.anim_graph_point(at) else {
            return;
        };
        let doc = &mut self.anim.graph;
        doc.gesture = match graph_canvas::hit(&doc.graph, p) {
            Hit::Output(n) => Some(GraphGesture::Wire { from: n, to: p }),
            Hit::Input(n, k) => {
                // Pulling a wired input picks its wire up from the source end.
                let source = doc.graph.nodes[n].inputs[k].node;
                let pin = doc.graph.nodes[n].inputs[k].pin.clone();
                source.map(|from| {
                    doc.edit(|g| g.disconnect(n, &pin));
                    GraphGesture::Wire { from, to: p }
                })
            }
            Hit::Node(n) => {
                doc.selected = Some(n);
                let (x, y) = doc.graph.nodes[n].position;
                Some(GraphGesture::MoveNode {
                    node: n,
                    grab: [p[0] - x, p[1] - y],
                })
            }
            Hit::Empty => {
                doc.selected = None;
                Some(GraphGesture::Pan {
                    last: [f32::from(at.x), f32::from(at.y)],
                })
            }
        };
        cx.notify();
    }

    fn anim_graph_drag(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let p = self.anim_graph_point(at);
        let doc = &mut self.anim.graph;
        match (doc.gesture, p) {
            (Some(GraphGesture::MoveNode { node, grab }), Some(p)) => {
                if let Some(n) = doc.graph.nodes.get_mut(node) {
                    n.position = (p[0] - grab[0], p[1] - grab[1]);
                    doc.dirty = true;
                }
            }
            (Some(GraphGesture::Pan { last }), _) => {
                let now = [f32::from(at.x), f32::from(at.y)];
                for a in 0..2 {
                    doc.view.pan[a] += now[a] - last[a];
                }
                doc.gesture = Some(GraphGesture::Pan { last: now });
            }
            (Some(GraphGesture::Wire { from, .. }), Some(p)) => {
                doc.gesture = Some(GraphGesture::Wire { from, to: p });
            }
            _ => return,
        }
        cx.notify();
    }

    fn anim_graph_release(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let p = self.anim_graph_point(at);
        let doc = &mut self.anim.graph;
        let Some(gesture) = doc.gesture.take() else {
            return;
        };
        if let (GraphGesture::Wire { from, .. }, Some(p)) = (gesture, p) {
            if let Hit::Input(n, k) = graph_canvas::hit(&doc.graph, p) {
                let pin = doc.graph.nodes[n].inputs[k].pin.clone();
                let mut ok = false;
                doc.edit(|g| ok = g.connect(from, n, &pin));
                if !ok {
                    self.anim.status = Some("That wire would make a loop".into());
                }
            }
        }
        cx.notify();
    }
}

fn paint_graph(
    window: &mut Window,
    b: Bounds<Pixels>,
    view: View,
    graph: &Graph,
    pending: Option<([f32; 2], [f32; 2])>,
) {
    let (gap, first) = view.dot_grid();
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    let (ox, oy) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let mut x = first[0];
    while x < w {
        let mut y = first[1];
        while y < h {
            paint::rect(
                window,
                ox + x - 0.75,
                oy + y - 0.75,
                1.5,
                1.5,
                tokens::border2(),
            );
            y += gap;
        }
        x += gap;
    }
    let stroke = |window: &mut Window, from: [f32; 2], to: [f32; 2], colour: Rgba| {
        let points: Vec<Point<Pixels>> = graph_canvas::wire(from, to)
            .into_iter()
            .map(|p| {
                let (sx, sy) = to_screen(view, b.origin, p);
                point(px(sx), px(sy))
            })
            .collect();
        paint::polyline(window, &points, colour, 2.0);
    };
    for node in &graph.nodes {
        for (k, input) in node.inputs.iter().enumerate() {
            let Some(from) = input
                .node
                .and_then(|n| graph.nodes.get(n))
                .and_then(graph_canvas::output_pin)
            else {
                continue;
            };
            stroke(
                window,
                from,
                graph_canvas::input_pin(node, k),
                tokens::text_label(),
            );
        }
    }
    if let Some((from, to)) = pending {
        stroke(window, from, to, tokens::check_on());
    }
}

fn paint_pins(window: &mut Window, b: Bounds<Pixels>, view: View, graph: &Graph) {
    let r = (4.5 * view.zoom).max(3.0);
    for node in &graph.nodes {
        if let Some(o) = graph_canvas::output_pin(node) {
            let (x, y) = to_screen(view, b.origin, o);
            paint::dot(window, x, y, r, tokens::check_on());
        }
        for (k, input) in node.inputs.iter().enumerate() {
            let (x, y) = to_screen(view, b.origin, graph_canvas::input_pin(node, k));
            let colour = if input.node.is_some() {
                tokens::text_full()
            } else {
                tokens::text_disabled()
            };
            paint::dot(window, x, y, r, colour);
        }
    }
}
