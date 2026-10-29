//! The graph itself, back to front: the edges, the boxes, then each
//! resolved edge's line badge at its midpoint. A plain require is drawn in
//! the accent, a cycle in the error red, a client-to-server require in the
//! warning amber, and an unknown one dashed and grey into a ghost box.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_audit::{Edge, Node, RequireGraph, Target};

use crate::explorer;
use crate::tokens;

use super::layout::{self, Item, GHOST_HEIGHT, NODE_HEIGHT, NODE_WIDTH};
use super::RequireGraphWindow;

const BADGE: [f32; 2] = [52., 18.];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Plain,
    Unknown,
    Crossing,
    Cycle,
}

impl Kind {
    fn of(edge: &Edge) -> Kind {
        match edge.target {
            Target::Unknown(_) => Kind::Unknown,
            Target::Module(_) if edge.cycle => Kind::Cycle,
            Target::Module(_) if edge.crosses => Kind::Crossing,
            Target::Module(_) => Kind::Plain,
        }
    }

    fn colour(self) -> Rgba {
        match self {
            Kind::Plain => tokens::accent_line(),
            Kind::Unknown => tokens::text3(),
            Kind::Crossing => tokens::warning(),
            Kind::Cycle => tokens::text_error(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Plain => "require",
            Kind::Unknown => "unknown",
            Kind::Crossing => "client \u{2192} server",
            Kind::Cycle => "cycle",
        }
    }
}

/// The edge colours, for the toolbar.
pub(super) fn legend() -> impl IntoElement {
    h_flex()
        .gap(px(14.))
        .text_size(tokens::text_xs())
        .text_color(tokens::text3())
        .children(
            [Kind::Plain, Kind::Cycle, Kind::Crossing, Kind::Unknown].map(|kind| {
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .child(div().w(px(14.)).h(px(2.)).bg(kind.colour()))
                    .child(kind.label())
            }),
        )
}

type Stroke = ([[f32; 2]; 4], f32, Kind);

fn paint(strokes: &[Stroke], origin: Point<Pixels>, window: &mut Window) {
    let at = |p: [f32; 2]| point(origin.x + px(p[0]), origin.y + px(p[1]));
    for &([a, c1, c2, b], direction, kind) in strokes {
        let mut builder = PathBuilder::stroke(px(1.5));
        if kind == Kind::Unknown {
            builder = builder.dash_array(&[px(5.), px(4.)]);
        }
        builder.move_to(at(a));
        builder.cubic_bezier_to(at(b), at(c1), at(c2));
        if let Ok(path) = builder.build() {
            window.paint_path(path, kind.colour());
        }
        // Every curve arrives level, so the head only needs its side.
        let back = b[0] - 7. * direction;
        let mut head = PathBuilder::fill();
        head.add_polygon(&[at(b), at([back, b[1] - 4.]), at([back, b[1] + 4.])], true);
        if let Ok(path) = head.build() {
            window.paint_path(path, kind.colour());
        }
    }
}

impl RequireGraphWindow {
    pub(super) fn canvas(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(graph) = self.graph.clone() else {
            return centred("Graphing\u{2026}");
        };
        if graph.nodes.is_empty() {
            return centred("No ModuleScripts, and no script requires anything.");
        }
        let layout = &self.layout;
        let mut strokes: Vec<Stroke> = graph
            .edges
            .iter()
            .enumerate()
            .filter_map(|(i, edge)| {
                let (points, direction) = layout::curve(edge, i, layout)?;
                Some((points, direction, Kind::of(edge)))
            })
            .collect();
        // The problems are drawn last, over the plain requires.
        strokes.sort_by_key(|stroke| stroke.2);

        let pack = self.shell.read(cx).icon_pack();
        let cyclic: Vec<_> = graph.cycles.iter().flatten().copied().collect();
        let mut plane = div()
            .relative()
            .flex_none()
            .w(px(layout.size[0]))
            .h(px(layout.size[1]))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| paint(&strokes, bounds.origin, window),
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        for &(item, at) in &layout.items {
            plane = plane.child(match item {
                Item::Node(i) => {
                    let node = &graph.nodes[i];
                    self.node(i, node, at, pack, cyclic.contains(&node.script), cx)
                }
                Item::Ghost(i) => self.ghost(i, &graph.edges[i], at, cx),
            });
        }
        plane = plane.children(self.badges(&graph, cx));
        div()
            .id("require-graph-canvas")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_scroll()
            .child(plane)
            .into_any_element()
    }

