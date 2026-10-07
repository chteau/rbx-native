//! The canvas element, back to front: dot grid, group frames, wires, nodes,
//! pins, then the overlays (marquee, literal field, add menu, minimap, zoom
//! readout). Pins are painted over the nodes because they straddle a
//! node's edge.

use std::collections::BTreeSet;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

mod overlays;

use super::nodes::{self, Mark};
use super::{problems, style, Gesture, GraphEditor};
use crate::script_editor::graph::catalog::{self, PinType};
use crate::script_editor::graph::layout::{self, Rect, Side};
use crate::script_editor::graph::{End, Graph};
use crate::script_editor::source;
use crate::tokens;
use crate::ui_canvas::View;
use overlays::{banner, minimap};

use super::super::Shell;

const MINIMAP: [f32; 2] = [168.0, 110.0];
/// Room left round the graph when it is fitted to the panel.
const FIT_MARGIN: f32 = 48.0;

/// One stroke or fill of the wire layer, in panel pixels.
enum Stroke {
    Curve([f32; 2], [f32; 2], Rgba, f32, bool),
    Rect(Rect, Rgba, Option<Rgba>, bool),
}

impl Shell {
    pub(super) fn graph_canvas(
        &mut self,
        reference: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let code = source::read(&self.dom, reference).unwrap_or_default();
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return div().into_any_element();
        };
        fit(editor);
        let view = editor.view;
        let graph = editor.graph.clone();
        let broken: BTreeSet<_> = problems(&graph).iter().map(|p| p.node).collect();
        let marks = |id| match (editor.selection.contains(&id), broken.contains(&id)) {
            (true, _) => Mark::Selected,
            (false, true) => Mark::Problem,
            _ => Mark::Plain,
        };
        let node_elements: Vec<AnyElement> = graph
            .nodes
            .iter()
            .map(|node| nodes::node(&graph, node, view, marks(node.id)))
            .collect();
        let picked = editor.group;
        let group_titles: Vec<AnyElement> = graph
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let band = layout::group_title(group);
                let at = view.to_view([band.x, band.y]);
                let z = view.zoom;
                div()
                    .absolute()
                    .left(px(at[0]))
                    .top(px(at[1]))
                    .h(px(band.h * z))
                    .flex()
                    .items_center()
                    .px(px(6.0 * z))
                    .bg(style::canvas())
                    .text_size(px(12.0 * z))
                    .text_color(match picked == Some(index) {
                        true => tokens::check_on(),
                        false => tokens::text_muted(),
                    })
                    .whitespace_nowrap()
                    .child(group.title.clone())
                    .into_any_element()
            })
            .collect();
        let strokes = strokes(editor, &graph);
        let pins = pins(&graph, view);
        let bounds = editor.bounds.clone();
        let focus = editor.focus.clone();
        let zoom = view.zoom;
        let empty = graph.nodes.is_empty();
        let banner = banner(editor, &graph, &code, reference, cx);
        let minimap = (!empty).then(|| minimap(editor, &graph, reference, cx));
        let menu = self.add_menu_element(reference, cx);
        let literal = self.literal_element(reference, cx);
        let _ = window;

        div()
            .id(reference_key(reference))
            .track_focus(&focus)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(style::canvas())
            .map(|this| self.graph_input(this, reference, cx))
            .child(
                canvas(
                    move |laid_out, _, _| bounds.set(laid_out),
                    move |laid_out, _, window, _| paint_dots(view, laid_out, window),
                )
                .absolute()
                .size_full(),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |laid_out, _, window, _| paint(&strokes, laid_out.origin, window),
                )
                .absolute()
                .size_full(),
            )
            .children(group_titles)
            .children(node_elements)
            .child(
                canvas(
                    |_, _, _| {},
                    move |laid_out, _, window, _| paint_pins(&pins, laid_out.origin, window),
                )
                .absolute()
                .size_full(),
            )
            .when(empty, |this| this.child(empty_hint()))
            .children(banner)
            .children(literal)
            .children(minimap)
            .child(
                div()
                    .absolute()
                    .left(px(12.0))
                    .bottom(px(10.0))
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text_muted())
                    .child(format!("{:.0}%", zoom * 100.0)),
            )
            .children(menu)
            .into_any_element()
    }
}

/// An element id for one tab's canvas.
pub(super) fn reference_key(reference: Ref) -> SharedString {
    SharedString::from(format!("script-graph-{reference:?}"))
}

