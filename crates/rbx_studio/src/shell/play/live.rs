//! Studio's Client/Server views during Play: the Explorer and Properties
//! show the running game's tree (the developer's client, or the server) as
//! the probe reports it, and edits made there go to the game in real time.
//!
//! How (see `docs/research/play-sandbox/live-views.md`):
//! - `rbx_play::live::LiveTree` per side, fed by `Uplink::Tree`, `Diff` and
//!   `Props` in `live_apply`.
//! - Showing a view swaps `Shell::dom` with that side's tree
//!   (`std::mem::swap(&mut self.dom, &mut tree.dom)`), stashing the edited
//!   place's undo `history` (a fresh `History` while live) and selection.
//!   While shown, an update is applied by swapping back, applying, swapping
//!   again; the DOM's change log is then drained (`take_changes`) so those
//!   changes are never mistaken for the user's edits.
//! - The 3D viewport keeps showing the edited place (the embedded game
//!   covers it) and never sees the live tree: nothing here calls
//!   `reflect_changes`'s normal path.
//! - `reflect_changes` hands every edit made while live to `live_reflect`,
//!   which turns the change log into `CommandBody`s for the shown side:
//!   `Property` → `Set` (`Name` included; `AttributesSerialize` →
//!   `SetAttribute` per changed/removed attribute; `Tags` is skipped),
//!   `Parent` → `Reparent`, `Removed` → `Destroy`, `Added` → `Create` (and
//!   the local instance is removed again: the probe's own `Diff` adds the
//!   real one with its id). Values are encoded with `rbx_play::value::encode`
//!   against the reflected `value_type`.
//! - `live_tick` (every Play poll) sends `Watch` for the shown side when the
//!   selection changed: each selected instance with the property names the
//!   Properties panel lists for its class (`properties_for_class`).
//! - Stopping restores everything exactly as it was (`live_end`).

use std::collections::{BTreeMap, HashMap, HashSet};

use rbx_dom::{Change, Ref, Variant, WeakDom};
use rbx_play::live::LiveTree;
use rbx_play::protocol::{CommandBody, Side, Uplink, WatchItem};

use gpui_kit::Context;

use super::super::Shell;
use crate::history::{History, DEFAULT_CAP};
use crate::properties::Properties;

/// Which game tree the Explorer shows during Play. Studio opens on Client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum View {
    Client,
    Server,
}

impl View {
    pub(crate) fn side(self) -> Side {
        match self {
            View::Client => Side::Client,
            View::Server => Side::Server,
        }
    }
}

/// The edited place's state while a live tree occupies `Shell::dom`.
struct Stash {
    history: History,
    selection: Vec<Ref>,
    /// The view whose tree is swapped in (its `dom` holds the edited place).
    view: View,
}

/// Last known attributes of one instance, as encoded JSON strings.
type AttributeJson = BTreeMap<String, String>;

pub(crate) struct Live {
    /// The view the editor is showing, or would show once `begin` ran.
    pub(crate) view: View,
    pub(crate) server: LiveTree,
    pub(crate) client: LiveTree,
    stash: Option<Stash>,
    watched: Option<(View, Vec<u64>)>,
    server_attributes: HashMap<u64, AttributeJson>,
    client_attributes: HashMap<u64, AttributeJson>,
}

impl Live {
    pub(crate) fn new() -> Self {
        Live {
            view: View::Client,
            server: LiveTree::new(),
            client: LiveTree::new(),
            stash: None,
            watched: None,
            server_attributes: HashMap::new(),
            client_attributes: HashMap::new(),
        }
    }

    fn tree(&self, view: View) -> &LiveTree {
        match view {
            View::Client => &self.client,
            View::Server => &self.server,
        }
    }

    fn tree_mut(&mut self, view: View) -> &mut LiveTree {
        match view {
            View::Client => &mut self.client,
            View::Server => &mut self.server,
        }
    }

    fn attributes_mut(&mut self, view: View) -> &mut HashMap<u64, AttributeJson> {
        match view {
            View::Client => &mut self.client_attributes,
            View::Server => &mut self.server_attributes,
        }
    }
}

fn view_of(side: Side) -> View {
    match side {
        Side::Client => View::Client,
        Side::Server => View::Server,
    }
}

