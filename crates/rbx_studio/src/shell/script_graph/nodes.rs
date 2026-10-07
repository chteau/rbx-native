//! One node as an element: header, then each row's input label and chip on
//! the left and output label on the right. Everything is placed from
//! `graph::layout`'s numbers, scaled by the zoom, rather than left to flow,
//! so the pins painted over it and the clicks tested against it agree with
//! what is drawn.

use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::style;
use crate::script_editor::graph::catalog::{self, PinType};
use crate::script_editor::graph::layout::{self, Side};
use crate::script_editor::graph::{Graph, Node};
use crate::tokens;
use crate::ui_canvas::View;

/// How a node is outlined.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mark {
    Plain,
    Selected,
    Problem,
}

pub(super) fn node(graph: &Graph, node: &Node, view: View, mark: Mark) -> AnyElement {
    let rect = layout::rect(graph, node);
    let z = view.zoom;
    let at = view.to_view([rect.x, rect.y]);
    let s = |v: f32| px(v * z);
    let border = match mark {
        Mark::Selected => tokens::check_on(),
        Mark::Problem => tokens::text_error(),
        Mark::Plain => style::node_border(),
    };
    let Some(kind) = catalog::kind(&node.kind) else {
        return div()
            .absolute()
            .left(px(at[0]))
            .top(px(at[1]))
            .w(s(rect.w))
            .h(s(rect.h))
            .bg(style::node_body())
            .border_1()
            .border_color(tokens::text_error())
            .rounded(s(6.0))
            .p(s(8.0))
            .text_size(s(11.0))
            .text_color(tokens::text_error())
            .child(SharedString::from(format!("Unknown node {}", node.kind)))
            .into_any_element();
    };
    let (fill, icon) = style::header(kind.category);

    let header = div()
        .absolute()
        .top_0()
        .left_0()
        .w_full()
        .h(s(layout::HEADER))
        .bg(fill)
        .border_b_1()
        .border_color(style::node_border())
        .flex()
        .items_center()
        .gap(s(6.0))
        .px(s(10.0))
        .child(
            Icon::new(style::icon(kind.category))
                .size(s(12.0))
                .text_color(icon),
        )
        .child(
            div()
                .text_size(s(12.0))
                .font_weight(tokens::WEIGHT_SEMIBOLD)
                .text_color(tokens::text_strong())
                .whitespace_nowrap()
                .child(kind.title),
        );

    let label = |text: String, x: f32, row: usize, right: bool| {
        div()
            .absolute()
            .top(s(layout::row_centre(row) - 8.0))
            .h(s(16.0))
            .flex()
            .items_center()
            .map(|this| match right {
                true => this.right(s(x)),
                false => this.left(s(x)),
            })
            .text_size(s(12.0))
            .text_color(tokens::text_muted())
            .whitespace_nowrap()
            .child(text)
    };
    let mut rows: Vec<AnyElement> = Vec::new();
    let pins = graph.pins(node.id);
    for (row, pin) in pins.inputs.iter().enumerate() {
        let name = layout::label(graph, node, pin, Side::Input);
        if !name.is_empty() {
            rows.push(label(name, 14.0, row, false).into_any_element());
        }
        if let (Some(text), Some(chip)) = (
            layout::chip(graph, node, pin),
            layout::chip_rect(graph, node, pin.name),
        ) {
            rows.push(
                div()
                    .absolute()
                    .left(s(chip.x - node.x))
                    .top(s(chip.y - node.y))
                    .w(s(chip.w))
                    .h(s(chip.h))
                    .flex()
                    .items_center()
                    .px(s(6.0))
                    .bg(style::chip())
                    .border_1()
                    .border_color(style::node_border())
                    .rounded(s(3.0))
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(s(11.0))
                    .text_color(literal_colour(pin.ty))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(layout::chip_text(&text))
                    .into_any_element(),
            );
        }
    }
    for (row, pin) in pins.outputs.iter().enumerate() {
        let name = layout::label(graph, node, pin, Side::Output);
        if !name.is_empty() {
            rows.push(label(name, 14.0, row, true).into_any_element());
        }
    }

    div()
        .absolute()
        .left(px(at[0]))
        .top(px(at[1]))
        .w(s(rect.w))
        .h(s(rect.h))
        .bg(style::node_body())
        .border_1()
        .border_color(border)
        .rounded(s(6.0))
        .overflow_hidden()
        .shadow_md()
        .child(header)
        .children(rows)
        .into_any_element()
}

fn literal_colour(ty: PinType) -> Rgba {
    match ty {
        PinType::String => style::pin(PinType::String),
        PinType::Number => style::pin(PinType::Number),
        _ => tokens::text_strong(),
    }
}
