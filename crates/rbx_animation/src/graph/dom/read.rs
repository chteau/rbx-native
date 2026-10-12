//! Instances back into a [`Graph`]: the layout the module doc describes,
//! and this crate's earlier one.

use std::cell::RefCell;
use std::collections::BTreeMap;

use rbx_dom::{Ref, Variant, WeakDom};

use super::codec::{named_value, point, prop_of, read_layout, value_of};
use super::{attrs, children, name, reference, LEGACY_TRIGGER_TAG, MASKS, PARAM_FOLDER};
use crate::easing::Style;
use crate::graph::model::{
    unique, Expression, Graph, Input, Mask, MaskJoint, Node, NodeKind, Param, ParamKind, ParamRef,
    Prop, Transition, Value, WaitFor, When, ENGINE_TRANSITION_DURATION,
};

struct Reader<'a> {
    dom: &'a WeakDom,
    nodes: Vec<Ref>,
    /// An expression's output (or the value node itself) → its source.
    sources: BTreeMap<Ref, String>,
    masks: BTreeMap<Ref, String>,
    /// Masks found outside `SharedMasks` while reading nodes.
    loose: RefCell<Vec<Mask>>,
}

pub(super) fn graph(dom: &WeakDom, definition: Ref) -> Graph {
    let Some(root) = dom.get(definition) else {
        return Graph::default();
    };
    let mut graph = Graph {
        name: root.name().to_owned(),
        nodes: Vec::new(),
        ..Graph::default()
    };
    let mut r = Reader {
        dom,
        nodes: children(dom, definition, "AnimationNodeDefinition"),
        sources: BTreeMap::new(),
        masks: BTreeMap::new(),
        loose: RefCell::new(Vec::new()),
    };
    params(dom, definition, &mut graph);
    r.expressions(definition, &mut graph);
    r.masks(definition, &mut graph);
    graph.nodes = r.nodes.iter().map(|&n| r.node(n)).collect();
    graph.masks.append(&mut r.loose.borrow_mut());
    if graph.output().is_none() {
        graph
            .nodes
            .push(Node::new(NodeKind::GraphOutput, "GraphOutput"));
    }
    graph
}

/// Parameters from their folders (which hold the type) and the root's
/// attributes (which hold the defaults), in folder order.
fn params(dom: &WeakDom, definition: Ref, graph: &mut Graph) {
    let defaults = attrs(dom, definition);
    let mut kinds: Vec<(String, ParamKind)> = Vec::new();
    for folder in children(dom, definition, "Folder") {
        let folder_name = name(dom, folder);
        let Some(stem) = folder_name.strip_prefix(PARAM_FOLDER) else {
            continue;
        };
        let a = attrs(dom, folder);
        let binding = match a.get("AnimGraph_BindingName") {
            Some(Variant::String(s)) if !s.is_empty() => s.clone(),
            _ => stem.to_owned(),
        };
        let kind = match a.get("AnimGraph_ParameterType") {
            Some(Variant::String(t)) => ParamKind::from_type_name(t),
            _ => None,
        };
        let position = read_layout(dom, folder).map_or((0., 0.), |l| l.position);
        graph.param_refs.push(ParamRef {
            param: binding.clone(),
            position,
        });
        if !kinds.iter().any(|(n, _)| *n == binding) {
            let kind = kind.unwrap_or_else(|| inferred(defaults.get(&binding)));
            kinds.push((binding, kind));
        }
    }
    let tags: Vec<&str> = dom.get(definition).map(|i| i.tags()).unwrap_or_default();
    for name in defaults.keys().filter(|n| !n.starts_with("RBX_")) {
        if !kinds.iter().any(|(n, _)| n == name) {
            let kind = match inferred(defaults.get(name)) {
                ParamKind::Bool
                    if tags.contains(&format!("{LEGACY_TRIGGER_TAG}{name}").as_str()) =>
                {
                    ParamKind::Trigger
                }
                other => other,
            };
            kinds.push((name.clone(), kind));
        }
    }
    graph.params = kinds
        .into_iter()
        .map(|(name, kind)| {
            let default = match (defaults.get(&name), kind.enum_type()) {
                (Some(Variant::Enum(n)), Some(e)) => e.item(*n).map(|i| Value::Text(i.to_owned())),
                (Some(v), _) => value_of(v),
                (None, _) => None,
            }
            .unwrap_or_else(|| kind.default_value());
            Param {
                name,
                kind,
                default,
            }
        })
        .collect();
    graph.rbx_attributes = defaults
        .iter()
        .filter(|(n, _)| n.starts_with("RBX_"))
        .filter_map(|(n, v)| Some((n.clone(), value_of(v)?)))
        .collect();
}