fn attribute_json(attributes: &BTreeMap<String, Variant>) -> AttributeJson {
    attributes
        .iter()
        .filter_map(|(name, value)| {
            rbx_play::value::encode_attribute(value).map(|json| (name.clone(), json.to_string()))
        })
        .collect()
}

/// The property names the Properties panel would list for `class`: every
/// property of the class and its superclasses in `db` that is not hidden or
/// deprecated, nearest class first, without duplicates.
pub(crate) fn properties_for_class(
    db: &rbx_reflection::ReflectionDatabase,
    class: &str,
) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    let mut visited = HashSet::new();
    let mut current = db.class(class);
    while let Some(descriptor) = current {
        if !visited.insert(descriptor.name.clone()) {
            break;
        }
        for property in &descriptor.properties {
            if property.is_hidden() || property.is_deprecated() {
                continue;
            }
            if seen.insert(property.name.clone()) {
                names.push(property.name.clone());
            }
        }
        current = descriptor.superclass.as_deref().and_then(|s| db.class(s));
    }
    names
}

impl Shell {
    /// A live tree is swapped into `self.dom` right now.
    pub(in crate::shell) fn live_shown(&self) -> bool {
        self.play.live.stash.is_some()
    }

    /// The edited place: `self.dom`, or the stashed one while live.
    pub(in crate::shell) fn edit_dom(&self) -> &WeakDom {
        match &self.play.live.stash {
            Some(stash) => &self.play.live.tree(stash.view).dom,
            None => &self.dom,
        }
    }

    fn live_swap(&mut self, view: View) {
        let tree = self.play.live.tree_mut(view);
        std::mem::swap(&mut self.dom, &mut tree.dom);
    }

    fn live_refresh(&mut self, cx: &mut Context<Self>) {
        self.dom.take_changes();
        self.properties = Properties::new(self.database.clone());
        self.rebuild_explorer(cx);
        cx.notify();
    }

    /// Play became Running: show `self.play.live.view` (Client by default).
    pub(in crate::shell) fn live_begin(&mut self, cx: &mut Context<Self>) {
        if self.live_shown() {
            return;
        }
        let history = std::mem::replace(&mut self.history, History::new(DEFAULT_CAP));
        let selection = self.selected_all().to_vec();
        self.deselect(cx);
        // Like Studio, every test opens on the tester's client.
        self.play.live.view = View::Client;
        let view = self.play.live.view;
        self.dom.take_changes();
        self.live_swap(view);
        self.play.live.stash = Some(Stash {
            history,
            selection,
            view,
        });
        self.play.live.watched = None;
        self.live_refresh(cx);
    }

    /// Play stopped: swap the edited place back with its history and
    /// selection, rebuild the Explorer, reset the Properties cache
    /// (`Properties::new` or equivalent), clear both live trees.
    pub(in crate::shell) fn live_end(&mut self, cx: &mut Context<Self>) {
        let Some(stash) = self.play.live.stash.take() else {
            return;
        };
        self.deselect(cx);
        self.live_swap(stash.view);
        self.dom.take_changes();
        self.history = stash.history;
        self.properties = Properties::new(self.database.clone());
        self.rebuild_explorer(cx);
        self.reselect(stash.selection, cx);
        let live = &mut self.play.live;
        live.server.clear();
        live.client.clear();
        live.server.dom.take_changes();
        live.client.dom.take_changes();
        live.server_attributes.clear();
        live.client_attributes.clear();
        live.watched = None;
        cx.notify();
    }

    /// The ribbon's Client/Server toggle.
    pub(in crate::shell) fn live_set_view(&mut self, view: View, cx: &mut Context<Self>) {
        let shown = self.play.live.stash.as_ref().map(|s| s.view);
        self.play.live.view = view;
        let Some(old) = shown else {
            return;
        };
        if old == view {
            return;
        }
        self.deselect(cx);
        self.live_swap(old);
        self.live_swap(view);
        if let Some(stash) = self.play.live.stash.as_mut() {
            stash.view = view;
        }
        self.play.live.watched = None;
        self.live_refresh(cx);
    }

