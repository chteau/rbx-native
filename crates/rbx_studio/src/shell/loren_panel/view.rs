//! The Loren dock's body: credit line, then the two boot columns, then the
//! cycles and problems.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use rbx_loren::graph::{Graph, Kind, Node, Severity};

use crate::shell::Shell;
use crate::tokens;

const EMPTY: &str = "This place has no Loren Services or Controllers. Start one from Home \u{203a} New \u{203a} Loren project, or run rbxnative loren init.";

impl Shell {
    pub(super) fn loren_body(&self, graph: &Graph, cx: &mut Context<Self>) -> AnyElement {
        let mut body = v_flex()
            .id("loren-body")
            .size_full()
            .overflow_y_scroll()
            .border_t_1()
            .border_color(tokens::border())
            .p(px(10.))
            .gap(px(10.))
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .text_color(tokens::text_strong())
            .child(header(cx));
        if graph.nodes.is_empty() {
            return body
                .child(div().text_color(tokens::text_muted()).child(EMPTY))
                .into_any_element();
        }
        body = body.child(
            h_flex()
                .w_full()
                .items_start()
                .gap(px(16.))
                .child(column(
                    "Server",
                    Kind::Service,
                    &graph.server_order,
                    graph,
                    cx,
                ))
                .child(column(
                    "Client",
                    Kind::Controller,
                    &graph.client_order,
                    graph,
                    cx,
                )),
        );
        if !graph.cycles.is_empty() {
            body = body
                .child(heading("Cycles"))
                .children(graph.cycles.iter().map(|cycle| {
                    div()
                        .text_color(tokens::warning())
                        .child(cycle.join(" \u{2192} "))
                }));
        }
        if !graph.problems.is_empty() {
            body = body
                .child(heading("Problems"))
                .children(graph.problems.iter().map(|problem| {
                    let colour = match problem.severity {
                        Severity::Error => tokens::text_error(),
                        Severity::Warning => tokens::warning(),
                    };
                    div().text_color(colour).child(problem.message.clone())
                }));
        }
        body.into_any_element()
    }
}

fn header(cx: &mut Context<Shell>) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap(px(8.))
        .flex_wrap()
        .child(
            div()
                .id("loren-credit")
                .cursor_pointer()
                .text_color(tokens::accent_line())
                .hover(|this| this.underline())
                .child(rbx_loren::CREDIT)
                .on_click(cx.listener(|_, _, _, cx| cx.open_url(rbx_loren::REPOSITORY_URL))),
        )
        .child(
            div()
                .text_color(tokens::text_muted())
                .child("Optional: ignore it and nothing changes."),
        )
}

fn heading(text: &'static str) -> impl IntoElement {
    div().text_color(tokens::text_label()).child(text)
}

fn column(
    title: &'static str,
    kind: Kind,
    order: &[String],
    graph: &Graph,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap(px(4.))
        .child(heading(title))
        .children(order.iter().enumerate().filter_map(|(index, name)| {
            let node = graph
                .nodes
                .iter()
                .find(|node| node.kind == kind && &node.name == name)?;
            Some(row(index, node, cx))
        }))
}

fn chip(text: String) -> Div {
    div()
        .px(px(6.))
        .rounded(tokens::radius())
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border())
        .text_size(tokens::text_xs())
        .text_color(tokens::text_label())
        .child(text)
}

fn row(index: usize, node: &Node, cx: &mut Context<Shell>) -> impl IntoElement {
    let kind = node.kind;
    let name = node.name.clone();
    let id = format!(
        "loren-{}-{}",
        if kind == Kind::Service { "s" } else { "c" },
        node.name
    );
    let mut line = h_flex()
        .id(SharedString::from(id))
        .w_full()
        .flex_wrap()
        .items_center()
        .gap(px(6.))
        .px(px(4.))
        .rounded(tokens::radius())
        .cursor_pointer()
        .hover(|this| this.bg(tokens::accent_soft()))
        .child(
            div()
                .text_color(tokens::text_muted())
                .child(format!("{}.", index + 1)),
        )
        .child(
            div()
                .text_color(tokens::text_full())
                .child(node.name.clone()),
        )
        .children(
            node.dependencies
                .iter()
                .map(|dependency| chip(format!("\u{2190} {dependency}"))),
        );
    if kind == Kind::Service && node.networked {
        line = line.child(chip("networked".to_owned()));
        let methods = node.client_methods.len();
        if methods > 0 {
            line = line.child(
                div()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text_muted())
                    .child(if methods == 1 {
                        "1 Client method".to_owned()
                    } else {
                        format!("{methods} Client methods")
                    }),
            );
        }
    }
    line.on_click(cx.listener(move |shell, event: &ClickEvent, window, cx| {
        shell.loren_row_clicked(kind, &name, event.click_count(), window, cx);
    }))
}
