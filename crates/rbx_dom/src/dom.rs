//! Weak reference-based instance tree.

use std::collections::HashMap;

use crate::change::Change;
use crate::error::DomError;
use crate::instance::Instance;
use crate::reference::Ref;
use crate::variant::Variant;

/// A tree of instances using weak references (u32 IDs).
///
/// Instances can be at the root level or parented to another instance. The DOM
/// maintains both forward (children) and reverse (parent) edges.
#[derive(Debug, Clone, Default)]
pub struct WeakDom {
    instances: HashMap<Ref, Instance>,
    root_refs: Vec<Ref>,
    // Reverse lookup of the tree edges: Instance itself only stores children,
    // so this is what lets set_parent detach a child from its current parent
    // (or from root_refs) before reattaching it.
    parents: HashMap<Ref, Ref>,
    // High-water mark for `new_instance`'s allocator. Bumped by `insert` too, so a
    // Ref freshly allocated here never collides with one loaded from a file: every
    // referent that ever entered the DOM, however it got in, raises this floor.
    next_ref: u32,
    changes: Vec<Change>,
}

impl WeakDom {
    pub fn new() -> Self {
        WeakDom::default()
    }

    pub fn insert(&mut self, instance: Instance) {
        let referent = instance.referent();
        self.next_ref = self.next_ref.max(referent.value().saturating_add(1));
        self.instances.insert(referent, instance);
        self.root_refs.push(referent);
        self.changes.push(Change::Added(referent));
    }

    /// Allocates a brand new instance, unique against every referent this DOM has
    /// ever held (including ones loaded from a file), and inserts it. If `parent`
    /// is `Some`, the instance is parented there instead of staying at the root.
    pub fn new_instance(&mut self, class: &str, name: &str, parent: Option<Ref>) -> Ref {
        let referent = Ref::new(self.next_ref);
        self.next_ref = self.next_ref.saturating_add(1);
        self.insert(Instance::new(referent, class, name));
        if let Some(parent_ref) = parent {
            self.set_parent(referent, Some(parent_ref));
        }
        referent
    }

    /// Overwrites a single property, returning whatever value it held before (if any).
    ///
    /// `value` is stored as-is: this crate has no `ReflectionDatabase`, so it cannot
    /// check that `value`'s variant matches what `name` is supposed to hold on this
    /// class. Callers that have reflection data should validate before calling this.
    pub fn set_property(
        &mut self,
        referent: Ref,
        name: &str,
        value: Variant,
    ) -> Result<Option<Variant>, DomError> {
        let instance = self
            .instances
            .get_mut(&referent)
            .ok_or(DomError::UnknownInstance(referent))?;
        let old = instance.properties_mut().insert(name.to_string(), value);
        self.changes.push(Change::Property {
            referent,
            name: name.to_string(),
        });
        Ok(old)
    }

    /// Renames an instance, returning its previous name. Tracked the same way
    /// `set_property` is: see `Change::Property`'s doc comment for why a rename is
    /// reported under that variant instead of a dedicated one.
    pub fn set_name(&mut self, referent: Ref, name: &str) -> Result<String, DomError> {
        let instance = self
            .instances
            .get_mut(&referent)
            .ok_or(DomError::UnknownInstance(referent))?;
        let old = instance.name().to_string();
        instance.set_name(name);
        self.changes.push(Change::Property {
            referent,
            name: "Name".to_string(),
        });
        Ok(old)
    }

    /// Removes an instance and its whole subtree, detaching it from its parent (or
    /// from the root list) first. Returns every removed referent, root included; the
    /// order within that list is unspecified beyond that guarantee.
    pub fn remove(&mut self, referent: Ref) -> Vec<Ref> {
        if !self.instances.contains_key(&referent) {
            return Vec::new();
        }
        self.detach(referent);

        let mut removed = Vec::new();
        let mut stack = vec![referent];
        while let Some(current) = stack.pop() {
            if let Some(instance) = self.instances.remove(&current) {
                stack.extend_from_slice(instance.children());
                self.parents.remove(&current);
                removed.push(current);
                self.changes.push(Change::Removed(current));
            }
        }
        removed
    }