    /// A probe message for the live trees; false for any other message.
    pub(in crate::shell) fn live_apply(&mut self, event: &Uplink, cx: &mut Context<Self>) -> bool {
        let (side, structural) = match event {
            Uplink::Tree { side, .. } | Uplink::Diff { side, .. } => (*side, true),
            Uplink::Props { side, .. } => (*side, false),
            _ => return false,
        };
        let view = view_of(side);
        let shown = self
            .play
            .live
            .stash
            .as_ref()
            .is_some_and(|s| s.view == view);
        if shown {
            self.live_swap(view);
        }
        let live = &mut self.play.live;
        match event {
            Uplink::Tree { reset, items, .. } => {
                let tree = live.tree_mut(view);
                if *reset {
                    tree.clear();
                    live.attributes_mut(view).clear();
                }
                live.tree_mut(view).add(items);
            }
            Uplink::Diff {
                added,
                removed,
                renamed,
                moved,
                ..
            } => {
                let tree = live.tree_mut(view);
                tree.add(added);
                tree.remove(removed);
                tree.rename(renamed);
                tree.reparent(moved);
                let attributes = live.attributes_mut(view);
                for id in removed {
                    attributes.remove(id);
                }
            }
            Uplink::Props {
                id,
                values,
                attributes,
                tags,
                ..
            } => {
                live.tree_mut(view).apply_props(
                    *id,
                    values,
                    attributes.as_ref(),
                    tags.as_deref(),
                    &self.database,
                );
                if let Some(attributes) = attributes {
                    let json = attributes
                        .iter()
                        .map(|(k, v)| (k.clone(), v.to_string()))
                        .collect();
                    live.attributes_mut(view).insert(*id, json);
                }
            }
            _ => {}
        }
        if !shown {
            live.tree_mut(view).dom.take_changes();
            return true;
        }
        self.live_swap(view);
        let changes = self.dom.take_changes();
        self.properties.dom_changed(&changes);
        if structural {
            self.rebuild_explorer(cx);
        }
        cx.notify();
        true
    }

    /// Edits made while a live view is shown (from `reflect_changes`).
    pub(in crate::shell) fn live_reflect(&mut self, changes: &[Change], cx: &mut Context<Self>) {
        let Some(view) = self.play.live.stash.as_ref().map(|s| s.view) else {
            return;
        };
        let side = view.side();
        let mut commands = Vec::new();
        let mut created = Vec::new();
        let mut structural = false;
        for change in changes {
            match change {
                Change::Property { referent, name } => {
                    if name == "Name" {
                        structural = true;
                    }
                    self.live_property_commands(view, *referent, name, &mut commands);
                }
                Change::Parent { referent, new, .. } => {
                    structural = true;
                    let tree = self.play.live.tree(view);
                    let Some(target) = tree.id_of(*referent) else {
                        continue;
                    };
                    let parent = match new {
                        Some(parent) => match tree.id_of(*parent) {
                            Some(id) => id,
                            None => continue,
                        },
                        None => 0,
                    };
                    commands.push(CommandBody::Reparent {
                        side,
                        target,
                        parent,
                    });
                }
                Change::Removed(referent) => {
                    structural = true;
                    if let Some(target) = self.play.live.tree(view).id_of(*referent) {
                        commands.push(CommandBody::Destroy { side, target });
                    }
                }
                Change::Added(referent) => {
                    structural = true;
                    let Some(instance) = self.dom.get(*referent) else {
                        continue;
                    };
                    let tree = self.play.live.tree(view);
                    let parent = match self.dom.parent(*referent) {
                        Some(parent) => tree.id_of(parent),
                        None => Some(0),
                    };
                    created.push(*referent);
                    if let Some(parent) = parent {
                        commands.push(CommandBody::Create {
                            side,
                            class: instance.class().to_string(),
                            name: instance.name().to_string(),
                            parent,
                        });
                    }
                }
                Change::Class(_) => structural = true,
            }
        }
        for referent in created {
            if self.dom.get(referent).is_some() {
                self.dom.remove(referent);
            }
        }
        if let Some(link) = self.play.link.as_ref() {
            for command in commands {
                link.channel.send(command);
            }
        }
        let changes = self.dom.take_changes();
        self.properties.dom_changed(&changes);
        if structural {
            self.rebuild_explorer(cx);
        }
        cx.notify();
    }

