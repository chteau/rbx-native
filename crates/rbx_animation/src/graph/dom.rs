//! Reads and writes a [`Graph`] as the instances Roblox stores one in.
//!
//! The tree is an `AnimationGraphDefinition` holding one
//! `AnimationNodeDefinition` per node (`NodeId`, `NodeType`). A node's input
//! pins are `ObjectValue` children named after the pin, whose `Value` is the
//! node wired in; per-input settings are attributes on that `ObjectValue`. A
//! state machine's transitions are `StateMachineTransitionDefinition`
//! children, with `From`/`To` pointing at the state's node (`From` unset is
//! "(Any)") and `Priority`. Roblox's docs settle all of that. They do not say
//! where a node keeps its own properties. Here they are attributes on the
//! node, written as plain values, `$name` for a value driven by parameter
//! `name`, or `=source` for an expression. The editor position is the
//! `EditorPosition` attribute. The graph's parameters are attributes on the
//! definition, which is what `GetParameterDefaults` reads. A trigger
//! parameter is a bool attribute that is also tagged `Trigger:<name>`.

use std::collections::BTreeMap;

use rbx_dom::{attributes, Ref, Variant, Vector2Data, WeakDom};

use crate::easing::Style;
use crate::graph::model::{
    Graph, Input, Node, NodeKind, Param, ParamKind, Prop, Transition, Value, WaitFor,
};

const ATTRIBUTES: &str = "AttributesSerialize";
const POSITION: &str = "EditorPosition";
const TRIGGER_TAG: &str = "Trigger:";

fn attrs(dom: &WeakDom, of: Ref) -> BTreeMap<String, Variant> {
    dom.get(of).map(|i| i.attributes()).unwrap_or_default()
}

fn set_attrs(dom: &mut WeakDom, of: Ref, map: &BTreeMap<String, Variant>) {
    if map.is_empty() {
        return;
    }
    // Attributes here are never enums, so the enum-name callback never runs.
    if let Some(raw) = attributes::encode(map, |_| None) {
        let _ = dom.set_property(of, ATTRIBUTES, Variant::Unknown { type_id: 0x01, raw });
    }
}

fn value_of(v: &Variant) -> Option<Value> {
    Some(match v {
        Variant::Float64(n) => Value::Number(*n),
        Variant::Float32(n) => Value::Number(f64::from(*n)),
        Variant::Int32(n) => Value::Number(f64::from(*n)),
        Variant::Bool(b) => Value::Bool(*b),
        Variant::String(s) => Value::Text(s.clone()),
        _ => return None,
    })
}

fn variant_of(v: &Value) -> Variant {
    match v {
        Value::Number(n) => Variant::Float64(*n),
        Value::Bool(b) => Variant::Bool(*b),
        Value::Text(t) => Variant::String(t.clone()),
    }
}

fn prop_of(v: &Variant) -> Option<Prop> {
    let value = value_of(v)?;
    Some(match value {
        Value::Text(t) if t.starts_with('$') && t.len() > 1 => Prop::Param(t[1..].to_owned()),
        Value::Text(t) if t.starts_with('=') => Prop::Expr(t[1..].to_owned()),
        other => Prop::Value(other),
    })
}

fn variant_of_prop(p: &Prop) -> Variant {
    match p {
        Prop::Value(v) => variant_of(v),
        Prop::Param(name) => Variant::String(format!("${name}")),
        Prop::Expr(source) => Variant::String(format!("={source}")),
    }
}

fn children<'a>(dom: &'a WeakDom, of: Ref, class: &'a str) -> Vec<Ref> {
    dom.get(of)
        .map(|i| i.children().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|c| dom.get(*c).is_some_and(|i| i.class() == class))
        .collect()
}

fn reference(dom: &WeakDom, of: Ref, property: &str) -> Option<Ref> {
    match dom.get(of)?.properties().get(property)? {
        Variant::Ref(r) if dom.get(*r).is_some() => Some(*r),
        _ => None,
    }
}

