//! A tree of changes between two DOMs, in the shape the editor's diff
//! window draws.
//!
//! These types live here (not in the editor) so that [`diff`] — a pure
//! comparison of two [`WeakDom`]s — can produce them without any editor
//! code. The Argon review path builds the same [`DiffNode`]s its own way;
//! this module adds a second producer that compares two arbitrary DOMs,
//! for the team-presence "View changes" (where the place now differs from
//! the snapshot a reconnecting user last saw).
//!
//! Instances are matched by name within each parent, in child order: the
//! i-th child named `X` under a parent pairs with the i-th child named `X`
//! under the matching parent on the other side. A rename therefore reads as
//! a removal plus an addition, the same ambiguity [`crate`]'s path handling
//! already carries for duplicate names.

use std::collections::BTreeMap;

use crate::{Ref, Variant, WeakDom};

/// What one node of a diff is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Added,
    Updated,
    Removed,
}

/// One property of a change: the value on each side. `None` means "no such
/// value": an addition has no before, a removal no after.
#[derive(Debug, Clone)]
pub struct PropertyChange {
    pub name: String,
    pub before: Option<Variant>,
    pub after: Option<Variant>,
}

/// A script's source on each side; a `None` side is a script that didn't
/// exist (an addition) or won't (a removal).
#[derive(Debug, Clone)]
pub struct SourceChange {
    pub old: Option<String>,
    pub new: Option<String>,
}

/// One change, with an addition's or removal's subtree counted under it.
#[derive(Debug, Clone)]
pub struct DiffNode {
    /// Stable within one diff: nodes are numbered in list order.
    pub id: usize,
    pub kind: ChangeKind,
    pub name: String,
    pub class: String,
    /// The parent's full name, `game` for a service.
    pub path: String,
    /// Changed properties, by name. `Source` never appears here; it is
    /// [`DiffNode::source`].
    pub properties: Vec<PropertyChange>,
    pub source: Option<SourceChange>,
    /// An addition's own subtree, in order.
    pub children: Vec<DiffNode>,
    /// Every descendant, however deep.
    pub nested: usize,
    /// Descendants by class, most first, then by name.
    pub contents: Vec<(String, usize)>,
}

impl DiffNode {
    pub fn full_path(&self) -> String {
        if self.path == "game" {
            self.name.clone()
        } else {
            format!("{}.{}", self.path, self.name)
        }
    }
}

/// A `LuaSourceContainer`: the classes whose `Source` is a script's code
/// rather than an ordinary property.
fn is_script_class(class: &str) -> bool {
    matches!(class, "Script" | "LocalScript" | "ModuleScript")
}

/// Numbers nodes in list order as the tree is built.
struct Ids(usize);

impl Ids {
    fn next(&mut self) -> usize {
        let id = self.0;
        self.0 += 1;
        id
    }
}

/// The full path a child at `parent_path` with `name` would have.
fn join(parent_path: &str, name: &str) -> String {
    if parent_path == "game" {
        name.to_owned()
    } else {
        format!("{parent_path}.{name}")
    }
}

