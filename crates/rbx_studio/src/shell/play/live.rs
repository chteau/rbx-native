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
//!   `SetAttribute` per changed/removed attribute; `Tags` → `SetTags`),
//!   `Parent` → `Reparent`, `Removed` → `Destroy`, `Added` → `Create` (and
//!   the local instance is removed again: the probe's own `Diff` adds the
//!   real one with its id). Values are encoded with `rbx_play::value::encode`
//!   against the reflected `value_type`.
//! - `live_tick` (every Play poll) sends `Watch` for the shown side when the
//!   selection changed: each selected instance with the property names the
//!   Properties panel lists for its class (`properties_for_class`).
//! - Scripts: a running game cannot read `Source`, so opening a script from
//!   a live tree opens the edited place's copy (`live_script_in_place`,
//!   matched by path, with the copies Roblox makes at join mapped back:
//!   `PlayerScripts` → `StarterPlayerScripts`, `PlayerGui` → `StarterGui`,
//!   `Backpack` → `StarterPack`, the character → `StarterCharacterScripts`).
//!   Its tab keeps typed text uncommitted until the test stops, when
//!   `live_end` writes it to the restored place (the running game keeps the
//!   code it started with, as in Studio).
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
    /// The "script edits apply when the test stops" note was shown.
    pub(in crate::shell) script_note: bool,
}

/// The edited place given `Shell::dom` and its live state: `dom`, or the
/// stashed place while a live tree is shown. A free function so a caller
/// can borrow it beside another `Shell` field.
pub(in crate::shell) fn edit_dom_of<'a>(dom: &'a WeakDom, live: &'a Live) -> &'a WeakDom {
    match &live.stash {
        Some(stash) => &live.tree(stash.view).dom,
        None => dom,
    }
}

/// Names from the top-level instance down to `referent`.
fn path_of(dom: &WeakDom, referent: Ref) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = Some(referent);
    while let Some(r) = current {
        let Some(instance) = dom.get(r) else {
            break;
        };
        if instance.class() == "DataModel" {
            break;
        }
        names.push(instance.name().to_string());
        current = dom.parent(r);
    }
    names.reverse();
    names
}

/// The instance at `path` (names from the top level down), first match by
/// name at each level; a `DataModel` root is looked through.
fn find_path(dom: &WeakDom, path: &[String]) -> Option<Ref> {
    let mut level: Vec<Ref> = dom.root_refs().to_vec();
    if let [only] = level.as_slice() {
        if dom.get(*only).is_some_and(|i| i.class() == "DataModel") {
            level = dom
                .get(*only)
                .map(|i| i.children().to_vec())
                .unwrap_or_default();
        }
    }
    let mut found = None;
    for name in path {
        let next = level
            .iter()
            .copied()
            .find(|r| dom.get(*r).is_some_and(|i| i.name() == name))?;
        found = Some(next);
        level = dom
            .get(next)
            .map(|i| i.children().to_vec())
            .unwrap_or_default();
    }
    found
}

