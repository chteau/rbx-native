//! The initial sync: what happens to the server's first snapshot, ported
//! from the plugin's processor (`argon-rbx/argon-roblox@30fd38d`,
//! `src/Core/Processor/init.luau`).
//!
//! The plugin never replaces Studio's tree with the snapshot. It first
//! *hydrates*: walks the two trees together and pairs each snapshot node
//! with the first not-yet-paired instance of the same name and class
//! (`:102-117`). Then it *diffs* the paired trees into additions, updates
//! and removals (`:119-221`), and what happens to that diff is the
//! Initial Sync Priority setting: "Server" applies it here, "Client"
//! reverses it and sends it to the server (`:223-243`), "None" drops it
//! and keeps only the pairing (`:44-48`).
//!
//! Everything here is pure over a `WeakDom` and the id tables, so the
//! rules can be tested without a window.

use std::collections::HashMap;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::argon_client::{self, ArgonRef, Changes, Snapshot, UpdatedSnapshot};

/// The class every script inherits from — what the plugin's
/// `isScriptRelated` and `shouldSyncProperties` test with `IsA`.
const SOURCE_CONTAINER: &str = "LuaSourceContainer";

/// Which side an initial diff favours: the plugin's `InitialSyncPriority`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Priority {
    /// The snapshot is applied to this DOM.
    Server,
    /// The reversed diff is written to the server: this DOM wins.
    Client,
    /// Nothing moves either way; only the pairing is kept.
    None,
}

/// The settings the initial diff reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Rules {
    pub(super) priority: Priority,
    /// `KeepUnknowns`: an instance the snapshot doesn't have survives.
    pub(super) keep_unknowns: bool,
    /// `OverridePackages`: server changes may write under a `PackageLink`.
    pub(super) override_packages: bool,
    /// `SyncbackProperties`: with Client priority, whether non-script
    /// instances get their properties compared at all.
    pub(super) syncback_properties: bool,
}

/// The two-way id table the diff reads and extends.
pub(super) struct Ids<'a> {
    pub(super) ids: &'a mut HashMap<ArgonRef, Ref>,
    pub(super) ids_rev: &'a mut HashMap<Ref, ArgonRef>,
}

impl Ids<'_> {
    fn pair(&mut self, id: ArgonRef, referent: Ref) {
        self.ids.insert(id, referent);
        self.ids_rev.insert(referent, id);
    }
}

/// Pairs the snapshot's nodes with this DOM's instances, first unpaired
/// instance of the same name and class wins (`Processor:hydrate`,
/// `init.luau:102-117`). The snapshot's root stands for the DataModel, so
/// its children pair with the DOM's root instances.
pub(super) fn hydrate(dom: &WeakDom, root: &Snapshot, ids: &mut Ids<'_>) {
    hydrate_children(dom, &root.children, dom.root_refs(), ids);
}

fn hydrate_children(dom: &WeakDom, nodes: &[Snapshot], candidates: &[Ref], ids: &mut Ids<'_>) {
    let mut taken = vec![false; candidates.len()];
    for node in nodes {
        let found = candidates.iter().enumerate().find(|(index, referent)| {
            !taken[*index]
                && dom.get(**referent).is_some_and(|instance| {
                    instance.name() == node.name && instance.class() == node.class
                })
        });
        if let Some((index, &referent)) = found {
            taken[index] = true;
            ids.pair(node.id, referent);
            if let Some(instance) = dom.get(referent) {
                hydrate_children(dom, &node.children, instance.children(), ids);
            }
        }
    }
}