fn inferred(v: Option<&Variant>) -> ParamKind {
    match v.and_then(value_of) {
        Some(Value::Bool(_)) => ParamKind::Bool,
        Some(Value::Text(_)) => ParamKind::Text,
        _ => ParamKind::Number,
    }
}

impl Reader<'_> {
    fn expressions(&mut self, definition: Ref, graph: &mut Graph) {
        for node in children(self.dom, definition, "AnimationValueNodeDefinition") {
            let source = match attrs(self.dom, node).get("Expression") {
                Some(Variant::String(s)) => s.clone(),
                _ => String::new(),
            };
            let layout = read_layout(self.dom, node).unwrap_or_default();
            self.sources.insert(node, source.clone());
            for out in children(self.dom, node, "AnimationValueOutputDefinition") {
                self.sources.insert(out, source.clone());
            }
            graph.expressions.push(Expression {
                name: name(self.dom, node),
                source,
                position: layout.position,
                size: layout.size,
                collapsed: layout.collapsed.unwrap_or(false),
            });
        }
    }

    fn masks(&mut self, definition: Ref, graph: &mut Graph) {
        let Some(folder) = children(self.dom, definition, "Folder")
            .into_iter()
            .find(|f| name(self.dom, *f) == MASKS)
        else {
            return;
        };
        for m in children(self.dom, folder, "Folder") {
            let a = attrs(self.dom, m);
            let text = |k: &str| match a.get(k) {
                Some(Variant::String(s)) => s.clone(),
                _ => String::new(),
            };
            let mut joints = joints(self.dom, m);
            // Without the NumberValue tree the mirrored attributes are the
            // mask, flat.
            if joints.is_empty() {
                joints = flat_joints(&a);
            }
            let mask = Mask {
                name: name(self.dom, m),
                rig_type: text("RBX_MaskRigType"),
                rig_name: text("RBX_MaskRigName"),
                joints,
            };
            self.masks.insert(m, mask.name.clone());
            graph.masks.push(mask);
        }
    }

    fn node(&self, r: Ref) -> Node {
        let dom = self.dom;
        let kind = match dom.get(r).and_then(|i| i.properties().get("NodeType")) {
            Some(Variant::Enum(v)) => NodeKind::from_value(*v),
            _ => None,
        }
        .unwrap_or(NodeKind::Clip);
        let layout = read_layout(dom, r);
        let legacy = layout.is_none();
        let instance_name = name(dom, r);
        let node_id = match dom.get(r).and_then(|i| i.properties().get("NodeId")) {
            Some(Variant::String(s)) if legacy && !s.is_empty() => s.clone(),
            _ => instance_name.clone(),
        };
        let mut node = Node::new(kind, &node_id);
        if kind.grows_pins() {
            node.inputs.clear();
        }
        match layout {
            Some(l) => {
                node.position = l.position;
                node.size = l.size;
                node.collapsed = l.collapsed.unwrap_or(false);
                node.z_index = l.z_index.unwrap_or(0.);
                if let Some(display) = l.display_name {
                    node.name = display;
                }
            }
            // The earlier layout kept the display name as the instance name.
            None => node.name = instance_name,
        }
        let mut entry: Option<Ref> = reference(dom, r, "EntryState");
        for (k, v) in attrs(dom, r) {
            if let (true, "EntryState", Variant::Ref(state)) =
                (kind == NodeKind::StateMachine, k.as_str(), &v)
            {
                entry = entry.or(dom.get(*state).map(|_| *state));
                continue;
            }
            if legacy && k == "EditorPosition" {
                node.position = point(Some(&v)).unwrap_or_default();
            } else if let Some(prop) = prop_of(&k, &v, legacy) {
                node.props.insert(k, prop);
            }
        }
        let mut pins: Vec<Ref> = Vec::new();
        for pin in children(dom, r, "ObjectValue") {
            let pin_name = name(dom, pin);
            let a = attrs(dom, pin);
            let target = reference(dom, pin, "Value");
            if a.contains_key("RBX_IsStart") || a.contains_key("RBX_IsAny") {
                continue;
            }
            if let Some(source) = target.and_then(|t| self.sources.get(&t)) {
                node.props.insert(pin_name, Prop::Expr(source.clone()));
                continue;
            }
            if kind == NodeKind::Mask && pin_name == "Mask" {
                let mask = match target.and_then(|t| self.masks.get(&t)) {
                    Some(shared) => shared.clone(),
                    None => self.loose_mask(pin, target),
                };
                node.props.insert(pin_name, Prop::Value(Value::Text(mask)));
                continue;
            }
            if kind == NodeKind::StateMachine && pin_name == "EntryState" {
                entry = target;
                continue;
            }
            let mut input = Input {
                pin: pin_name,
                node: target.and_then(|t| self.nodes.iter().position(|n| *n == t)),
                props: a
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), prop_of(k, v, legacy)?)))
                    .collect(),
                position: read_layout(dom, pin).map(|l| l.position),
            };
            for wire in children(dom, pin, "ObjectValue") {
                let source = reference(dom, wire, "Value").and_then(|t| self.sources.get(&t));
                if let Some(source) = source {
                    input
                        .props
                        .insert(name(dom, wire), Prop::Expr(source.clone()));
                }
            }
            match node.inputs.iter_mut().find(|i| i.pin == input.pin) {
                Some(fixed) if !kind.grows_pins() => {
                    fixed.node = input.node;
                    fixed.props.extend(input.props);
                }
                _ => node.inputs.push(input),
            }
            pins.push(pin);
        }
        if kind == NodeKind::StateMachine {
            let defs = children(dom, r, "StateMachineTransitionDefinition");
            // A transition out of "(Entry)" is the entry in all but name;
            // the editor keeps that one as `EntryState` instead.
            let from_entry = defs.iter().find_map(|&t| {
                let from = endpoint(dom, t, "From")?;
                attrs(dom, from)
                    .contains_key("RBX_IsStart")
                    .then(|| endpoint(dom, t, "To"))?
            });
            if let Some(state) = entry
                .or(from_entry)
                .and_then(|e| self.state(&node, &pins, e))
            {
                let pin = node.inputs[state].pin.clone();
                node.props
                    .insert("EntryState".into(), Prop::Value(Value::Text(pin)));
            }
            node.transitions = defs
                .into_iter()
                .filter_map(|t| self.transition(&node, &pins, t))
                .collect();
        }
        node
    }

    /// A Mask node's mask kept outside `SharedMasks`, which Roblox also
    /// reads: weights as attributes on the `Mask` ObjectValue itself, or on
    /// whatever instance it points at. The model only names shared masks,
    /// so it becomes one (and is saved back as one).
    fn loose_mask(&self, pin: Ref, target: Option<Ref>) -> String {
        let holder = target.unwrap_or(pin);
        let joints = flat_joints(&attrs(self.dom, holder));
        if joints.is_empty() {
            return String::new();
        }
        let mut loose = self.loose.borrow_mut();
        if let Some(found) = loose.iter().find(|m| m.joints == joints) {
            return found.name.clone();
        }
        let taken =
            |n: &str| self.masks.values().any(|m| m == n) || loose.iter().any(|m| m.name == n);
        let name = unique(&name(self.dom, holder), taken);
        loose.push(Mask {
            name: name.clone(),
            rig_type: "Custom".into(),
            rig_name: String::new(),
            joints,
        });
        name
    }

    /// The input a state reference names: one of the state ObjectValues, or
    /// (as the earlier layout and Roblox's docs have it) the node feeding it.
    fn state(&self, node: &Node, pins: &[Ref], target: Ref) -> Option<usize> {
        if let Some(at) = pins.iter().position(|p| *p == target) {
            return Some(at);
        }
        let fed_by = self.nodes.iter().position(|n| *n == target)?;
        node.inputs.iter().position(|i| i.node == Some(fed_by))
    }

    fn transition(&self, node: &Node, pins: &[Ref], t: Ref) -> Option<Transition> {
        let dom = self.dom;
        let to = self.state(node, pins, endpoint(dom, t, "To")?)?;
        let from = match endpoint(dom, t, "From") {
            Some(f) if attrs(dom, f).contains_key("RBX_IsStart") => return None,
            Some(f) if attrs(dom, f).contains_key("RBX_IsAny") => None,
            Some(f) => Some(self.state(node, pins, f)?),
            None => None,
        };
        let a = attrs(dom, t);
        let get = |k: &str| a.get(k).and_then(|v| named_value(k, v));
        let props = dom.get(t).map(|i| i.properties());
        Some(Transition {
            id: match props.and_then(|p| p.get("TransitionId")) {
                Some(Variant::String(s)) => s.clone(),
                _ => String::new(),
            },
            from,
            to,
            wait_for: match get("WaitFor") {
                Some(v) if v.text() == "Finished" => WaitFor::Finished,
                _ => WaitFor::Trigger,
            },
            expression: get("Expression").map(|v| v.text()).unwrap_or_default(),
            priority: props
                .and_then(|p| p.get("Priority"))
                .and_then(value_of)
                .map_or(0, |v| v.number().clamp(0., 65535.) as u32),
            duration: get("Duration").map_or(ENGINE_TRANSITION_DURATION, |v| v.number() as f32),
            // The editor's `coerceCurve` takes an EnumItem, its number or its
            // name.
            curve: match a.get("Curve") {
                Some(Variant::Enum(n)) => Style::from_value(*n),
                Some(Variant::String(s)) if s == "CubicV2" => Style::CubicV2,
                Some(v) => match value_of(v) {
                    Some(Value::Number(n)) => Style::from_value(n as u32),
                    _ => Style::Linear,
                },
                None => Style::Linear,
            },
            when: match get("When") {
                Some(v) if v.text() == "BeforeFinished" => When::BeforeFinished,
                _ => When::Finished,
            },
        })
    }
}

