//! A [`Graph`] into instances, laid out as the module doc describes.

use std::collections::BTreeMap;

use rbx_dom::{Ref, Variant, WeakDom};

use super::codec::{guid, write_layout, Attrs, Layout};
use super::{set, HINT_PREFIX, MASKS, PARAM_FOLDER, PARAM_PREFIX};
use crate::graph::model::enums::{self, CURVE, WAIT_FOR, WHEN};
use crate::graph::model::{
    unique, Expression, Graph, MaskJoint, Node, NodeKind, ParamKind, Prop, Transition, Value,
    WaitFor, When,
};

/// The state-machine pseudo-states, named and placed as the editor seeds
/// them (`StateMachineData.ENTRY_STATE_NAME` / `ANY_STATE_NAME`).
const ENTRY_NAME: &str = "(Entry)";
const ANY_NAME: &str = "(Any)";
const ENTRY_AT: (f32, f32) = (-300., -10.);
const ANY_AT: (f32, f32) = (-300., -110.);
const OUTPUT_CLASS: &str = "AnimationValueOutputDefinition";

struct Writer<'a> {
    dom: &'a mut WeakDom,
    graph: &'a Graph,
    nodes: Vec<Ref>,
    masks: BTreeMap<String, Ref>,
    /// Expression source → the `AnimationValueNodeDefinition` holding it.
    expressions: BTreeMap<String, Ref>,
    /// Parameter → (property, the node or pin ObjectValue bound to it).
    hints: BTreeMap<String, Vec<(String, Ref)>>,
}

pub(super) fn graph(dom: &mut WeakDom, parent: Option<Ref>, graph: &Graph) -> Ref {
    let definition = dom.new_instance("AnimationGraphDefinition", &graph.name, parent);
    let mut root = Attrs::default();
    for p in &graph.params {
        // A Mask parameter's default is an instance, which attributes here
        // cannot hold; its folder still records it.
        if p.kind != ParamKind::Mask {
            root.value(&p.name, &p.default, p.kind.enum_type());
        }
    }
    for (k, v) in &graph.rbx_attributes {
        root.value(k, v, None);
    }
    root.write(dom, definition);

    let mut w = Writer {
        dom,
        graph,
        nodes: Vec::new(),
        masks: BTreeMap::new(),
        expressions: BTreeMap::new(),
        hints: BTreeMap::new(),
    };
    w.nodes = graph
        .nodes
        .iter()
        .map(|n| {
            let r = w
                .dom
                .new_instance("AnimationNodeDefinition", &n.id, Some(definition));
            set(w.dom, r, "NodeType", Variant::Enum(n.kind.value()));
            r
        })
        .collect();
    w.expressions(definition);
    w.masks(definition);
    for (node, &r) in graph.nodes.iter().zip(&w.nodes.clone()) {
        w.node(node, r);
    }
    w.params(definition);
    definition
}