fn text(dom: &WeakDom, of: Ref, property: &str) -> String {
    match dom.get(of).and_then(|i| i.properties().get(property)) {
        Some(Variant::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Reads the graph under an `AnimationGraphDefinition`.
pub fn read(dom: &WeakDom, definition: Ref) -> Graph {
    let Some(root) = dom.get(definition) else {
        return Graph::default();
    };
    let tags: Vec<&str> = root.tags();
    let params = attrs(dom, definition)
        .iter()
        .filter_map(|(name, v)| {
            let default = value_of(v)?;
            let kind = match &default {
                Value::Bool(_) if tags.contains(&format!("{TRIGGER_TAG}{name}").as_str()) => {
                    ParamKind::Trigger
                }
                Value::Bool(_) => ParamKind::Bool,
                Value::Number(_) => ParamKind::Number,
                Value::Text(_) => ParamKind::Text,
            };
            Some(Param {
                name: name.clone(),
                kind,
                default,
            })
        })
        .collect();

    let refs = children(dom, definition, "AnimationNodeDefinition");
    let index_of = |r: Ref| refs.iter().position(|x| *x == r);
    let mut nodes: Vec<Node> = refs
        .iter()
        .map(|&r| {
            let kind = match dom.get(r).and_then(|i| i.properties().get("NodeType")) {
                Some(Variant::Enum(v)) => NodeKind::from_value(*v),
                _ => None,
            }
            .unwrap_or(NodeKind::Clip);
            let mut node = Node::new(kind, &text(dom, r, "NodeId"));
            node.name = dom.get(r).map(|i| i.name().to_owned()).unwrap_or_default();
            for (name, v) in attrs(dom, r) {
                if name == POSITION {
                    if let Variant::Vector2(p) = v {
                        node.position = (p.x, p.y);
                    }
                } else if let Some(prop) = prop_of(&v) {
                    node.props.insert(name, prop);
                }
            }
            node.inputs = children(dom, r, "ObjectValue")
                .into_iter()
                .map(|pin| Input {
                    pin: dom
                        .get(pin)
                        .map(|i| i.name().to_owned())
                        .unwrap_or_default(),
                    node: reference(dom, pin, "Value").and_then(index_of),
                    props: attrs(dom, pin)
                        .iter()
                        .filter_map(|(k, v)| Some((k.clone(), prop_of(v)?)))
                        .collect(),
                })
                .collect();
            node
        })
        .collect();

    // Transitions name states by node; map each back to the input feeding it.
    for (at, &r) in refs.iter().enumerate() {
        let state_of = |target: Option<Ref>, nodes: &[Node]| {
            let target = index_of(target?)?;
            nodes[at].inputs.iter().position(|i| i.node == Some(target))
        };
        let transitions: Vec<Transition> = children(dom, r, "StateMachineTransitionDefinition")
            .into_iter()
            .filter_map(|t| {
                let to = state_of(reference(dom, t, "To"), &nodes)?;
                let from = state_of(reference(dom, t, "From"), &nodes);
                let a = attrs(dom, t);
                let get = |k: &str| a.get(k).and_then(value_of);
                Some(Transition {
                    from,
                    to,
                    wait_for: if get("WaitFor").is_some_and(|v| v.text() == "Finished") {
                        WaitFor::Finished
                    } else {
                        WaitFor::Trigger
                    },
                    expression: get("Expression").map(|v| v.text()).unwrap_or_default(),
                    priority: match dom.get(t).and_then(|i| i.properties().get("Priority")) {
                        Some(Variant::Int32(p)) => (*p).max(0) as u32,
                        _ => 0,
                    },
                    duration: get("Duration").map_or(0.15, |v| v.number() as f32),
                    curve: if get("Curve").is_some_and(|v| v.text() == "CubicV2") {
                        Style::CubicV2
                    } else {
                        Style::Linear
                    },
                })
            })
            .collect();
        nodes[at].transitions = transitions;
    }

    let mut graph = Graph {
        name: root.name().to_owned(),
        nodes,
        params,
    };
    if graph.output().is_none() {
        graph.nodes.push(Node::new(NodeKind::GraphOutput, "Output"));
    }
    graph
}

fn set(dom: &mut WeakDom, of: Ref, property: &str, value: Variant) {
    // Only fails for a referent not in the DOM; every one here was just made.
    let _ = dom.set_property(of, property, value);
}

/// Writes `graph` as a new `AnimationGraphDefinition` under `parent`.
pub fn write(dom: &mut WeakDom, parent: Option<Ref>, graph: &Graph) -> Ref {
    let definition = dom.new_instance("AnimationGraphDefinition", &graph.name, parent);
    let params: BTreeMap<String, Variant> = graph
        .params
        .iter()
        .map(|p| (p.name.clone(), variant_of(&p.default)))
        .collect();
    set_attrs(dom, definition, &params);
    let triggers: Vec<String> = graph
        .params
        .iter()
        .filter(|p| p.kind == ParamKind::Trigger)
        .map(|p| format!("{TRIGGER_TAG}{}", p.name))
        .collect();
    if !triggers.is_empty() {
        let borrowed: Vec<&str> = triggers.iter().map(String::as_str).collect();
        set(
            dom,
            definition,
            "Tags",
            Variant::String(attributes::encode_tags(&borrowed)),
        );
    }

    let refs: Vec<Ref> = graph
        .nodes
        .iter()
        .map(|node| {
            let r = dom.new_instance("AnimationNodeDefinition", &node.name, Some(definition));
            set(dom, r, "NodeId", Variant::String(node.id.clone()));
            set(dom, r, "NodeType", Variant::Enum(node.kind.value()));
            let mut map: BTreeMap<String, Variant> = node
                .props
                .iter()
                .map(|(k, p)| (k.clone(), variant_of_prop(p)))
                .collect();
            map.insert(
                POSITION.into(),
                Variant::Vector2(Vector2Data {
                    x: node.position.0,
                    y: node.position.1,
                }),
            );
            set_attrs(dom, r, &map);
            r
        })
        .collect();

    for (node, &r) in graph.nodes.iter().zip(&refs) {
        for input in &node.inputs {
            let pin = dom.new_instance("ObjectValue", &input.pin, Some(r));
            if let Some(target) = input.node.and_then(|i| refs.get(i)) {
                set(dom, pin, "Value", Variant::Ref(*target));
            }
            let map = input
                .props
                .iter()
                .map(|(k, p)| (k.clone(), variant_of_prop(p)))
                .collect();
            set_attrs(dom, pin, &map);
        }
        let state = |i: usize| {
            node.inputs
                .get(i)
                .and_then(|input| input.node)
                .and_then(|n| refs.get(n))
                .copied()
        };
        for (k, t) in node.transitions.iter().enumerate() {
            let Some(to) = state(t.to) else { continue };
            let made = dom.new_instance("StateMachineTransitionDefinition", "Transition", Some(r));
            set(dom, made, "To", Variant::Ref(to));
            if let Some(from) = t.from.and_then(state) {
                set(dom, made, "From", Variant::Ref(from));
            }
            set(dom, made, "Priority", Variant::Int32(t.priority as i32));
            set(
                dom,
                made,
                "TransitionId",
                Variant::String(format!("{}T{}", node.id, k + 1)),
            );
            let wait = if t.wait_for == WaitFor::Finished {
                "Finished"
            } else {
                "Trigger"
            };
            let curve = if t.curve == Style::CubicV2 {
                "CubicV2"
            } else {
                "Linear"
            };
            let map = BTreeMap::from([
                (
                    "Expression".to_owned(),
                    Variant::String(t.expression.clone()),
                ),
                ("WaitFor".to_owned(), Variant::String(wait.into())),
                (
                    "Duration".to_owned(),
                    Variant::Float64(f64::from(t.duration)),
                ),
                ("Curve".to_owned(), Variant::String(curve.into())),
            ]);
            set_attrs(dom, made, &map);
        }
    }
    definition
}

/// Replaces an existing definition's contents with `graph`, keeping the
/// instance (and what points at it).
pub fn overwrite(dom: &mut WeakDom, definition: Ref, graph: &Graph) {
    for child in dom
        .get(definition)
        .map(|i| i.children().to_vec())
        .unwrap_or_default()
    {
        dom.remove(child);
    }
    let fresh = write(dom, None, graph);
    for child in dom
        .get(fresh)
        .map(|i| i.children().to_vec())
        .unwrap_or_default()
    {
        dom.set_parent(child, Some(definition));
    }
    for property in [ATTRIBUTES, "Tags"] {
        match dom
            .get(fresh)
            .and_then(|i| i.properties().get(property))
            .cloned()
        {
            Some(v) => set(dom, definition, property, v),
            None => {
                let _ = dom.remove_property(definition, property);
            }
        }
    }
    dom.remove(fresh);
    if dom.get(definition).is_some_and(|i| i.name() != graph.name) {
        let _ = dom.set_name(definition, &graph.name);
    }
}

#[cfg(test)]
mod tests;
