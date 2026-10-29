//! The column beside the graph: what the graph found wrong, worst first,
//! each row opening the script (at the require, where there is one).

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable};
use gpui_kit::*;
use rbx_audit::{RequireGraph, Target};
use rbx_dom::Ref;

use crate::tokens;

use super::RequireGraphWindow;

const WIDTH: f32 = 320.;

/// One row: where a click goes, and what it says.
struct Row {
    script: Ref,
    line: Option<usize>,
    title: String,
    detail: String,
}

struct Section {
    title: &'static str,
    icon: IconName,
    colour: Rgba,
    rows: Vec<Row>,
}

fn sections(graph: &RequireGraph) -> Vec<Section> {
    let name = |r: Ref| graph.node(r).map_or("?", |n| n.name.as_str()).to_string();
    let cycles = graph
        .cycles
        .iter()
        .filter_map(|cycle| {
            let first = *cycle.first()?;
            let next = cycle[1 % cycle.len()];
            let edge = graph
                .edges
                .iter()
                .find(|e| e.from == first && e.target == Target::Module(next))?;
            let mut names: Vec<String> = cycle.iter().map(|r| name(*r)).collect();
            names.push(name(first));
            Some(Row {
                script: first,
                line: Some(edge.line),
                title: names.join(" \u{2192} "),
                detail: format!("{} module{}", cycle.len(), plural(cycle.len())),
            })
        })
        .collect();
    let crossing = graph
        .edges
        .iter()
        .filter(|e| e.crosses)
        .filter_map(|e| {
            let Target::Module(to) = e.target else {
                return None;
            };
            Some(Row {
                script: e.from,
                line: Some(e.line),
                title: format!("{} \u{2192} {}", name(e.from), name(to)),
                detail: format!("Ln {}, the client never receives {}", e.line, name(to)),
            })
        })
        .collect();
    let unknown = graph
        .edges
        .iter()
        .filter_map(|e| match e.target {
            Target::Unknown(reason) => Some(Row {
                script: e.from,
                line: Some(e.line),
                title: format!("{} \u{b7} Ln {}", name(e.from), e.line),
                detail: format!("require({}): {reason}", e.text),
            }),
            Target::Module(_) => None,
        })
        .collect();
    let unused = graph
        .nodes
        .iter()
        .filter(|n| n.unused)
        .map(|n| Row {
            script: n.script,
            line: None,
            title: n.name.clone(),
            detail: n.path.clone(),
        })
        .collect();
    vec![
        Section {
            title: "CYCLES",
            icon: IconName::CircleX,
            colour: tokens::text_error(),
            rows: cycles,
        },
        Section {
            title: "CLIENT REQUIRES SERVER",
            icon: IconName::TriangleAlert,
            colour: tokens::warning(),
            rows: crossing,
        },
        Section {
            title: "UNKNOWN REQUIRES",
            icon: IconName::Info,
            colour: tokens::text3(),
            rows: unknown,
        },
        Section {
            title: "UNUSED MODULES",
            icon: IconName::Info,
            colour: tokens::text3(),
            rows: unused,
        },
    ]
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

impl RequireGraphWindow {
    pub(super) fn issues(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = v_flex().px(px(12.)).pb(px(12.));
        if let Some(graph) = &self.graph {
            let sections: Vec<Section> = sections(graph)
                .into_iter()
                .filter(|s| !s.rows.is_empty())
                .collect();
            if sections.is_empty() {
                list = list.child(
                    h_flex()
                        .pt(px(12.))
                        .gap(px(8.))
                        .text_color(tokens::text2())
                        .child(Icon::new(IconName::Check).small())
                        .child("No cycles, crossings, unknown requires or unused modules."),
                );
            }
            let mut index = 0_usize;
            for section in sections {
                list = list.child(
                    div()
                        .pt(px(12.))
                        .pb(px(4.))
                        .text_size(tokens::text_xxs())
                        .font_weight(tokens::WEIGHT_BOLD)
                        .text_color(tokens::text3())
                        .child(format!("{} {}", section.title, section.rows.len())),
                );
                for row in section.rows {
                    let (script, line) = (row.script, row.line);
                    list =
                        list.child(
                            h_flex()
                                .id(("require-issue", index))
                                .mx(px(-6.))
                                .px(px(6.))
                                .py(px(5.))
                                .gap(px(8.))
                                .items_start()
                                .rounded(tokens::radius())
                                .cursor_pointer()
                                .hover(|this| tokens::hover_fx(this).bg(tokens::hover_subtle()))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.reveal(script, line, cx)
                                }))
                                .child(div().pt(px(2.)).child(
                                    Icon::new(section.icon).small().text_color(section.colour),
                                ))
                                .child(
                                    v_flex()
                                        .min_w_0()
                                        .child(div().text_color(tokens::text()).child(row.title))
                                        .child(
                                            div()
                                                .text_size(tokens::text_xs())
                                                .line_height(tokens::line_xs())
                                                .text_color(tokens::text3())
                                                .child(row.detail),
                                        ),
                                ),
                        );
                    index += 1;
                }
            }
        }
        v_flex()
            .w(px(WIDTH))
            .flex_none()
            .h_full()
            .border_l_1()
            .border_color(tokens::border())
            .bg(tokens::chrome())
            .child(
                div()
                    .id("require-graph-issues")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
    }
}