    fn node(
        &self,
        index: usize,
        node: &Node,
        at: [f32; 2],
        pack: crate::class_icons::IconPack,
        cyclic: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let script = node.script;
        let border = if cyclic {
            tokens::text_error()
        } else {
            tokens::border2()
        };
        let parent = node
            .path
            .rsplit_once('.')
            .map_or("", |(parent, _)| parent)
            .to_string();
        let tag = if node.unused {
            Some("unused")
        } else if node.client {
            Some("client")
        } else {
            None
        };
        v_flex()
            .id(("require-node", index))
            .absolute()
            .left(px(at[0]))
            .top(px(at[1]))
            .w(px(NODE_WIDTH))
            .h(px(NODE_HEIGHT))
            .px(px(10.))
            .gap(px(2.))
            .justify_center()
            .rounded(tokens::radius())
            .bg(tokens::tile())
            .border_1()
            .border_color(border)
            .when(node.unused, |this| this.border_dashed())
            .cursor_pointer()
            .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.reveal(script, None, cx)))
            .child(
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .child(super::super::rows::class_icon(explorer::resolve_icon(
                        &node.class,
                        pack,
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(tokens::text_strong())
                            .child(node.name.clone()),
                    )
                    .children(tag.map(chip)),
            )
            .child(
                div()
                    .truncate()
                    .text_size(tokens::text_xs())
                    .line_height(tokens::line_xs())
                    .text_color(tokens::text3())
                    .child(parent),
            )
            .into_any_element()
    }

    fn ghost(&self, index: usize, edge: &Edge, at: [f32; 2], cx: &mut Context<Self>) -> AnyElement {
        let (from, line) = (edge.from, edge.line);
        let reason = match edge.target {
            Target::Unknown(reason) => reason,
            Target::Module(_) => "",
        };
        h_flex()
            .id(("require-ghost", index))
            .absolute()
            .left(px(at[0]))
            .top(px(at[1]))
            .w(px(NODE_WIDTH))
            .h(px(GHOST_HEIGHT))
            .px(px(10.))
            .gap(px(8.))
            .items_center()
            .rounded(tokens::radius())
            .border_1()
            .border_dashed()
            .border_color(tokens::border2())
            .text_size(tokens::text_xs())
            .cursor_pointer()
            .hover(|this| tokens::hover_fx(this).bg(tokens::hover_subtle()))
            .on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| this.reveal(from, Some(line), cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(tokens::font_mono())
                    .text_color(tokens::text2())
                    .child(edge.text.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(tokens::text3())
                    .child(format!("{reason} \u{b7} Ln {line}")),
            )
            .into_any_element()
    }

    /// A badge with the line of the call on each resolved edge, see
    /// `layout::badge`.
    fn badges(&self, graph: &RequireGraph, cx: &mut Context<Self>) -> Vec<AnyElement> {
        graph
            .edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| matches!(edge.target, Target::Module(_)))
            .filter_map(|(i, edge)| {
                let (points, direction) = layout::curve(edge, i, &self.layout)?;
                let mid = layout::badge(&points, direction);
                let (from, line) = (edge.from, edge.line);
                Some(
                    div()
                        .id(("require-edge", i))
                        .absolute()
                        .left(px(mid[0] - BADGE[0] / 2.))
                        .top(px(mid[1] - BADGE[1] / 2.))
                        .w(px(BADGE[0]))
                        .h(px(BADGE[1]))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(tokens::radius())
                        .bg(tokens::dock())
                        .border_1()
                        .border_color(Kind::of(edge).colour())
                        .font_family(tokens::font_mono())
                        .text_size(tokens::text_xxs())
                        .text_color(tokens::text2())
                        .cursor_pointer()
                        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.reveal(from, Some(line), cx)
                        }))
                        .child(format!("Ln {line}"))
                        .into_any_element(),
                )
            })
            .collect()
    }
}

fn chip(label: &'static str) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .rounded(tokens::radius())
        .bg(tokens::hover())
        .text_size(tokens::text_xxs())
        .line_height(tokens::line_xs())
        .text_color(tokens::text2())
        .child(label)
}

fn centred(text: &'static str) -> AnyElement {
    div()
        .flex_1()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(tokens::text3())
        .child(text)
        .into_any_element()
}