    /// Drains and returns every change recorded since the last call, in the order
    /// the mutations happened.
    pub fn take_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.changes)
    }

    pub fn get(&self, referent: Ref) -> Option<&Instance> {
        self.instances.get(&referent)
    }

    // Raw escape hatch: edits made through the returned `&mut Instance` (e.g. via
    // `properties_mut`/`set_name`) bypass the change log, since `Instance` has no
    // link back to it. Prefer `set_property`/`set_name` for tracked live edits.
    pub fn get_mut(&mut self, referent: Ref) -> Option<&mut Instance> {
        self.instances.get_mut(&referent)
    }

    pub fn root_refs(&self) -> &[Ref] {
        &self.root_refs
    }

    /// The instance `referent` currently hangs under, or `None` when it is a
    /// root (or absent entirely).
    ///
    /// Reads the reverse edge `set_parent` already maintains, so walking up a
    /// chain costs its depth rather than a search of the whole tree — which is
    /// what makes an "is this instance an ancestor of that one" check cheap
    /// enough for a caller to run per frame (the Explorer's drag-and-drop
    /// refuses a drop into the dragged instance's own subtree that way).
    pub fn parent(&self, referent: Ref) -> Option<Ref> {
        self.parents.get(&referent).copied()
    }

    /// Moves `child` to a new parent, detaching it from its current parent if any.
    ///
    /// If `parent` is `None`, the child becomes a root instance.
    pub fn set_parent(&mut self, child: Ref, parent: Option<Ref>) {
        let old = self.parents.get(&child).copied();
        self.detach(child);

        match parent {
            Some(parent_ref) => {
                self.parents.insert(child, parent_ref);
                if let Some(parent_instance) = self.instances.get_mut(&parent_ref) {
                    parent_instance.children_mut().push(child);
                }
            }
            None => self.root_refs.push(child),
        }

        self.changes.push(Change::Parent {
            referent: child,
            old,
            new: parent,
        });
    }

    fn detach(&mut self, child: Ref) {
        match self.parents.remove(&child) {
            Some(old_parent) => {
                if let Some(parent_instance) = self.instances.get_mut(&old_parent) {
                    parent_instance.children_mut().retain(|&r| r != child);
                }
            }
            None => self.root_refs.retain(|&r| r != child),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_starts_as_root() {
        let mut dom = WeakDom::new();
        let r = Ref::new(1);
        dom.insert(Instance::new(r, "Part", "Part"));

        assert_eq!(dom.root_refs(), &[r]);
        assert!(dom.get(r).is_some());
    }

    #[test]
    fn set_parent_moves_child_out_of_roots() {
        let mut dom = WeakDom::new();
        let parent = Ref::new(1);
        let child = Ref::new(2);
        dom.insert(Instance::new(parent, "Folder", "Folder"));
        dom.insert(Instance::new(child, "Part", "Part"));

        dom.set_parent(child, Some(parent));

        assert_eq!(dom.root_refs(), &[parent]);
        assert_eq!(dom.get(parent).unwrap().children(), &[child]);
    }

    #[test]
    fn set_parent_none_moves_child_back_to_roots() {
        let mut dom = WeakDom::new();
        let parent = Ref::new(1);
        let child = Ref::new(2);
        dom.insert(Instance::new(parent, "Folder", "Folder"));
        dom.insert(Instance::new(child, "Part", "Part"));
        dom.set_parent(child, Some(parent));

        dom.set_parent(child, None);

        assert!(dom.get(parent).unwrap().children().is_empty());
        assert!(dom.root_refs().contains(&child));
    }

    #[test]
    fn reparenting_removes_from_previous_parent() {
        let mut dom = WeakDom::new();
        let a = Ref::new(1);
        let b = Ref::new(2);
        let child = Ref::new(3);
        dom.insert(Instance::new(a, "Folder", "A"));
        dom.insert(Instance::new(b, "Folder", "B"));
        dom.insert(Instance::new(child, "Part", "Part"));

        dom.set_parent(child, Some(a));
        dom.set_parent(child, Some(b));

        assert!(dom.get(a).unwrap().children().is_empty());
        assert_eq!(dom.get(b).unwrap().children(), &[child]);
    }

    #[test]
    fn parent_follows_the_reverse_edge_and_is_none_at_the_root() {
        let mut dom = WeakDom::new();
        let folder = Ref::new(1);
        let child = Ref::new(2);
        dom.insert(Instance::new(folder, "Folder", "Folder"));
        dom.insert(Instance::new(child, "Part", "Part"));

        assert_eq!(dom.parent(folder), None);
        assert_eq!(dom.parent(child), None);

        dom.set_parent(child, Some(folder));
        assert_eq!(dom.parent(child), Some(folder));

        dom.set_parent(child, None);
        assert_eq!(dom.parent(child), None);
    }

    #[test]
    fn parent_of_an_unknown_referent_is_none() {
        let dom = WeakDom::new();
        assert_eq!(dom.parent(Ref::new(7)), None);
    }
}