    fn live_property_commands(
        &mut self,
        view: View,
        referent: Ref,
        name: &str,
        commands: &mut Vec<CommandBody>,
    ) {
        let side = view.side();
        let Some(target) = self.play.live.tree(view).id_of(referent) else {
            return;
        };
        let Some(instance) = self.dom.get(referent) else {
            return;
        };
        match name {
            "Tags" => {}
            "AttributesSerialize" => {
                let now = attribute_json(&instance.attributes());
                let last = self
                    .play
                    .live
                    .attributes_mut(view)
                    .remove(&target)
                    .unwrap_or_default();
                for (key, json) in &now {
                    if last.get(key) != Some(json) {
                        let Ok(value) = serde_json::from_str(json) else {
                            continue;
                        };
                        commands.push(CommandBody::SetAttribute {
                            side,
                            target,
                            name: key.clone(),
                            value,
                        });
                    }
                }
                for key in last.keys().filter(|k| !now.contains_key(*k)) {
                    commands.push(CommandBody::SetAttribute {
                        side,
                        target,
                        name: key.clone(),
                        value: serde_json::Value::Null,
                    });
                }
                self.play.live.attributes_mut(view).insert(target, now);
            }
            _ => {
                let value = if name == "Name" {
                    Variant::String(instance.name().to_string())
                } else {
                    match instance.properties().get(name) {
                        Some(value) => value.clone(),
                        None => return,
                    }
                };
                let Some(descriptor) = self.database.resolve_property(instance.class(), name)
                else {
                    return;
                };
                let tree = self.play.live.tree(view);
                let Some(json) =
                    rbx_play::value::encode(&value, &descriptor.value_type, &self.database, &|r| {
                        tree.id_of(r)
                    })
                else {
                    return;
                };
                commands.push(CommandBody::Set {
                    side,
                    target,
                    prop: name.to_string(),
                    value: json,
                });
            }
        }
    }

    /// Each Play poll: keeps the shown side's watch set on the selection.
    pub(in crate::shell) fn live_tick(&mut self, cx: &mut Context<Self>) {
        let _ = cx;
        let Some(view) = self.play.live.stash.as_ref().map(|s| s.view) else {
            return;
        };
        let tree = self.play.live.tree(view);
        let selected: Vec<(u64, Ref)> = self
            .selected_all()
            .iter()
            .filter_map(|r| tree.id_of(*r).map(|id| (id, *r)))
            .collect();
        let ids: Vec<u64> = selected.iter().map(|(id, _)| *id).collect();
        if self
            .play
            .live
            .watched
            .as_ref()
            .is_some_and(|(v, last)| *v == view && *last == ids)
        {
            return;
        }
        let Some(link) = self.play.link.as_ref() else {
            return;
        };
        let items = selected
            .iter()
            .filter_map(|(id, r)| {
                let class = self.dom.get(*r)?.class();
                Some(WatchItem {
                    id: *id,
                    props: properties_for_class(&self.database, class),
                })
            })
            .collect();
        link.channel.send(CommandBody::Watch {
            side: view.side(),
            items,
        });
        self.play.live.watched = Some((view, ids));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_reflection::ReflectionDatabase;

    #[test]
    fn part_lists_visible_properties() {
        let db = ReflectionDatabase::shared();
        let names = properties_for_class(db, "Part");
        for wanted in ["Size", "Anchored", "Name"] {
            assert!(names.iter().any(|n| n == wanted), "missing {wanted}");
        }
        let mut deprecated = HashSet::new();
        let mut current = db.class("Part");
        while let Some(class) = current {
            for property in class.properties.iter().filter(|p| p.is_deprecated()) {
                deprecated.insert(property.name.clone());
            }
            current = class.superclass.as_deref().and_then(|s| db.class(s));
        }
        assert!(names.iter().all(|n| !deprecated.contains(n)));
        let unique: HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
    }

    #[test]
    fn view_side() {
        assert_eq!(View::Client.side(), Side::Client);
        assert_eq!(View::Server.side(), Side::Server);
    }
}