/// A transition's `From` or `To`: the property the editor sets, or (as
/// Roblox's docs describe it) an ObjectValue child of that name.
fn endpoint(dom: &WeakDom, t: Ref, end: &str) -> Option<Ref> {
    reference(dom, t, end).or_else(|| {
        children(dom, t, "ObjectValue")
            .into_iter()
            .find(|c| name(dom, *c) == end)
            .and_then(|c| reference(dom, c, "Value"))
    })
}

/// `{joint = weight}` attributes as a flat list of joints.
fn flat_joints(a: &BTreeMap<String, Variant>) -> Vec<MaskJoint> {
    a.iter()
        .filter(|(k, _)| !k.starts_with("RBX_"))
        .filter_map(|(k, v)| {
            Some(MaskJoint {
                name: k.clone(),
                weight: value_of(v)?.number() as f32,
                children: Vec::new(),
            })
        })
        .collect()
}

/// A mask's NumberValue tree, siblings in their `Order`.
fn joints(dom: &WeakDom, of: Ref) -> Vec<MaskJoint> {
    let mut list: Vec<(f64, MaskJoint)> = children(dom, of, "NumberValue")
        .into_iter()
        .map(|j| {
            let weight = dom
                .get(j)
                .and_then(|i| i.properties().get("Value"))
                .and_then(value_of)
                .map_or(1., |v| v.number() as f32);
            let order = attrs(dom, j)
                .get("Order")
                .and_then(value_of)
                .map_or(f64::MAX, |v| v.number());
            let joint = MaskJoint {
                name: name(dom, j),
                weight,
                children: joints(dom, j),
            };
            (order, joint)
        })
        .collect();
    list.sort_by(|a, b| a.0.total_cmp(&b.0));
    list.into_iter().map(|(_, j)| j).collect()
}
