//! Reads and writes a [`Graph`] as the instances Roblox's Graph Editor
//! stores one in. The layout follows what the shipping editor plugin writes:
//!
//! - The `AnimationGraphDefinition`'s attributes are the parameters'
//!   defaults, named after them, beside the editor's `RBX_` bookkeeping
//!   (`RBX_SourceAssetId`, `RBX_GraphDirty`, `RBX_GraphVersion`), which is
//!   never a parameter.
//! - Each node is an `AnimationNodeDefinition` named `ClipNode`, `ClipNode1`,
//!   … with its `NodeType`. `NodeId` is script-locked and left unset, as the
//!   editor leaves it. Its properties are attributes named as the engine
//!   names them, enums as `EnumItem` attributes. A `Configuration` child
//!   holds the editor's `AnimGraph_Position`, `AnimGraph_Size`,
//!   `AnimGraph_Collapsed`, `AnimGraph_DisplayName` and `AnimGraph_ZIndex`.
//! - An input pin is an `ObjectValue` child of the node it feeds, named
//!   after the pin, whose `Value` is the source node (nil while unwired);
//!   per-input settings are attributes on it.
//! - A property bound to parameter `P` holds the string `param::P`. Each
//!   parameter has a `Parameter_<P>` folder (`AnimGraph_ParameterType`,
//!   `AnimGraph_BindingName`, a `Configuration` for its place) holding
//!   `connectionHint_<property>` ObjectValues that only draw the wire.
//! - An expression is an `AnimationValueNodeDefinition` (`NodeType`
//!   Expression) with an `Expression` attribute and an
//!   `AnimationValueOutputDefinition` child. A property wired to it has no
//!   attribute; an ObjectValue named after the property points at that
//!   output instead.
//! - A state machine's states are its pins, each with a `Configuration` for
//!   its place; the "(Entry)" and "(Any)" pseudo-states are ObjectValues
//!   marked `RBX_IsStart` / `RBX_IsAny`. Transitions are
//!   `StateMachineTransitionDefinition` children named `<From>_to_<To>`,
//!   with a GUID `TransitionId`, `From`/`To` pointing at the state
//!   ObjectValues (`From` nil is "(Any)"), `Priority`, and `Duration`,
//!   `Curve`, `Expression`, `WaitFor` and `When` attributes.
//! - Masks are folders under a `SharedMasks` folder, a tree of `NumberValue`
//!   joint weights mirrored as `{joint = weight}` attributes on the folder
//!   (which is what the engine reads). A Mask node's `Mask` is an
//!   ObjectValue child pointing at one.
//!
//! Three things differ from the editor because they cannot be matched
//! here. `EntryState` is an `Instance` attribute in Roblox; the attribute
//! codec has no such type, so it is written as the entry state's name (the
//! reader takes either that or an ObjectValue/Ref to the state or its node).
//! The names of the pseudo-state ObjectValues ("Entry", "Any") and of the
//! expression output ("Output") are unverified. `InputPinData`'s layout is
//! undocumented, so a blob read from a file is written back only while that
//! node's pins are unchanged; otherwise the pins' child order stands alone.
//!
//! Graphs written by this crate's earlier layout (`EditorPosition`,
//! `$name`/`=source` strings, `Trigger:<name>` tags, transitions pointing at
//! nodes) still read.

use std::collections::BTreeMap;

use rbx_dom::{attributes, Ref, Variant, WeakDom};

use crate::graph::model::Graph;

mod codec;
mod read;
mod write;

const ATTRIBUTES: &str = "AttributesSerialize";
const CONFIGURATION: &str = "Configuration";
const PARAM_PREFIX: &str = "param::";
const PARAM_FOLDER: &str = "Parameter_";
const HINT_PREFIX: &str = "connectionHint_";
const MASKS: &str = "SharedMasks";
const LEGACY_TRIGGER_TAG: &str = "Trigger:";

fn attrs(dom: &WeakDom, of: Ref) -> BTreeMap<String, Variant> {
    dom.get(of).map(|i| i.attributes()).unwrap_or_default()
}

fn children(dom: &WeakDom, of: Ref, class: &str) -> Vec<Ref> {
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

fn name(dom: &WeakDom, of: Ref) -> String {
    dom.get(of).map(|i| i.name().to_owned()).unwrap_or_default()
}

fn set(dom: &mut WeakDom, of: Ref, property: &str, value: Variant) {
    // Only fails for a referent not in the DOM; every one here was just made.
    let _ = dom.set_property(of, property, value);
}

/// Reads the graph under an `AnimationGraphDefinition`.
pub fn read(dom: &WeakDom, definition: Ref) -> Graph {
    read::graph(dom, definition)
}

/// Writes `graph` as a new `AnimationGraphDefinition` under `parent`.
pub fn write(dom: &mut WeakDom, parent: Option<Ref>, graph: &Graph) -> Ref {
    write::graph(dom, parent, graph)
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
    match dom
        .get(fresh)
        .and_then(|i| i.properties().get(ATTRIBUTES))
        .cloned()
    {
        Some(v) => set(dom, definition, ATTRIBUTES, v),
        None => {
            let _ = dom.remove_property(definition, ATTRIBUTES);
        }
    }
    // Trigger tags were how parameters' types used to be kept; the folders
    // hold them now, and any other tag is the user's own.
    let tags: Vec<String> = dom
        .get(definition)
        .map(|i| i.tags().into_iter().map(str::to_owned).collect())
        .unwrap_or_default();
    if tags.iter().any(|t| t.starts_with(LEGACY_TRIGGER_TAG)) {
        let kept: Vec<&str> = tags
            .iter()
            .map(String::as_str)
            .filter(|t| !t.starts_with(LEGACY_TRIGGER_TAG))
            .collect();
        set(
            dom,
            definition,
            "Tags",
            Variant::String(attributes::encode_tags(&kept)),
        );
    }
    dom.remove(fresh);
    if dom.get(definition).is_some_and(|i| i.name() != graph.name) {
        let _ = dom.set_name(definition, &graph.name);
    }
}

#[cfg(test)]
mod tests;