/// Frames the whole graph the first time it is shown, and again until the
/// user pans or zooms — never past 100%, where text is drawn at its size.
fn fit(editor: &mut GraphEditor) {
    let panel = editor.panel_size();
    if !editor.fitted || panel[0] < 2.0 {
        return;
    }
    let Some(extent) = layout::extent(&editor.graph) else {
        return;
    };
    let room = [
        (panel[0] - FIT_MARGIN * 2.0).max(1.0) / extent.w.max(1.0),
        (panel[1] - FIT_MARGIN * 2.0).max(1.0) / extent.h.max(1.0),
    ];
    let zoom = room[0].min(room[1]).clamp(0.25, 1.0);
    editor.view = View {
        zoom,
        pan: [
            panel[0] * 0.5 - (extent.x + extent.w * 0.5) * zoom,
            panel[1] * 0.5 - (extent.y + extent.h * 0.5) * zoom,
        ],
    };
}

pub(super) fn fit_now(editor: &mut GraphEditor) {
    editor.fitted = true;
    fit(editor);
}

fn strokes(editor: &GraphEditor, graph: &Graph) -> Vec<Stroke> {
    let view = editor.view;
    let mut out = Vec::new();
    for (index, group) in graph.groups.iter().enumerate() {
        let a = view.to_view([group.x, group.y]);
        let rect = Rect {
            x: a[0],
            y: a[1],
            w: group.w * view.zoom,
            h: group.h * view.zoom,
        };
        let outline = match editor.group == Some(index) {
            true => tokens::check_on(),
            false => style::group_border(),
        };
        out.push(Stroke::Rect(rect, outline, Some(style::group_fill()), true));
    }
    for wire in &graph.wires {
        let (Some(a), Some(b)) = (
            layout::pin(graph, &wire.from, Side::Output),
            layout::pin(graph, &wire.to, Side::Input),
        ) else {
            continue;
        };
        let ty = super::wire_type(graph, &wire.from, Side::Output).unwrap_or(PinType::Any);
        out.push(Stroke::Curve(
            view.to_view(a),
            view.to_view(b),
            style::pin(ty),
            if ty == PinType::Exec { 2.0 } else { 1.6 },
            false,
        ));
    }
    // The wire a dropped pin left waiting on the add menu, drawn to it.
    if let Some((end, side, at)) = editor.menu.as_ref().and_then(|menu| menu.waiting()) {
        if let Some(pin) = layout::pin(graph, end, side) {
            let ty = super::wire_type(graph, end, side).unwrap_or(PinType::Any);
            let (a, b) = match side {
                Side::Output => (view.to_view(pin), at),
                Side::Input => (at, view.to_view(pin)),
            };
            out.push(Stroke::Curve(a, b, style::pin(ty), 1.6, true));
        }
    }
    match &editor.gesture {
        Some(Gesture::Wire { end, side, to }) => {
            if let Some(at) = layout::pin(graph, end, *side) {
                let ty = super::wire_type(graph, end, *side).unwrap_or(PinType::Any);
                let (a, b) = match side {
                    Side::Output => (view.to_view(at), view.to_view(*to)),
                    Side::Input => (view.to_view(*to), view.to_view(at)),
                };
                out.push(Stroke::Curve(a, b, style::pin(ty), 1.6, true));
            }
        }
        Some(Gesture::Marquee { from, to, .. }) => {
            let (a, b) = (view.to_view(*from), view.to_view(*to));
            let accent = tokens::check_on();
            out.push(Stroke::Rect(
                Rect::spanning(a, b),
                accent,
                Some(accent.opacity(0.12)),
                false,
            ));
        }
        _ => {}
    }
    out
}

/// Each pin to paint: centre in panel pixels, type, and whether a wire
/// touches it.
fn pins(graph: &Graph, view: View) -> Vec<([f32; 2], PinType, bool, f32)> {
    let mut out = Vec::new();
    for node in &graph.nodes {
        let Some(kind) = catalog::kind(&node.kind) else {
            continue;
        };
        for (side, pins) in [(Side::Input, kind.inputs), (Side::Output, kind.outputs)] {
            for pin in pins {
                let end = End::new(node.id, pin.name);
                let Some(at) = layout::pin(graph, &end, side) else {
                    continue;
                };
                let wired = match side {
                    Side::Input => graph.wire_into(&end).is_some(),
                    Side::Output => graph.wires_from(&end).next().is_some(),
                };
                out.push((view.to_view(at), pin.ty, wired, view.zoom));
            }
        }
    }
    out
}