impl Writer<'_> {
    /// One value node per distinct source: those the graph lists (keeping
    /// their names and places), then any a property names that it does not.
    fn expressions(&mut self, definition: Ref) {
        let mut list = self.graph.expressions.clone();
        let props = self.graph.nodes.iter().flat_map(|n| {
            n.props
                .values()
                .chain(n.inputs.iter().flat_map(|i| i.props.values()))
        });
        for prop in props {
            if let Prop::Expr(source) = prop {
                if !list.iter().any(|e| &e.source == source) {
                    let name = unique("Expression", |n| list.iter().any(|e| e.name == n));
                    list.push(Expression {
                        name,
                        source: source.clone(),
                        position: (0., 0.),
                        size: None,
                        collapsed: false,
                    });
                }
            }
        }
        for e in list {
            let node =
                self.dom
                    .new_instance("AnimationValueNodeDefinition", &e.name, Some(definition));
            set(self.dom, node, "NodeType", Variant::Enum(1));
            let mut a = Attrs::default();
            a.insert("Expression", Variant::String(e.source.clone()));
            a.write(self.dom, node);
            write_layout(
                self.dom,
                node,
                Layout {
                    position: e.position,
                    size: e.size,
                    collapsed: Some(e.collapsed),
                    ..Layout::default()
                },
            );
            self.expressions.entry(e.source).or_insert(node);
        }
    }

    fn masks(&mut self, definition: Ref) {
        if self.graph.masks.is_empty() {
            return;
        }
        let folder = self.dom.new_instance("Folder", MASKS, Some(definition));
        for mask in &self.graph.masks {
            let r = self.dom.new_instance("Folder", &mask.name, Some(folder));
            let mut a = Attrs::default();
            a.insert("RBX_MaskRigType", Variant::String(mask.rig_type.clone()));
            a.insert("RBX_MaskRigName", Variant::String(mask.rig_name.clone()));
            for (joint, weight) in mask.flatten() {
                a.insert(joint, Variant::Float64(f64::from(weight)));
            }
            a.write(self.dom, r);
            let mut order = 0;
            joints(self.dom, r, &mask.joints, &mut order);
            self.masks.insert(mask.name.clone(), r);
        }
    }

    /// `props` as attributes on `of`, with parameter bindings, expression
    /// wires and (for a Mask node) the mask reference beside them.
    fn props(&mut self, of: Ref, props: &BTreeMap<String, Prop>, kind: Option<NodeKind>) -> Attrs {
        let mut a = Attrs::default();
        for (k, prop) in props {
            match prop {
                Prop::Value(v) if kind == Some(NodeKind::Mask) && k == "Mask" => {
                    if let Some(&mask) = self.masks.get(&v.text()) {
                        let pin = self.dom.new_instance("ObjectValue", "Mask", Some(of));
                        set(self.dom, pin, "Value", Variant::Ref(mask));
                    }
                }
                // An `Instance` attribute names a state that does not exist
                // yet; `node` adds it once the state ObjectValues are made.
                Prop::Value(_) if kind == Some(NodeKind::StateMachine) && k == "EntryState" => {}
                Prop::Value(v) => a.value(k, v, enums::of_prop(k)),
                Prop::Param(name) => {
                    a.insert(k, Variant::String(format!("{PARAM_PREFIX}{name}")));
                    self.hints
                        .entry(name.clone())
                        .or_default()
                        .push((k.clone(), of));
                }
                Prop::Expr(source) => {
                    // The editor gives every wire its own output endpoint,
                    // left with the class's default name.
                    let wire = self.dom.new_instance("ObjectValue", k, Some(of));
                    if let Some(&node) = self.expressions.get(source) {
                        let out = self
                            .dom
                            .new_instance(OUTPUT_CLASS, OUTPUT_CLASS, Some(node));
                        set(self.dom, wire, "Value", Variant::Ref(out));
                    }
                }
            }
        }
        a
    }

    fn node(&mut self, node: &Node, r: Ref) {
        write_layout(
            self.dom,
            r,
            Layout {
                position: node.position,
                size: node.size,
                collapsed: Some(node.collapsed),
                display_name: node.renamed().then(|| node.name.clone()),
                z_index: Some(node.z_index),
            },
        );
        let mut attributes = self.props(r, &node.props, Some(node.kind));
        let dynamic = node.kind.grows_pins();
        let mut pins = Vec::new();
        for input in &node.inputs {
            if !dynamic && input.node.is_none() && input.props.is_empty() {
                pins.push(None);
                continue;
            }
            let pin = self.dom.new_instance("ObjectValue", &input.pin, Some(r));
            if let Some(&target) = input.node.and_then(|i| self.nodes.get(i)) {
                set(self.dom, pin, "Value", Variant::Ref(target));
            }
            self.props(pin, &input.props, None).write(self.dom, pin);
            if let Some(at) = input.position {
                write_layout(
                    self.dom,
                    pin,
                    Layout {
                        position: at,
                        ..Layout::default()
                    },
                );
            }
            pins.push(Some(pin));
        }
        if !node.inputs.is_empty() {
            let names: Vec<&str> = node.inputs.iter().map(|i| i.pin.as_str()).collect();
            set(self.dom, r, "InputPinData", input_pin_data(&names));
        }
        if node.kind == NodeKind::StateMachine {
            let entry = node.props.get("EntryState").map(|p| match p {
                Prop::Value(v) => v.text(),
                _ => String::new(),
            });
            let state = entry
                .filter(|e| !e.is_empty())
                .and_then(|e| node.inputs.iter().position(|i| i.pin == e))
                .and_then(|at| pins.get(at).copied().flatten());
            if let Some(state) = state {
                attributes.insert("EntryState", Variant::Ref(state));
            }
        }
        attributes.write(self.dom, r);
        if node.kind == NodeKind::StateMachine {
            self.state_machine(node, r, &pins);
        }
    }

    fn state_machine(&mut self, node: &Node, r: Ref, pins: &[Option<Ref>]) {
        for (name, flag, at) in [
            (ENTRY_NAME, "RBX_IsStart", ENTRY_AT),
            (ANY_NAME, "RBX_IsAny", ANY_AT),
        ] {
            let pseudo = self.dom.new_instance("ObjectValue", name, Some(r));
            let mut a = Attrs::default();
            a.insert(flag, Variant::Bool(true));
            a.write(self.dom, pseudo);
            write_layout(
                self.dom,
                pseudo,
                Layout {
                    position: at,
                    ..Layout::default()
                },
            );
        }
        let pin_name = |i: usize| node.inputs.get(i).map(|p| p.pin.as_str());
        for (k, t) in node.transitions.iter().enumerate() {
            let Some(&Some(to)) = pins.get(t.to) else {
                continue;
            };
            let from = match t.from {
                Some(f) => match pins.get(f) {
                    Some(&Some(p)) => Some(p),
                    _ => continue,
                },
                None => None,
            };
            let label = format!(
                "{}_to_{}",
                t.from.and_then(pin_name).unwrap_or("Any"),
                pin_name(t.to).unwrap_or_default()
            );
            let made = self
                .dom
                .new_instance("StateMachineTransitionDefinition", &label, Some(r));
            set(self.dom, made, "To", Variant::Ref(to));
            if let Some(from) = from {
                set(self.dom, made, "From", Variant::Ref(from));
            }
            set(
                self.dom,
                made,
                "Priority",
                Variant::Int32(t.priority.min(65535) as i32),
            );
            let id = if t.id.is_empty() {
                guid((&self.graph.name, &node.id, &label, k))
            } else {
                t.id.clone()
            };
            set(self.dom, made, "TransitionId", Variant::String(id));
            transition_attrs(t).write(self.dom, made);
        }
    }

    fn params(&mut self, definition: Ref) {
        for p in &self.graph.params {
            let mut places: Vec<(f32, f32)> = self
                .graph
                .param_refs
                .iter()
                .filter(|r| r.param == p.name)
                .map(|r| r.position)
                .collect();
            if places.is_empty() {
                places.push((0., 0.));
            }
            let hints = self.hints.remove(&p.name).unwrap_or_default();
            for (k, at) in places.into_iter().enumerate() {
                let folder_name = format!("{PARAM_FOLDER}{}", p.name);
                let folder = self
                    .dom
                    .new_instance("Folder", &folder_name, Some(definition));
                let mut a = Attrs::default();
                a.insert(
                    "AnimGraph_ParameterType",
                    Variant::String(p.kind.type_name().into()),
                );
                a.insert("AnimGraph_BindingName", Variant::String(p.name.clone()));
                a.write(self.dom, folder);
                write_layout(
                    self.dom,
                    folder,
                    Layout {
                        position: at,
                        ..Layout::default()
                    },
                );
                if k > 0 {
                    continue;
                }
                for (prop, target) in &hints {
                    let hint = format!("{HINT_PREFIX}{prop}");
                    let wire = self.dom.new_instance("ObjectValue", &hint, Some(folder));
                    set(self.dom, wire, "Value", Variant::Ref(*target));
                }
            }
        }
    }
}

