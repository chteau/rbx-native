//! Instance node in the DOM tree.

use std::collections::BTreeMap;

use crate::reference::Ref;
use crate::variant::Variant;

/// A single instance in the Roblox object hierarchy.
///
/// Each instance has a class, a name, a dictionary of properties, and a list of child references.
/// The instance itself does not hold strong references to children; parent-child relationships
/// are managed exclusively by `WeakDom`.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    referent: Ref,
    class: String,
    name: String,
    // BTreeMap over HashMap: deterministic iteration order, useful for
    // snapshot-style tests and the future CLI output.
    properties: BTreeMap<String, Variant>,
    children: Vec<Ref>,
}

impl Instance {
    pub fn new(referent: Ref, class: impl Into<String>, name: impl Into<String>) -> Self {
        Instance {
            referent,
            class: class.into(),
            name: name.into(),
            properties: BTreeMap::new(),
            children: Vec::new(),
        }
    }

    pub fn referent(&self) -> Ref {
        self.referent
    }

    pub fn class(&self) -> &str {
        &self.class
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    // Untracked, like `set_name`; `WeakDom::set_class` is the tracked path.
    pub(crate) fn set_class(&mut self, class: &str) {
        self.class = class.to_string();
    }

    // Bypasses WeakDom's change log by construction: this type has no reference back
    // to the DOM that owns it. Bulk construction (the binary deserializer) uses this
    // directly; a tracked rename on an instance already in a DOM should go through
    // `WeakDom::set_name` instead.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub fn properties(&self) -> &BTreeMap<String, Variant> {
        &self.properties
    }

    // Same bypass as `set_name`: this is the raw escape hatch bulk construction uses.
    // A tracked write on an instance already in a DOM should go through
    // `WeakDom::set_property` instead.
    pub fn properties_mut(&mut self) -> &mut BTreeMap<String, Variant> {
        &mut self.properties
    }

    /// The instance's attributes, decoded from the `AttributesSerialize` blob
    /// the property map holds verbatim. Decoded per call rather than cached:
    /// only style sheets read it, and only once per plan.
    pub fn attributes(&self) -> BTreeMap<String, Variant> {
        crate::attributes::decode(self.properties.get("AttributesSerialize"))
    }

    /// The instance's `CollectionService` tags.
    pub fn tags(&self) -> Vec<&str> {
        crate::attributes::tags(self.properties.get("Tags"))
    }

    pub fn children(&self) -> &[Ref] {
        &self.children
    }

    // Mutated only by WeakDom::set_parent, which is the sole place allowed
    // to change the tree topology.
    pub(crate) fn children_mut(&mut self) -> &mut Vec<Ref> {
        &mut self.children
    }
}
