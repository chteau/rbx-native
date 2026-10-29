//! The graph's side panel: saving and loading, adding nodes, the selected
//! node's properties and pins (a state machine's anim_transitions too), and the
//! graph's parameters, which are live while the graph plays.
//!
//! Each value is one text field. A number or `true`/`false` is that value,
//! `$name` binds the graph parameter `name`, `=…` is an expression over the
//! parameters, and anything else is text (see `graph_canvas::parse_prop`).

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_animation::graph::{NodeKind, ParamKind};

use super::super::panel::WIDTH;
use super::super::widgets::{
    field_row_owned, keyed_button, label, picker, primary_button, section, wide_button,
};
use super::Shell;
use crate::anim_editor::graph_canvas::format_named_prop;
use crate::anim_editor::place;
use crate::shell::menu::{self, MenuId};
use crate::tokens;

mod fields;
mod transitions;

impl Shell {
    pub(in crate::shell::animation) fn anim_graph_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = self.anim.graph.graph.name.clone();
        let name_input = self.anim_graph_input("graph-name", &name, window, cx);
        let rig = self.anim.bound.as_ref().map(|b| b.model);
        let saved = place::all_of_class(&self.dom, "AnimationGraphDefinition", rig);
        let loads: Vec<menu::Item> = if saved.is_empty() {
            vec![menu::item("No graph in this place").disabled()]
        } else {
            saved
                .into_iter()
                .map(|(r, n)| menu::item(n).on_click(move |shell, cx| shell.anim_load_graph(r, cx)))
                .collect()
        };
        let kinds: Vec<menu::Item> = NodeKind::INSERTABLE
            .into_iter()
            .map(|k| {
                menu::item(k.label()).on_click(move |shell, cx| shell.anim_add_graph_node(k, cx))
            })
            .collect();
        let playing = self.anim.graph.playing;
        let header = section("Graph")
            .child(field_row_owned("Name", &name_input))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        wide_button("graph-new", "New", true)
                            .on_click(cx.listener(|s, _, _, cx| s.anim_new_graph(cx))),
                    )
                    .child(picker(
                        self,
                        MenuId::AnimGraphLoad,
                        "graph-load",
                        "Load",
                        loads,
                        cx,
                    ))
                    .child(
                        primary_button("graph-save", "Save")
                            .on_click(cx.listener(|s, _, _, cx| s.anim_save_graph(cx))),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(picker(
                        self,
                        MenuId::AnimGraphAdd,
                        "graph-add",
                        "Add node",
                        kinds,
                        cx,
                    ))
                    .child(
                        wide_button(
                            "graph-play",
                            if playing {
                                "Stop preview"
                            } else {
                                "Preview on rig"
                            },
                            true,
                        )
                        .on_click(cx.listener(|s, _, _, cx| s.toggle_animation_playback(cx))),
                    ),
            );
        let node = self.anim_node_section(window, cx);
        let params = self.anim_params_section(window, cx);
        v_flex()
            .id("graph-panel")
            .flex_none()
            .w(px(WIDTH))
            .h_full()
            .overflow_y_scroll()
            .p(px(8.))
            .gap(px(10.))
            .bg(tokens::dock())
            .border_l(px(1.))
            .border_color(tokens::border())
            .child(header)
            .child(node)
            .child(params)
            .into_any_element()
    }

    fn anim_node_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(n) = self
            .anim
            .graph
            .selected
            .filter(|&n| n < self.anim.graph.graph.nodes.len())
        else {
            return section("Node")
                .child(label("Click a node to edit it".into()))
                .into_any_element();
        };
        let node = self.anim.graph.graph.nodes[n].clone();
        let mut out = section("Node").child(label(format!("{} · {}", node.kind.label(), node.id)));
        for (prop, value) in &node.props {
            let input = self.anim_graph_input(
                &format!("prop:{n}:{prop}"),
                &format_named_prop(prop, value),
                window,
                cx,
            );
            out = out.child(field_row_owned(prop, &input));
        }
        for (k, pin) in node.inputs.iter().enumerate() {
            let source = pin.node.map_or("unwired".to_owned(), |s| {
                self.anim.graph.graph.nodes[s].name.clone()
            });
            out = out.child(label(format!("{} ← {source}", pin.pin)));
            for (prop, value) in &pin.props {
                let input = self.anim_graph_input(
                    &format!("pin:{n}:{k}:{prop}"),
                    &format_named_prop(prop, value),
                    window,
                    cx,
                );
                out = out.child(field_row_owned(prop, &input));
            }
        }
        let grows = node.kind.grows_pins();
        out = out.child(
            h_flex()
                .gap(px(6.))
                .when(grows, |this| {
                    this.child(wide_button("graph-add-pin", "Add input", true).on_click(
                        cx.listener(move |s, _, _, cx| {
                            s.anim.graph.edit(|g| {
                                g.nodes[n].add_pin();
                            });
                            cx.notify();
                        }),
                    ))
                })
                .child(
                    wide_button(
                        "graph-delete-node",
                        "Delete node",
                        node.kind != NodeKind::GraphOutput,
                    )
                    .on_click(cx.listener(|s, _, _, cx| s.anim_delete_graph_node(cx))),
                ),
        );
        if node.kind == NodeKind::StateMachine {
            out = out.child(self.anim_transitions_list(n, &node.transitions, &node, window, cx));
        }
        out.into_any_element()
    }

    fn anim_params_section(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let params = self.anim.graph.graph.params.clone();
        let mut out = section("Parameters");
        for p in &params {
            let live = self
                .anim
                .graph
                .player
                .as_ref()
                .filter(|_| self.anim.graph.playing)
                .and_then(|pl| pl.get(&p.name).cloned());
            let value = live.unwrap_or_else(|| p.default.clone());
            let input =
                self.anim_graph_input(&format!("param:{}", p.name), &value.text(), window, cx);
            let name = p.name.clone();
            out = out.child(
                h_flex()
                    .gap(px(4.))
                    .items_center()
                    .child(div().flex_1().child(field_row_owned(&p.name, &input)))
                    .when(p.kind == ParamKind::Trigger, |this| {
                        this.child(
                            keyed_button(
                                SharedString::from(format!("graph-fire-{name}")).into(),
                                "Fire",
                                true,
                            )
                            .on_click(cx.listener(
                                move |s, _, _, cx| {
                                    if let Some(player) = &mut s.anim.graph.player {
                                        player.fire(&name);
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                    }),
            );
        }
        // Seeded with its own text: clicking a kind button blurs the field,
        // and an empty seed would wipe the name before the click lands.
        let typed = self
            .anim
            .graph
            .inputs
            .get("new-param")
            .map(|i| i.read(cx).value().to_string())
            .unwrap_or_default();
        let new_name = self.anim_graph_input("new-param", &typed, window, cx);
        out.child(field_row_owned("New", &new_name)).child(
            h_flex()
                .gap(px(4.))
                .child(
                    wide_button("graph-param-number", "Number", true).on_click(
                        cx.listener(|s, _, _, cx| s.anim_add_param(ParamKind::Number, cx)),
                    ),
                )
                .child(
                    wide_button("graph-param-bool", "Bool", true)
                        .on_click(cx.listener(|s, _, _, cx| s.anim_add_param(ParamKind::Bool, cx))),
                )
                .child(
                    wide_button("graph-param-trigger", "Trigger", true).on_click(
                        cx.listener(|s, _, _, cx| s.anim_add_param(ParamKind::Trigger, cx)),
                    ),
                )
                .child(
                    wide_button("graph-param-text", "Text", true)
                        .on_click(cx.listener(|s, _, _, cx| s.anim_add_param(ParamKind::Text, cx))),
                ),
        )
    }
}