/// Where a script in the running game came from in the place: the same
/// path, or for the copies Roblox makes when a player joins, the container
/// it copied from. `players` are the names under `Players` in the live tree.
pub(in crate::shell) fn place_paths(path: &[String], players: &[String]) -> Vec<Vec<String>> {
    let mut paths = vec![path.to_vec()];
    let rest = |from: usize| path.get(from..).unwrap_or_default().to_vec();
    match path {
        [players_service, _, container, ..] if players_service == "Players" => {
            let start: &[&str] = match container.as_str() {
                "PlayerScripts" => &["StarterPlayer", "StarterPlayerScripts"],
                "PlayerGui" => &["StarterGui"],
                "Backpack" | "StarterGear" => &["StarterPack"],
                _ => &[],
            };
            if !start.is_empty() {
                let mut mapped: Vec<String> = start.iter().map(|s| s.to_string()).collect();
                mapped.extend(rest(3));
                paths.push(mapped);
            }
        }
        [workspace, character, ..] if workspace == "Workspace" && players.contains(character) => {
            let mut mapped = vec![
                "StarterPlayer".to_string(),
                "StarterCharacterScripts".to_string(),
            ];
            mapped.extend(rest(2));
            paths.push(mapped);
        }
        _ => {}
    }
    paths
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
            script_note: false,
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
        edit_dom_of(&self.dom, &self.play.live)
    }

    /// The edited place's script for `live`, a script in the shown live
    /// tree (see the module docs), if the place has one of the same class.
    pub(in crate::shell) fn live_script_in_place(&self, live: Ref) -> Option<Ref> {
        let class = self.dom.get(live)?.class().to_string();
        let path = path_of(&self.dom, live);
        let players: Vec<String> = find_path(&self.dom, &["Players".to_string()])
            .and_then(|p| self.dom.get(p))
            .map(|players| {
                players
                    .children()
                    .iter()
                    .filter_map(|c| self.dom.get(*c).map(|i| i.name().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let place = self.edit_dom();
        place_paths(&path, &players)
            .iter()
            .filter_map(|candidate| find_path(place, candidate))
            .find(|r| place.get(*r).is_some_and(|i| i.class() == class))
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
        self.play.live.script_note = false;
        self.live_refresh(cx);
    }

    /// The edited place's script a game script path names (`Players.<you>.
    /// PlayerScripts.Hud` and the like mapped back as `live_script_in_place`
    /// maps them), if the place has a script there.
    pub(in crate::shell) fn play_place_script(
        &self,
        context: super::super::output::game::Context,
        path: &str,
    ) -> Option<Ref> {
        use super::super::output::game::Context as Side;
        let path: Vec<String> = path.split('.').map(str::to_string).collect();
        let tree = match context {
            Side::Server => &self.play.live.server,
            Side::Client => &self.play.live.client,
        };
        let mut players: Vec<String> = find_path(&tree.dom, &["Players".to_string()])
            .and_then(|p| tree.dom.get(p))
            .map(|p| {
                p.children()
                    .iter()
                    .filter_map(|c| tree.dom.get(*c).map(|i| i.name().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        // After the test the live trees are gone: a `Workspace.<name>`
        // the place does not have is taken to be the tester's character.
        let place = self.edit_dom();
        if let [workspace, name, ..] = path.as_slice() {
            if workspace == "Workspace"
                && find_path(place, &[workspace.clone(), name.clone()]).is_none()
            {
                players.push(name.clone());
            }
        }
        place_paths(&path, &players)
            .iter()
            .filter_map(|candidate| find_path(place, candidate))
            .find(|r| crate::script_editor::source::is_script(place, &self.database, *r))
    }

    /// An Output location from the game (`output::game`): opens the place's
    /// copy of that script at `line`, during the test or after it. Says so
    /// in Output when the script only exists in the game.
    pub(in crate::shell) fn play_reveal(
        &mut self,
        context: super::super::output::game::Context,
        location: &super::super::output::game::Location,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(script) = self.play_place_script(context, &location.path) else {
            self.output.push(
                "Play",
                crate::command_bar::Feedback::Warning(format!(
                    "{} only exists in the running game, so its source cannot be shown.",
                    location.path
                )),
            );
            cx.notify();
            return;
        };
        let at = lsp_types::Position::new(location.line.saturating_sub(1), 0);
        self.reveal(script, lsp_types::Range::new(at, at), window, cx);
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
        // Typing kept in script tabs during the test reaches the place now.
        self.flush_script_edits(cx);
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
            "Tags" => {
                let tags = instance.tags().into_iter().map(str::to_string).collect();
                commands.push(CommandBody::SetTags { side, target, tags });
            }
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

    fn names(path: &[&str]) -> Vec<String> {
        path.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn join_copies_map_back_to_their_starter_containers() {
        let players = names(&["Tester"]);
        let cases = [
            (
                names(&["Players", "Tester", "PlayerScripts", "Hud", "Init"]),
                names(&["StarterPlayer", "StarterPlayerScripts", "Hud", "Init"]),
            ),
            (
                names(&["Players", "Tester", "PlayerGui", "Menu", "Open"]),
                names(&["StarterGui", "Menu", "Open"]),
            ),
            (
                names(&["Players", "Tester", "Backpack", "Sword", "Swing"]),
                names(&["StarterPack", "Sword", "Swing"]),
            ),
            (
                names(&["Workspace", "Tester", "Sprint"]),
                names(&["StarterPlayer", "StarterCharacterScripts", "Sprint"]),
            ),
        ];
        for (live, place) in cases {
            let paths = place_paths(&live, &players);
            assert_eq!(paths[0], live, "the same path is tried first");
            assert_eq!(paths.get(1), Some(&place));
        }
        // A model that is not a character keeps its own path only.
        let model = names(&["Workspace", "Door", "Script"]);
        assert_eq!(place_paths(&model, &players), vec![model]);
    }

    #[test]
    fn find_path_looks_through_a_data_model_root() {
        let mut dom = WeakDom::new();
        let game = dom.new_instance("DataModel", "Game", None);
        let sss = dom.new_instance("ServerScriptService", "ServerScriptService", Some(game));
        let main = dom.new_instance("Script", "Main", Some(sss));
        assert_eq!(
            find_path(&dom, &names(&["ServerScriptService", "Main"])),
            Some(main)
        );
        assert_eq!(path_of(&dom, main), names(&["ServerScriptService", "Main"]));
        assert_eq!(
            find_path(&dom, &names(&["ServerScriptService", "Gone"])),
            None
        );
    }

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
