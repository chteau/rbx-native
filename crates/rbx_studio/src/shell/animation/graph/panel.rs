//! The graph's side panel: saving and loading, adding nodes, the selected
//! node's properties and pins (a state machine's anim_transitions too), and the
//! graph's parameters, which are live while the graph plays.
//!
//! Each value is one text field. A number or `true`/`false` is that value,
//! `$name` binds the graph parameter `name`, `=…` is an expression over the
//! parameters, and anything else is text (see `graph_canvas::parse_prop`).

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_animation::graph::{NodeKind, Param, ParamKind, Transition, Value, WaitFor};
use rbx_animation::Style;

use super::super::panel::{
    field_row_owned, keyed_button, label, picker, primary_button, section, wide_button, WIDTH,
};
use super::Shell;
use crate::anim_editor::graph_canvas::{format_prop, parse_prop};
use crate::anim_editor::place;
use crate::shell::menu::{self, MenuId};
use crate::tokens;

impl Shell {
    /// The field editing `key`, made on first use and kept in step with
    /// `seed` while it is not being typed in.
    fn anim_graph_input(
        &mut self,
        key: &str,
        seed: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.anim.graph.inputs.get(key) {
            let input = input.clone();
            if !input.focus_handle(cx).is_focused(window) && input.read(cx).value().as_ref() != seed
            {
                input.update(cx, |s, cx| s.set_value(seed.to_owned(), window, cx));
            }
            return input;
        }
        let input = cx.new(|cx| InputState::new(window, cx).default_value(seed.to_owned()));
        let owned = key.to_owned();
        let subscription = cx.subscribe(&input, move |shell, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                let text = input.read(cx).value().to_string();
                shell.anim_commit_graph_input(&owned, &text, cx);
            }
        });
        self.anim.graph.subscriptions.push(subscription);
        self.anim.graph.inputs.insert(key.to_owned(), input.clone());
        input
    }

    fn anim_commit_graph_input(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        let parts: Vec<&str> = key.split(':').collect();
        let index = |i: usize| parts.get(i).and_then(|p| p.parse::<usize>().ok());
        let doc = &mut self.anim.graph;
        match parts.first().copied() {
            Some("graph-name") if !text.trim().is_empty() => {
                let name = text.trim().to_owned();
                doc.edit(|g| g.name = name);
            }
            Some("prop") => {
                if let (Some(n), Some(name)) = (index(1), parts.get(2)) {
                    let (name, prop) = ((*name).to_owned(), parse_prop(text));
                    doc.edit(|g| {
                        drop(g.nodes.get_mut(n).map(|node| node.props.insert(name, prop)))
                    });
                }
            }
            Some("pin") => {
                if let (Some(n), Some(k), Some(name)) = (index(1), index(2), parts.get(3)) {
                    let (name, prop) = ((*name).to_owned(), parse_prop(text));
                    doc.edit(|g| {
                        if let Some(input) =
                            g.nodes.get_mut(n).and_then(|node| node.inputs.get_mut(k))
                        {
                            input.props.insert(name, prop);
                        }
                    });
                }
            }
            Some(field @ ("tr-expr" | "tr-dur" | "tr-pri")) => {
                if let (Some(n), Some(t)) = (index(1), index(2)) {
                    let text = text.trim().to_owned();
                    doc.edit(|g| {
                        let Some(tr) = g
                            .nodes
                            .get_mut(n)
                            .and_then(|node| node.transitions.get_mut(t))
                        else {
                            return;
                        };
                        match field {
                            "tr-expr" => tr.expression = text,
                            "tr-dur" => tr.duration = text.parse().unwrap_or(tr.duration).max(0.0),
                            _ => tr.priority = text.parse().unwrap_or(tr.priority),
                        }
                    });
                }
            }
            Some("param") => {
                if let Some(name) = parts.get(1) {
                    let name = (*name).to_owned();
                    let kind = doc.graph.param(&name).map(|p| p.kind).unwrap_or_default();
                    let value = match kind {
                        ParamKind::Number => Value::Number(text.trim().parse().unwrap_or(0.0)),
                        ParamKind::Bool | ParamKind::Trigger => Value::Bool(text.trim() == "true"),
                        ParamKind::Text => Value::Text(text.to_owned()),
                    };
                    // While playing, a parameter is a live input to the
                    // running graph rather than an edit of its default.
                    match doc.player.as_mut().filter(|_| doc.playing) {
                        Some(player) => player.set(&name, value),
                        None => doc.edit(|g| {
                            if let Some(p) = g.params.iter_mut().find(|p| p.name == name) {
                                p.default = value;
                            }
                        }),
                    }
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn anim_add_param(&mut self, kind: ParamKind, cx: &mut Context<Self>) {
        let name = self
            .anim
            .graph
            .inputs
            .get("new-param")
            .map(|i| i.read(cx).value().trim().to_owned())
            .unwrap_or_default();
        let doc = &mut self.anim.graph;
        if name.is_empty() || name.contains(':') || doc.graph.param(&name).is_some() {
            self.anim.status = Some("Name the parameter (unique, no ':') first".into());
        } else {
            let default = match kind {
                ParamKind::Number => Value::Number(0.0),
                ParamKind::Text => Value::Text(String::new()),
                _ => Value::Bool(false),
            };
            doc.edit(|g| {
                g.params.push(Param {
                    name,
                    kind,
                    default,
                })
            });
        }
        cx.notify();
    }

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
            .when_some(self.anim.status.clone(), |this, s| {
                this.child(
                    div()
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text_muted())
                        .child(s),
                )
            })
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
            let input =
                self.anim_graph_input(&format!("prop:{n}:{prop}"), &format_prop(value), window, cx);
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
                    &format_prop(value),
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

    fn anim_transitions_list(
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
        let new_name = self.anim_graph_input("new-param", "", window, cx);
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