fn string_of(value: Option<&Variant>) -> Option<String> {
    match value {
        Some(Variant::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn children_of(dom: &WeakDom, referent: Ref) -> &[Ref] {
    dom.get(referent).map(|i| i.children()).unwrap_or(&[])
}

/// An added or removed instance, to turn into a [`DiffNode`] once ids are
/// assigned in the right order.
struct Pending {
    referent: Ref,
    path: String,
}

/// A paired instance whose own properties, source or class differ.
struct PendingUpdate {
    name: String,
    class: String,
    path: String,
    properties: Vec<PropertyChange>,
    source: Option<SourceChange>,
}

/// The changes from `before` to `after`, as additions (new subtrees),
/// updates (a paired instance whose own properties or source changed) and
/// removals (gone subtrees). Additions and removals carry their subtree's
/// counts; a paired instance's changed descendants are their own nodes.
/// Nodes are numbered additions first, then updates, then removals, so the
/// ids are stable across identical inputs.
pub fn diff(before: &WeakDom, after: &WeakDom) -> Vec<DiffNode> {
    let mut added = Vec::new();
    let mut updated = Vec::new();
    let mut removed = Vec::new();
    classify(
        before,
        after,
        before.root_refs(),
        after.root_refs(),
        "game",
        &mut added,
        &mut updated,
        &mut removed,
    );

    let mut ids = Ids(0);
    let mut nodes = Vec::new();
    for item in added {
        nodes.push(added_node(after, item.referent, item.path, &mut ids));
    }
    for update in updated {
        nodes.push(DiffNode {
            id: ids.next(),
            kind: ChangeKind::Updated,
            name: update.name,
            class: update.class,
            path: update.path,
            properties: update.properties,
            source: update.source,
            children: Vec::new(),
            nested: 0,
            contents: Vec::new(),
        });
    }
    for item in removed {
        nodes.push(removed_node(before, item.referent, item.path, &mut ids));
    }
    nodes
}

#[allow(clippy::too_many_arguments)]
fn classify(
    before: &WeakDom,
    after: &WeakDom,
    before_children: &[Ref],
    after_children: &[Ref],
    path: &str,
    added: &mut Vec<Pending>,
    updated: &mut Vec<PendingUpdate>,
    removed: &mut Vec<Pending>,
) {
    let mut consumed = vec![false; before_children.len()];
    for &a in after_children {
        let a_name = after.get(a).map(|i| i.name());
        let matched = before_children
            .iter()
            .enumerate()
            .find(|&(i, &b)| !consumed[i] && before.get(b).map(|i| i.name()) == a_name);
        match matched {
            Some((i, &b)) => {
                consumed[i] = true;
                if let Some(update) = updated_of(before, after, b, a, path) {
                    updated.push(update);
                }
                let child_path = join(path, a_name.unwrap_or_default());
                classify(
                    before,
                    after,
                    children_of(before, b),
                    children_of(after, a),
                    &child_path,
                    added,
                    updated,
                    removed,
                );
            }
            None => added.push(Pending {
                referent: a,
                path: path.to_owned(),
            }),
        }
    }
    for (i, &b) in before_children.iter().enumerate() {
        if !consumed[i] {
            removed.push(Pending {
                referent: b,
                path: path.to_owned(),
            });
        }
    }
}

/// The update for a paired instance, or `None` when nothing of its own
/// changed (its descendants are handled separately).
fn updated_of(
    before: &WeakDom,
    after: &WeakDom,
    b: Ref,
    a: Ref,
    path: &str,
) -> Option<PendingUpdate> {
    let (bi, ai) = (before.get(b)?, after.get(a)?);
    let class = ai.class().to_owned();
    let is_script = is_script_class(&class) || is_script_class(bi.class());
    let mut properties = Vec::new();
    let mut source = None;
    let keys: std::collections::BTreeSet<&str> = bi
        .properties()
        .keys()
        .chain(ai.properties().keys())
        .map(String::as_str)
        .collect();
    for key in keys {
        let before_v = bi.properties().get(key);
        let after_v = ai.properties().get(key);
        if key == "Source" && is_script {
            let (old, new) = (string_of(before_v), string_of(after_v));
            if old != new {
                source = Some(SourceChange { old, new });
            }
            continue;
        }
        if before_v != after_v {
            properties.push(PropertyChange {
                name: key.to_owned(),
                before: before_v.cloned(),
                after: after_v.cloned(),
            });
        }
    }
    let class_changed = bi.class() != ai.class();
    if properties.is_empty() && source.is_none() && !class_changed {
        return None;
    }
    Some(PendingUpdate {
        name: ai.name().to_owned(),
        class,
        path: path.to_owned(),
        properties,
        source,
    })
}

/// An addition and its subtree, numbered depth-first.
fn added_node(dom: &WeakDom, referent: Ref, path: String, ids: &mut Ids) -> DiffNode {
    let id = ids.next();
    let Some(instance) = dom.get(referent) else {
        return DiffNode {
            id,
            kind: ChangeKind::Added,
            name: String::new(),
            class: String::new(),
            path,
            properties: Vec::new(),
            source: None,
            children: Vec::new(),
            nested: 0,
            contents: Vec::new(),
        };
    };
    let class = instance.class().to_owned();
    let is_script = is_script_class(&class);
    let mut properties = Vec::new();
    let mut source = None;
    for (key, value) in instance.properties() {
        if key == "Source" && is_script {
            source = Some(SourceChange {
                old: None,
                new: string_of(Some(value)),
            });
            continue;
        }
        properties.push(PropertyChange {
            name: key.clone(),
            before: None,
            after: Some(value.clone()),
        });
    }
    properties.sort_by(|x, y| x.name.cmp(&y.name));
    let full = join(&path, instance.name());
    let children: Vec<DiffNode> = instance
        .children()
        .iter()
        .map(|&child| added_node(dom, child, full.clone(), ids))
        .collect();
    let mut counts = BTreeMap::new();
    count_descendants(dom, referent, &mut counts);
    DiffNode {
        id,
        kind: ChangeKind::Added,
        name: instance.name().to_owned(),
        class,
        path,
        properties,
        source,
        children,
        nested: counts.values().sum(),
        contents: sorted_contents(counts),
    }
}

fn removed_node(dom: &WeakDom, referent: Ref, path: String, ids: &mut Ids) -> DiffNode {
    let id = ids.next();
    let instance = dom.get(referent);
    let (name, class) = instance
        .map(|i| (i.name().to_owned(), i.class().to_owned()))
        .unwrap_or_default();
    let source = is_script_class(&class).then(|| SourceChange {
        old: instance.and_then(|i| string_of(i.properties().get("Source"))),
        new: None,
    });
    let mut counts = BTreeMap::new();
    count_descendants(dom, referent, &mut counts);
    DiffNode {
        id,
        kind: ChangeKind::Removed,
        name,
        class,
        path,
        properties: Vec::new(),
        source,
        children: Vec::new(),
        nested: counts.values().sum(),
        contents: sorted_contents(counts),
    }
}

/// Tallies every descendant of `referent` (not itself) by class.
fn count_descendants(dom: &WeakDom, referent: Ref, counts: &mut BTreeMap<String, usize>) {
    for &child in children_of(dom, referent) {
        if let Some(instance) = dom.get(child) {
            *counts.entry(instance.class().to_owned()).or_default() += 1;
        }
        count_descendants(dom, child, counts);
    }
}

/// Most numerous first, then by name.
fn sorted_contents(counts: BTreeMap<String, usize>) -> Vec<(String, usize)> {
    let mut contents: Vec<(String, usize)> = counts.into_iter().collect();
    contents.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    contents
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(nodes: &[DiffNode]) -> Vec<(ChangeKind, &str)> {
        nodes.iter().map(|n| (n.kind, n.name.as_str())).collect()
    }

    #[test]
    fn identical_doms_have_no_changes() {
        let mut dom = WeakDom::new();
        let ws = dom.new_instance("Workspace", "Workspace", None);
        dom.new_instance("Part", "Part", Some(ws));
        let clone = WeakDom::new(); // fresh; rebuild same shape
        let mut after = clone;
        let ws2 = after.new_instance("Workspace", "Workspace", None);
        after.new_instance("Part", "Part", Some(ws2));
        assert!(diff(&dom, &after).is_empty());
    }

    #[test]
    fn detects_added_removed_and_changed() {
        let mut before = WeakDom::new();
        let bws = before.new_instance("Workspace", "Workspace", None);
        let bpart = before.new_instance("Part", "Part", Some(bws));
        before
            .get_mut(bpart)
            .unwrap()
            .properties_mut()
            .insert("Transparency".into(), Variant::Float32(0.0));
        before.new_instance("Folder", "Gone", Some(bws));

        let mut after = WeakDom::new();
        let aws = after.new_instance("Workspace", "Workspace", None);
        let apart = after.new_instance("Part", "Part", Some(aws));
        after
            .get_mut(apart)
            .unwrap()
            .properties_mut()
            .insert("Transparency".into(), Variant::Float32(0.5));
        after.new_instance("Model", "New", Some(aws));

        let nodes = diff(&before, &after);
        // Addition first, then update, then removal.
        assert_eq!(
            kinds(&nodes),
            vec![
                (ChangeKind::Added, "New"),
                (ChangeKind::Updated, "Part"),
                (ChangeKind::Removed, "Gone"),
            ]
        );
        let update = &nodes[1];
        assert_eq!(update.path, "Workspace");
        assert_eq!(update.properties.len(), 1);
        assert_eq!(update.properties[0].name, "Transparency");
        assert_eq!(update.properties[0].before, Some(Variant::Float32(0.0)));
        assert_eq!(update.properties[0].after, Some(Variant::Float32(0.5)));
    }

    #[test]
    fn script_source_is_a_source_change() {
        let mut before = WeakDom::new();
        let bs = before.new_instance("Script", "Script", None);
        before
            .get_mut(bs)
            .unwrap()
            .properties_mut()
            .insert("Source".into(), Variant::String("print(1)".into()));
        let mut after = WeakDom::new();
        let as_ = after.new_instance("Script", "Script", None);
        after
            .get_mut(as_)
            .unwrap()
            .properties_mut()
            .insert("Source".into(), Variant::String("print(2)".into()));

        let nodes = diff(&before, &after);
        assert_eq!(nodes.len(), 1);
        let source = nodes[0].source.as_ref().unwrap();
        assert_eq!(source.old.as_deref(), Some("print(1)"));
        assert_eq!(source.new.as_deref(), Some("print(2)"));
        assert!(nodes[0].properties.is_empty());
    }

    #[test]
    fn added_subtree_is_counted() {
        let before = WeakDom::new();
        let mut after = WeakDom::new();
        let model = after.new_instance("Model", "Model", None);
        let part = after.new_instance("Part", "Part", Some(model));
        after.new_instance("Decal", "Decal", Some(part));

        let nodes = diff(&before, &after);
        assert_eq!(nodes.len(), 1);
        let added = &nodes[0];
        assert_eq!(added.name, "Model");
        assert_eq!(added.nested, 2);
        assert_eq!(added.children.len(), 1);
        assert_eq!(added.children[0].name, "Part");
    }

    #[test]
    fn same_named_siblings_pair_in_order() {
        let mut before = WeakDom::new();
        let bws = before.new_instance("Workspace", "Workspace", None);
        before.new_instance("Part", "Part", Some(bws));
        let b2 = before.new_instance("Part", "Part", Some(bws));
        before
            .get_mut(b2)
            .unwrap()
            .properties_mut()
            .insert("Anchored".into(), Variant::Bool(false));

        let mut after = WeakDom::new();
        let aws = after.new_instance("Workspace", "Workspace", None);
        after.new_instance("Part", "Part", Some(aws));
        let a2 = after.new_instance("Part", "Part", Some(aws));
        after
            .get_mut(a2)
            .unwrap()
            .properties_mut()
            .insert("Anchored".into(), Variant::Bool(true));

        // Only the second Part changed; the first pairs cleanly.
        let nodes = diff(&before, &after);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, ChangeKind::Updated);
        assert_eq!(nodes[0].properties[0].name, "Anchored");
    }
}
