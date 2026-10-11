//! A state machine's transitions, each with its From and To states, its
//! condition, curve, fade and priority.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_animation::graph::{Transition, WaitFor};
use rbx_animation::Style;

use super::super::super::widgets::{field_row_owned, keyed_button, label, picker, wide_button};
use super::Shell;
use crate::shell::menu::{self, MenuId};
use crate::tokens;

impl Shell {
    pub(in crate::shell::animation) fn anim_transitions_list(
        &mut self,
        n: usize,
        list: &[Transition],
        node: &rbx_animation::graph::Node,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let states: Vec<String> = node.inputs.iter().map(|i| i.pin.clone()).collect();
        let mut out = v_flex().gap(px(6.)).child(label("Transitions".into()));
        for (t, tr) in list.iter().enumerate() {
            let from_label = tr
                .from
                .and_then(|f| states.get(f))
                .cloned()
                .unwrap_or_else(|| "(Any)".into());
            let to_label = states.get(tr.to).cloned().unwrap_or_default();
            let froms: Vec<menu::Item> = std::iter::once(None)
                .chain((0..states.len()).map(Some))
                .map(|f| {
                    let text = f.map_or("(Any)".to_owned(), |i| states[i].clone());
                    menu::item(text)
                        .checked(f == tr.from)
                        .on_click(move |s, cx| {
                            s.anim.graph.edit(|g| g.nodes[n].transitions[t].from = f);
                            cx.notify();
                        })
                })
                .collect();
            let tos: Vec<menu::Item> = (0..states.len())
                .map(|i| {
                    menu::item(states[i].clone())
                        .checked(i == tr.to)
                        .on_click(move |s, cx| {
                            s.anim.graph.edit(|g| g.nodes[n].transitions[t].to = i);
                            cx.notify();
                        })
                })
                .collect();
            let finished = tr.wait_for == WaitFor::Finished;
            let cubic = tr.curve == Style::CubicV2;
            let expr =
                self.anim_graph_input(&format!("tr-expr:{n}:{t}"), &tr.expression, window, cx);
            let duration = self.anim_graph_input(
                &format!("tr-dur:{n}:{t}"),
                &tr.duration.to_string(),
                window,
                cx,
            );
            let priority = self.anim_graph_input(
                &format!("tr-pri:{n}:{t}"),
                &tr.priority.to_string(),
                window,
                cx,
            );
            out = out.child(
                v_flex()
                    .gap(px(4.))
                    .p(px(6.))
                    .rounded(px(4.))
                    .bg(tokens::wash_faint())
                    .child(
                        h_flex()
                            .gap(px(4.))
                            .items_center()
                            .child(picker(
                                self,
                                MenuId::AnimTransitionFrom(t),
                                "graph-tr-from",
                                &from_label,
                                froms,
                                cx,
                            ))
                            .child(div().text_color(tokens::text_muted()).child("→"))
                            .child(picker(
                                self,
                                MenuId::AnimTransitionTo(t),
                                "graph-tr-to",
                                &to_label,
                                tos,
                                cx,
                            )),
                    )
                    .child(
                        h_flex()
                            .gap(px(4.))
                            .child(
                                keyed_button(
                                    ("graph-tr-wait", t).into(),
                                    if finished {
                                        "When finished"
                                    } else {
                                        "When true"
                                    },
                                    true,
                                )
                                .on_click(cx.listener(
                                    move |s, _, _, cx| {
                                        s.anim.graph.edit(|g| {
                                            let tr = &mut g.nodes[n].transitions[t];
                                            tr.wait_for = if tr.wait_for == WaitFor::Finished {
                                                WaitFor::Trigger
                                            } else {
                                                WaitFor::Finished
                                            };
                                        });
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                keyed_button(
                                    ("graph-tr-curve", t).into(),
                                    if cubic { "CubicV2" } else { "Linear" },
                                    true,
                                )
                                .on_click(cx.listener(
                                    move |s, _, _, cx| {
                                        s.anim.graph.edit(|g| {
                                            let tr = &mut g.nodes[n].transitions[t];
                                            tr.curve = if tr.curve == Style::CubicV2 {
                                                Style::Linear
                                            } else {
                                                Style::CubicV2
                                            };
                                        });
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                    .when(!finished, |this| this.child(field_row_owned("When", &expr)))
                    .child(field_row_owned("Fade s", &duration))
                    .child(field_row_owned("Priority", &priority)),
            );
        }
        out.child(
            wide_button("graph-add-transition", "Add transition", states.len() >= 2).on_click(
                cx.listener(move |s, _, _, cx| {
                    let count = s.anim.graph.graph.nodes[n].inputs.len();
                    if count >= 2 {
                        s.anim
                            .graph
                            .edit(|g| g.nodes[n].transitions.push(Transition::new(Some(0), 1)));
                    }
                    cx.notify();
                }),
            ),
        )
    }
}