/// The diff between the paired trees (`Processor:diff`, `init.luau:119-221`),
/// plus the package filter Server priority applies afterwards
/// (`:59-82`). An instance the snapshot doesn't know and that is going to
/// be removed gets an id of its own here, so the removal can travel
/// through the same batch shape as a live one and, under Client priority,
/// be turned back into an addition the server can take.
pub(super) fn diff(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    root: &Snapshot,
    rules: Rules,
    ids: &mut Ids<'_>,
) -> Changes {
    let client = rules.priority == Priority::Client;
    let mut changes = Changes {
        additions: Vec::new(),
        updates: Vec::new(),
        removals: Vec::new(),
    };
    for node in &root.children {
        diff_node(
            dom,
            database,
            node,
            ArgonRef::ROOT,
            rules,
            client,
            ids,
            &mut changes,
        );
    }
    // Instances at the root that no snapshot node claimed: services, so
    // never creatable, so never removed — the same outcome the plugin
    // reaches through `Dom.isCreatable` at `:214`.
    if rules.priority == Priority::Server && !rules.override_packages {
        changes.additions.retain(|addition| {
            !addition
                .parent
                .and_then(|parent| ids.ids.get(&parent))
                .is_some_and(|&parent| is_package_descendant(dom, parent))
        });
        changes.updates.retain(|update| {
            !ids.ids
                .get(&update.id)
                .is_some_and(|&referent| is_package_descendant(dom, referent))
        });
    }
    changes
}

#[allow(clippy::too_many_arguments)]
fn diff_node(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    node: &Snapshot,
    parent: ArgonRef,
    rules: Rules,
    client: bool,
    ids: &mut Ids<'_>,
    changes: &mut Changes,
) {
    let Some(&referent) = ids.ids.get(&node.id) else {
        changes.additions.push(with_parent(node, parent));
        return;
    };
    let Some(instance) = dom.get(referent) else {
        return;
    };

    // Properties (`:126-171`): a snapshot value that differs from the
    // instance is an update; a known property the snapshot doesn't carry
    // is meant to be at its default, so an instance that strayed from it
    // is updated back. With Client priority only scripts are compared,
    // unless Syncback Properties says every instance is (`:127`,
    // `Read.luau:249-258`).
    if !client || should_sync_properties(database, instance.class(), rules) {
        let mut updated: Vec<(String, rmpv::Value)> = Vec::new();
        let mut keys: Vec<&str> = instance.properties().keys().map(String::as_str).collect();
        keys.extend(node.properties.iter().map(|(name, _)| name.as_str()));
        keys.sort_unstable();
        keys.dedup();
        for key in keys {
            let current = instance
                .properties()
                .get(key)
                .or_else(|| database.default_value(instance.class(), key));
            match node.properties.iter().find(|(name, _)| name == key) {
                Some((_, encoded)) => {
                    let Some(wanted) = argon_client::decode_value(encoded) else {
                        continue;
                    };
                    if current != Some(&wanted) {
                        updated.push((key.to_owned(), encoded.clone()));
                    }
                }
                None => {
                    let Some(default) = database.default_value(instance.class(), key) else {
                        continue;
                    };
                    if current != Some(default) {
                        if let Some(encoded) = argon_client::encode_value(default) {
                            updated.push((key.to_owned(), encoded));
                        }
                    }
                }
            }
        }
        if !updated.is_empty() {
            changes.updates.push(UpdatedSnapshot {
                id: node.id,
                name: None,
                class: None,
                properties: Some(updated),
            });
        }
    }

    // Snapshot children with no pair are additions (`:174-180`).
    for child in &node.children {
        if !ids.ids.contains_key(&child.id) {
            changes.additions.push(with_parent(child, node.id));
        }
    }

    // Instance children: paired ones recurse; the rest are unknown to the
    // server and go unless something keeps them (`:182-218`).
    for &child in instance.children() {
        match ids.ids_rev.get(&child).copied() {
            Some(child_id) => {
                if let Some(child_node) = node.children.iter().find(|n| n.id == child_id) {
                    diff_node(
                        dom, database, child_node, node.id, rules, client, ids, changes,
                    );
                }
            }
            None => {
                let Some(unknown) = dom.get(child) else {
                    continue;
                };
                let kept = node.keep_unknowns || rules.keep_unknowns;
                if (client || !kept) && database.is_creatable(unknown.class()) {
                    let id = ArgonRef::generate();
                    ids.pair(id, child);
                    changes.removals.push(id);
                }
            }
        }
    }
}