fn joints(dom: &mut WeakDom, parent: Ref, list: &[MaskJoint], order: &mut u32) {
    for j in list {
        let r = dom.new_instance("NumberValue", &j.name, Some(parent));
        set(dom, r, "Value", Variant::Float64(f64::from(j.weight)));
        let mut a = Attrs::default();
        a.insert("Order", Variant::Float64(f64::from(*order)));
        a.write(dom, r);
        *order += 1;
        joints(dom, r, &j.children, order);
    }
}

fn transition_attrs(t: &Transition) -> Attrs {
    let mut a = Attrs::default();
    a.insert("Duration", Variant::Float64(f64::from(t.duration.max(0.))));
    // PoseEasingStyle numbers its items as `Style` does.
    let curve = Value::Number(f64::from(t.curve.value()));
    a.value("Curve", &curve, Some(&CURVE));
    a.insert("Expression", Variant::String(t.expression.clone()));
    let wait = match t.wait_for {
        WaitFor::Finished => "Finished",
        WaitFor::Trigger => "Trigger",
    };
    a.enum_item("WaitFor", &WAIT_FOR, wait);
    let when = match t.when {
        When::Finished => "Finished",
        When::BeforeFinished => "BeforeFinished",
    };
    a.enum_item("When", &WHEN, when);
    a
}

/// The `InputPinData` property as Studio saves it: a `u32` 1 (a version),
/// the pin count, then each pin's name behind a `u32` length. The property is
/// a `String` whose bytes these are, so the 1 and the counts are control
/// characters inside it.
fn input_pin_data(names: &[&str]) -> Variant {
    let mut raw = 1u32.to_le_bytes().to_vec();
    raw.extend((names.len() as u32).to_le_bytes());
    for name in names {
        raw.extend((name.len() as u32).to_le_bytes());
        raw.extend(name.as_bytes());
    }
    Variant::Unknown { type_id: 0x01, raw }
}