fn point_at(origin: Point<Pixels>, p: [f32; 2]) -> Point<Pixels> {
    point(origin.x + px(p[0]), origin.y + px(p[1]))
}

fn paint(strokes: &[Stroke], origin: Point<Pixels>, window: &mut Window) {
    let at = |p: [f32; 2]| point_at(origin, p);
    for stroke in strokes {
        match stroke {
            Stroke::Curve(a, b, colour, width, dashed) => {
                let reach = ((b[0] - a[0]).abs() * 0.5).max(40.0);
                let mut builder = PathBuilder::stroke(px(*width));
                if *dashed {
                    builder = builder.dash_array(&[px(5.0), px(4.0)]);
                }
                builder.move_to(at(*a));
                builder.cubic_bezier_to(at(*b), at([a[0] + reach, a[1]]), at([b[0] - reach, b[1]]));
                if let Ok(path) = builder.build() {
                    window.paint_path(path, *colour);
                }
            }
            Stroke::Rect(rect, outline, fill, dashed) => {
                let corners = [
                    at([rect.x, rect.y]),
                    at([rect.x + rect.w, rect.y]),
                    at([rect.x + rect.w, rect.y + rect.h]),
                    at([rect.x, rect.y + rect.h]),
                ];
                if let Some(fill) = fill {
                    let mut builder = PathBuilder::fill();
                    builder.add_polygon(&corners, true);
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, *fill);
                    }
                }
                let mut builder = PathBuilder::stroke(px(1.0));
                if *dashed {
                    builder = builder.dash_array(&[px(4.0), px(3.0)]);
                }
                builder.add_polygon(&corners, true);
                if let Ok(path) = builder.build() {
                    window.paint_path(path, *outline);
                }
            }
        }
    }
}

fn paint_pins(pins: &[([f32; 2], PinType, bool, f32)], origin: Point<Pixels>, window: &mut Window) {
    for &(centre, ty, wired, zoom) in pins {
        let colour = style::pin(ty);
        let z = zoom.clamp(0.5, 1.5);
        let at = |dx: f32, dy: f32| point_at(origin, [centre[0] + dx * z, centre[1] + dy * z]);
        let shape: Vec<Point<Pixels>> = match ty {
            PinType::Exec => vec![
                at(-4.0, -5.0),
                at(1.0, -5.0),
                at(5.0, 0.0),
                at(1.0, 5.0),
                at(-4.0, 5.0),
            ],
            _ => (0..16)
                .map(|i| {
                    let angle = i as f32 / 16.0 * std::f32::consts::TAU;
                    at(4.5 * angle.cos(), 4.5 * angle.sin())
                })
                .collect(),
        };
        let mut backing = PathBuilder::fill();
        backing.add_polygon(&shape, true);
        if let Ok(path) = backing.build() {
            window.paint_path(path, if wired { colour } else { style::node_body() });
        }
        if !wired {
            let mut ring = PathBuilder::stroke(px(1.5));
            ring.add_polygon(&shape, true);
            if let Ok(path) = ring.build() {
                window.paint_path(path, colour);
            }
        }
    }
}

/// The dot grid, as the UI Editor's canvas draws it.
fn paint_dots(view: View, bounds: Bounds<Pixels>, window: &mut Window) {
    const HALF: f32 = 0.75;
    let (gap, first) = view.dot_grid();
    let [width, height] = [f32::from(bounds.size.width), f32::from(bounds.size.height)];
    let at = |x: f32, y: f32| point(bounds.origin.x + px(x), bounds.origin.y + px(y));
    let mut path: Option<Path<Pixels>> = None;
    let mut x = first[0];
    while x < width {
        let mut y = first[1];
        while y < height {
            let corner = at(x - HALF, y - HALF);
            let path = path.get_or_insert_with(|| Path::new(corner));
            path.move_to(corner);
            path.line_to(at(x + HALF, y - HALF));
            path.line_to(at(x + HALF, y + HALF));
            path.line_to(at(x - HALF, y + HALF));
            y += gap;
        }
        x += gap;
    }
    if let Some(path) = path {
        window.paint_path(path, tokens::border2());
    }
}

fn empty_hint() -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .text_size(tokens::text_sm())
        .text_color(tokens::text_muted())
        .child("Shift+A or double-click to add a node")
        .into_any_element()
}