fn should_sync_properties(database: &ReflectionDatabase, class: &str, rules: Rules) -> bool {
    database.is_subclass_of(class, SOURCE_CONTAINER) || rules.syncback_properties
}

/// The plugin's `isPackageDescendant` (`init.luau:17-27`): the instance,
/// or any ancestor, has a `PackageLink` child.
fn is_package_descendant(dom: &WeakDom, referent: Ref) -> bool {
    let mut current = Some(referent);
    while let Some(referent) = current {
        let Some(instance) = dom.get(referent) else {
            return false;
        };
        if instance
            .children()
            .iter()
            .any(|&child| dom.get(child).is_some_and(|c| c.class() == "PackageLink"))
        {
            return true;
        }
        current = dom.parent(referent);
    }
    false
}

fn with_parent(node: &Snapshot, parent: ArgonRef) -> Snapshot {
    Snapshot {
        id: node.id,
        parent: Some(parent),
        name: node.name.clone(),
        class: node.class.clone(),
        properties: node.properties.clone(),
        children: node
            .children
            .iter()
            .map(|child| with_parent(child, node.id))
            .collect(),
        keep_unknowns: node.keep_unknowns,
    }
}

/// Client priority: the diff turned around so the server ends up matching
/// this DOM (`Processor:reverseChanges`, `init.luau:223-243`). Additions
/// become removals of the same ids; updates re-read the instance as it
/// stands; removals become additions of the instance's whole subtree.
pub(super) fn reverse(dom: &WeakDom, changes: &Changes, ids: &mut Ids<'_>) -> Changes {
    let removals = changes
        .additions
        .iter()
        .map(|addition| addition.id)
        .collect();
    let updates = changes
        .updates
        .iter()
        .filter_map(|update| {
            let &referent = ids.ids.get(&update.id)?;
            let instance = dom.get(referent)?;
            Some(UpdatedSnapshot {
                id: update.id,
                name: Some(instance.name().to_owned()),
                class: Some(instance.class().to_owned()),
                properties: Some(encode_properties(instance.properties())),
            })
        })
        .collect();
    let additions = changes
        .removals
        .iter()
        .filter_map(|&id| {
            let &referent = ids.ids.get(&id)?;
            let parent = dom
                .parent(referent)
                .and_then(|parent| ids.ids_rev.get(&parent).copied())
                .unwrap_or(ArgonRef::ROOT);
            snapshot_of(dom, referent, id, parent, ids)
        })
        .collect();
    Changes {
        additions,
        updates,
        removals,
    }
}

/// An instance and its subtree as the server would receive them, every
/// node given an id of its own (`Read.luau:onAdd`).
fn snapshot_of(
    dom: &WeakDom,
    referent: Ref,
    id: ArgonRef,
    parent: ArgonRef,
    ids: &mut Ids<'_>,
) -> Option<Snapshot> {
    let instance = dom.get(referent)?;
    ids.pair(id, referent);
    let children = instance
        .children()
        .iter()
        .filter_map(|&child| snapshot_of(dom, child, ArgonRef::generate(), id, ids))
        .collect();
    Some(Snapshot {
        id,
        parent: Some(parent),
        name: instance.name().to_owned(),
        class: instance.class().to_owned(),
        properties: encode_properties(instance.properties()),
        children,
        keep_unknowns: false,
    })
}

fn encode_properties(
    properties: &std::collections::BTreeMap<String, Variant>,
) -> Vec<(String, rmpv::Value)> {
    properties
        .iter()
        .filter_map(|(name, variant)| {
            argon_client::encode_value(variant).map(|v| (name.clone(), v))
        })
        .collect()
}

#[cfg(test)]
mod tests;
