//! The graph panel's fields: one per editable value, committed on Enter
//! or blur, and new parameters.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_animation::graph::{Param, ParamKind, Value};

use super::Shell;
use crate::anim_editor::graph_canvas::parse_prop;

impl Shell {
    /// The field editing `key`, made on first use and kept in step with
    /// `seed` while it is not being typed in.
    pub(in crate::shell::animation) fn anim_graph_input(
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

    pub(in crate::shell::animation) fn anim_commit_graph_input(
        &mut self,
        key: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) {
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
                        // A mask or enum parameter's value is its name.
                        _ => Value::Text(text.to_owned()),
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

    pub(in crate::shell::animation) fn anim_add_param(
        &mut self,
        kind: ParamKind,
        cx: &mut Context<Self>,
    ) {
        let name = self
            .anim
            .graph
            .inputs
            .get("new-param")
            .map(|i| i.read(cx).value().trim().to_owned())
            .unwrap_or_default();
        let doc = &mut self.anim.graph;
        if name.is_empty() || name.contains(':') || doc.graph.param(&name).is_some() {
            self.anim.warn("Name the parameter (unique, no ':') first");
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
            // Made again, empty, on the next render.
            doc.inputs.remove("new-param");
        }
        cx.notify();
    }
}
