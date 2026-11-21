//! The Players service and its players: joining, characters
//! (`LoadCharacter`, `CharacterAutoLoads`, respawning after `RespawnTime`),
//! where they spawn, `StarterCharacter`, `StarterCharacterScripts` and
//! `StarterPlayerScripts`. Written from the Players, Player and
//! SpawnLocation docs and the PlayersCharacter golden.

use std::collections::HashMap;

use rbx_dom::{CFrameData, Ref, Variant, WeakDom};
use rbx_lua::server::{Arg, CallReply, Event};
use rbx_reflection::ReflectionDatabase;

use crate::humanoid::{cframe, flag, inverse, mul, number, reference, set, vector};

/// Where a character goes when no spawn will have it (PlayersCharacter
/// `spawn_none`: the root of an R6 at 103, so its feet at 100).
const NO_SPAWN_HEIGHT: f32 = 100.0;

/// Builds characters and descriptions from avatars: what the app knows
/// about signed-in users (public avatar endpoints, the asset cache). The
/// headless simulation has none and falls back to the noob.
pub trait Avatars {
    /// `Players:GetHumanoidDescriptionFromUserId`: a DOM whose one root is
    /// a `HumanoidDescription`.
    fn description(&self, user_id: i64) -> Result<WeakDom, String>;
    /// A character model (the DOM's one root) dressed as `description`
    /// says, facing -Z with its feet at the origin.
    fn character(&self, description: &WeakDom) -> Result<WeakDom, String>;
}

/// A player in the game, as the server keeps them.
pub struct Member {
    pub player: Ref,
    pub user_id: i64,
    pub name: String,
    /// Their character as a DOM whose one root is the model, copied for
    /// every spawn; [`Players::template`], else the noob, when unset.
    pub template: Option<WeakDom>,
    /// `CharacterAppearanceLoaded` waiting for a frame: (frame, character).
    appearance: Vec<(u64, Ref)>,
    respawn_at: Option<u64>,
    joined: bool,
    /// Whether a character has spawned yet: a `ScreenGui` that isn't reset
    /// on spawn is copied on the first spawn only.
    spawned: bool,
}

#[derive(Default)]
pub struct Players {
    /// The server's players, in the order they joined.
    pub members: Vec<Member>,
    /// A client's own player, `Players.LocalPlayer` (nil on a server).
    pub local: Option<Ref>,
    /// The character for players who bring none.
    pub template: Option<WeakDom>,
    pub avatars: Option<std::rc::Rc<dyn Avatars>>,
    /// Old characters to destroy, once their removal has been seen.
    pub old: Vec<Ref>,
    pub frame: u64,
    /// A client: characters come from the server, `LoadCharacter` and
    /// `ApplyDescription` refuse.
    pub client: bool,
    /// Players a script kicked, with its message, for the session to
    /// take out.
    pub kicked: Vec<(Ref, String)>,
}

const PLAYERS_METHODS: &[&str] = &[
    "GetPlayers",
    "GetPlayerFromCharacter",
    "GetPlayerByUserId",
    // The fetch behind `GetHumanoidDescriptionFromUserId`, which
    // `helpers.luau` makes yield like the web call it is on Roblox.
    "GetHumanoidDescriptionFromUserId#fetch",
    "CreateHumanoidModelFromDescription",
];
const HUMANOID_METHODS: &[&str] = &["ApplyDescription", "GetAppliedDescription"];
/// What `ApplyDescription` swaps on a character: what it wears and its
/// colours.
const WORN: &[&str] = &["Accessory", "Shirt", "Pants", "ShirtGraphic", "BodyColors"];
const PLAYER_METHODS: &[&str] = &[
    "LoadCharacter",
    "LoadCharacter#parent",
    "LoadCharacter#build",
    "Kick",
];

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    (class == "Players" && PLAYERS_METHODS.contains(&key))
        || (class == "Player" && PLAYER_METHODS.contains(&key))
        || (class == "Humanoid" && HUMANOID_METHODS.contains(&key))
}

pub(crate) fn is_signal(class: &str, key: &str) -> bool {
    matches!(
        (class, key),
        ("Players", "PlayerAdded" | "PlayerRemoving")
            | (
                "Player",
                "CharacterAdded" | "CharacterRemoving" | "CharacterAppearanceLoaded" | "Chatted"
            )
    )
}

/// `Players.LocalPlayer`: the client's own player, nil on the server.
pub(crate) fn property(
    shared: &crate::physics::Shared,
    _referent: Ref,
    class: &str,
    key: &str,
) -> Option<Arg> {
    if class == "Players" && key == "LocalPlayer" {
        return Some(shared.players.local.map_or(Arg::Nil, Arg::Instance));
    }
    None
}

fn service(dom: &WeakDom, class: &str) -> Option<Ref> {
    dom.root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == class))
}

fn child_of_class(dom: &WeakDom, parent: Ref, class: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.class() == class))
}

fn child_named(dom: &WeakDom, parent: Ref, name: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))
}

/// Copies `root` of `from` (and everything under it) under `parent` in
/// `to`; references inside the copy point into the copy.
pub fn graft(to: &mut WeakDom, from: &WeakDom, root: Ref, parent: Option<Ref>) -> Ref {
    graft_with(to, from, root, parent, &mut |r| Some(r))
}

/// [`graft`], a reference to outside the copy going through `outside`
/// (`None` drops the property).
fn graft_with(
    to: &mut WeakDom,
    from: &WeakDom,
    root: Ref,
    parent: Option<Ref>,
    outside: &mut dyn FnMut(Ref) -> Option<Ref>,
) -> Ref {
    let order = snapshot(from, root);
    insert(to, order, root, parent, outside)
}

/// `root` and everything under it, parents before children: referent,
/// class, name, properties and parent.
type Snapshot = Vec<(
    Ref,
    String,
    String,
    std::collections::BTreeMap<String, Variant>,
    Option<Ref>,
)>;

fn snapshot(from: &WeakDom, root: Ref) -> Snapshot {
    let mut order = Vec::new();
    let mut stack = vec![root];
    while let Some(at) = stack.pop() {
        if let Some(i) = from.get(at) {
            order.push((
                at,
                i.class().to_owned(),
                i.name().to_owned(),
                i.properties().clone(),
                from.parent(at),
            ));
            stack.extend(i.children().iter().rev());
        }
    }
    order
}

fn insert(
    to: &mut WeakDom,
    order: Snapshot,
    root: Ref,
    parent: Option<Ref>,
    outside: &mut dyn FnMut(Ref) -> Option<Ref>,
) -> Ref {
    let mut map = HashMap::new();
    for (old, class, name, _, old_parent) in &order {
        let new_parent = if *old == root {
            parent
        } else {
            old_parent.and_then(|p| map.get(&p).copied())
        };
        map.insert(*old, to.new_instance(class, name, new_parent));
    }
    for (old, _, _, props, _) in order {
        let new = map[&old];
        for (key, value) in props {
            let value = match value {
                Variant::Ref(r) => match map.get(&r).copied().or_else(|| outside(r)) {
                    Some(r) => Variant::Ref(r),
                    None => continue,
                },
                other => other,
            };
            let _ = to.set_property(new, &key, value);
        }
    }
    map[&root]
}

/// `Humanoid:ApplyDescription`: the app's avatars dress a fresh character
/// as `description` says, and its accessories, clothing and body colours
/// replace the character's own, each accessory put where it sat on the
/// fresh one relative to the same part. The body takes the fresh one's
/// shape too (see [`reshape`]). A copy of the description goes under the
/// humanoid for `GetAppliedDescription`.
fn apply_description(
    players: &Players,
    dom: &mut WeakDom,
    humanoid: Ref,
    description: Ref,
) -> Result<(), String> {
    if dom.get(description).map(|i| i.class()) != Some("HumanoidDescription") {
        return Err("Argument 1 must be a HumanoidDescription".into());
    }
    let model = dom
        .parent(humanoid)
        .ok_or("the Humanoid has no character")?;
    let avatars = players
        .avatars
        .as_ref()
        .ok_or("Avatars are not available offline")?;
    let mut alone = WeakDom::new();
    graft(&mut alone, dom, description, None);
    let fresh = avatars.character(&alone)?;
    let fresh_model = *fresh.root_refs().first().ok_or("no character was built")?;
    let worn = |dom: &WeakDom, model: Ref| -> Vec<Ref> {
        dom.get(model)
            .map(|m| m.children().to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter(|c| dom.get(*c).is_some_and(|i| WORN.contains(&i.class())))
            .collect()
    };
    reshape(dom, &fresh, fresh_model, model);
    for old in worn(dom, model) {
        dom.remove(old);
    }
    let parts: Vec<(String, Ref)> = dom
        .get(model)
        .map(|m| m.children().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| dom.get(c).map(|i| (i.name().to_owned(), c)))
        .collect();
    for item in worn(&fresh, fresh_model) {
        let mut seated = None;
        let copy = graft_with(dom, &fresh, item, Some(model), &mut |r| {
            let name = fresh.get(r)?.name();
            let target = parts.iter().find(|(n, _)| n == name)?.1;
            seated.get_or_insert((r, target));
            Some(target)
        });
        if let Some((on, target)) = seated {
            reseat(dom, &fresh, copy, on, target);
        }
    }
    if let Some(old) = child_of_class(dom, humanoid, "HumanoidDescription") {
        dom.remove(old);
    }
    copy(dom, description, Some(humanoid));
    Ok(())
}

/// What of the humanoid a description decides: its scales and the hip
/// height they come to. Health, speeds and the rest stay the game's.
const HUMANOID_SHAPE: &[&str] = &["HipHeight", "InternalBodyScale", "InternalHeadScale"];

/// The body of `fresh_model` (a character built from the description)
/// given to `model`: each body part takes its namesake's mesh, size and
/// attachments, each joint its offsets, and the humanoid its scales, so
/// the body-part packages and BodyScale / BodyType / proportions /
/// HeadScale all land. Parts stay where they are; the joints lay the body
/// out again on the next frame. A part of another class (an R6 limb asked
/// to become an R15 one) is left alone: that needs a new character.
fn reshape(dom: &mut WeakDom, fresh: &WeakDom, fresh_model: Ref, model: Ref) {
    let children = |dom: &WeakDom, of: Ref| {
        dom.get(of)
            .map(|i| i.children().to_vec())
            .unwrap_or_default()
    };
    for from in children(fresh, fresh_model) {
        let Some(source) = fresh.get(from) else {
            continue;
        };
        let class = source.class();
        let Some(to) = children(dom, model).into_iter().find(|c| {
            dom.get(*c)
                .is_some_and(|i| i.name() == source.name() && i.class() == class)
        }) else {
            continue;
        };
        if class == "Humanoid" {
            for key in HUMANOID_SHAPE {
                if let Some(value) = source.properties().get(*key) {
                    let _ = dom.set_property(to, key, value.clone());
                }
            }
            match_children(dom, fresh, from, to, &|i| i.class() == "NumberValue");
        } else if is_part_class(class) {
            copy_values(dom, source, to, &["CFrame"]);
            match_children(dom, fresh, from, to, &|_| true);
        }
    }
}

fn is_part_class(class: &str) -> bool {
    matches!(class, "Part" | "MeshPart")
}

/// `from`'s values (no references, nothing in `skip`) written onto `to`.
fn copy_values(dom: &mut WeakDom, from: &rbx_dom::Instance, to: Ref, skip: &[&str]) {
    for (key, value) in from.properties() {
        if !matches!(value, Variant::Ref(_)) && !skip.contains(&key.as_str()) {
            let _ = dom.set_property(to, key, value.clone());
        }
    }
}

/// Each child of `from` that `pick` accepts copied onto `to`'s child of the
/// same name and class, all the way down.
fn match_children(
    dom: &mut WeakDom,
    fresh: &WeakDom,
    from: Ref,
    to: Ref,
    pick: &dyn Fn(&rbx_dom::Instance) -> bool,
) {
    let Some(source) = fresh.get(from) else {
        return;
    };
    for child in source.children() {
        let Some(c) = fresh.get(*child).filter(|c| pick(c)) else {
            continue;
        };
        let target = dom.get(to).and_then(|t| {
            t.children().iter().copied().find(|r| {
                dom.get(*r)
                    .is_some_and(|i| i.name() == c.name() && i.class() == c.class())
            })
        });
        if let Some(target) = target {
            copy_values(dom, c, target, &[]);
            match_children(dom, fresh, *child, target, pick);
        }
    }
}

/// Moves `copy`'s parts so they sit on `target` as they sat on `on`, its
/// counterpart on the fresh character.
fn reseat(dom: &mut WeakDom, fresh: &WeakDom, copy: Ref, on: Ref, target: Ref) {
    let frame = |i: Option<&rbx_dom::Instance>| match i?.properties().get("CFrame") {
        Some(Variant::CFrame(c)) => Some(*c),
        _ => None,
    };
    let (Some(from), Some(to)) = (frame(fresh.get(on)), frame(dom.get(target))) else {
        return;
    };
    let shift = mul(&to, &inverse(&from));
    let mut stack = vec![copy];
    while let Some(at) = stack.pop() {
        let Some(instance) = dom.get(at) else {
            continue;
        };
        stack.extend(instance.children().iter().copied());
        if let Some(c) = frame(Some(instance)) {
            let _ = dom.set_property(at, "CFrame", Variant::CFrame(mul(&shift, &c)));
        }
    }
}

/// What a joining player takes from `StarterPlayer` (the docs of each
/// `Player` property: "set from StarterPlayer"), `DevEnableMouseLock` from
/// `EnableMouseLockOption`. A place that never set one has StarterPlayer's
/// default, which the reflection database lacks for these: they're the
/// creator docs' defaults.
fn copy_player_settings(dom: &mut WeakDom, db: &ReflectionDatabase, from: Ref, player: Ref) {
    let settings: [(&str, &str, Variant); 12] = [
        (
            "CameraMaxZoomDistance",
            "CameraMaxZoomDistance",
            Variant::Float32(400.0),
        ),
        (
            "CameraMinZoomDistance",
            "CameraMinZoomDistance",
            Variant::Float32(0.5),
        ),
        ("CameraMode", "CameraMode", Variant::Enum(0)),
        ("AutoJumpEnabled", "AutoJumpEnabled", Variant::Bool(true)),
        (
            "HealthDisplayDistance",
            "HealthDisplayDistance",
            Variant::Float32(100.0),
        ),
        (
            "NameDisplayDistance",
            "NameDisplayDistance",
            Variant::Float32(100.0),
        ),
        (
            "DevCameraOcclusionMode",
            "DevCameraOcclusionMode",
            Variant::Enum(0),
        ),
        (
            "DevComputerMovementMode",
            "DevComputerMovementMode",
            Variant::Enum(0),
        ),
        (
            "DevTouchMovementMode",
            "DevTouchMovementMode",
            Variant::Enum(0),
        ),
        (
            "DevComputerCameraMovementMode",
            "DevComputerCameraMode",
            Variant::Enum(0),
        ),
        (
            "DevTouchCameraMovementMode",
            "DevTouchCameraMode",
            Variant::Enum(0),
        ),
        (
            "EnableMouseLockOption",
            "DevEnableMouseLock",
            Variant::Bool(true),
        ),
    ];
    for (source, target, default) in settings {
        let value = dom
            .get(from)
            .and_then(|i| db.stored_or_default(i, source))
            .map_or(default, |(_, v)| v.clone());
        let _ = dom.set_property(player, target, value);
    }
}

/// Copies within one DOM.
fn copy(dom: &mut WeakDom, root: Ref, parent: Option<Ref>) -> Ref {
    let order = snapshot(dom, root);
    insert(dom, order, root, parent, &mut Some)
}

/// The `Player`s under `Players`, in child order.
pub(crate) fn players_of(dom: &WeakDom) -> Vec<Ref> {
    service(dom, "Players")
        .and_then(|p| dom.get(p))
        .map(|p| p.children().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|c| dom.get(*c).is_some_and(|i| i.class() == "Player"))
        .collect()
}

/// A client's `PlayerScripts`: a copy of `StarterPlayerScripts`, and our
/// default controls and camera unless the place brings its own
/// `PlayerModule`. Made on the client, so the server never has it.
pub(crate) fn player_scripts(dom: &mut WeakDom, db: &ReflectionDatabase, player: Ref) -> Ref {
    let scripts = dom.new_instance("PlayerScripts", "PlayerScripts", Some(player));
    let starter =
        service(dom, "StarterPlayer").and_then(|s| child_of_class(dom, s, "StarterPlayerScripts"));
    if let Some(starter) = starter {
        for child in dom
            .get(starter)
            .map(|i| i.children().to_vec())
            .unwrap_or_default()
        {
            copy(dom, child, Some(scripts));
        }
    }
    if child_named(dom, scripts, "PlayerModule").is_none() {
        let module = dom.new_instance("ModuleScript", "PlayerModule", Some(scripts));
        set(
            dom,
            db,
            module,
            "Source",
            Variant::String(crate::character::PLAYER_MODULE_SOURCE.into()),
        );
        let loader = dom.new_instance("LocalScript", "PlayerScriptsLoader", Some(scripts));
        set(
            dom,
            db,
            loader,
            "Source",
            Variant::String(crate::character::PLAYER_LOADER_SOURCE.into()),
        );
    }
    scripts
}

impl Players {
    /// Adds a player under `Players`, with `Backpack` and `PlayerGui`
    /// (their client makes its own `PlayerScripts`). `PlayerAdded` and the
    /// first character come on the next frame.
    pub fn join(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        name: &str,
        user_id: i64,
        template: Option<WeakDom>,
    ) -> Option<Ref> {
        let players = service(dom, "Players")?;
        let player = dom.new_instance("Player", name, Some(players));
        set(dom, db, player, "UserId", Variant::Int64(user_id));
        set(dom, db, player, "DisplayName", Variant::String(name.into()));
        // Neutral and white until a team takes them (Teams `defaults`).
        let _ = dom.set_property(player, "TeamColor", Variant::BrickColor(1));
        let _ = dom.set_property(player, "Neutral", Variant::Bool(true));
        dom.new_instance("Backpack", "Backpack", Some(player));
        dom.new_instance("StarterGear", "StarterGear", Some(player));
        dom.new_instance("PlayerGui", "PlayerGui", Some(player));
        if let Some(starter_player) = service(dom, "StarterPlayer") {
            copy_player_settings(dom, db, starter_player, player);
        }
        self.members.push(Member {
            player,
            user_id,
            name: name.into(),
            template,
            appearance: Vec::new(),
            respawn_at: None,
            joined: false,
            spawned: false,
        });
        Some(player)
    }

    /// Instances whose LocalScripts a client runs: its player (`Backpack`,
    /// `PlayerGui`, `PlayerScripts`) and its character.
    pub fn client_roots(&self, dom: &WeakDom) -> Vec<Ref> {
        let Some(local) = self.local else {
            return Vec::new();
        };
        let mut roots = vec![local];
        if let Some(character) = reference(dom, local, "Character") {
            roots.push(character);
        }
        roots
    }
    /// The frame's players work, before physics, as steps for the
    /// simulation to run with the scripts in between.
    pub fn due(
        &mut self,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        humanoids: &crate::humanoid::Humanoids,
    ) -> Vec<Due> {
        let mut out = Vec::new();
        let players = service(dom, "Players");
        let auto = players.is_some_and(|p| flag(dom, db, p, "CharacterAutoLoads"));
        let frame = self.frame;
        for member in &mut self.members {
            let player = member.player;
            if dom.get(player).is_none() {
                continue;
            }
            if !member.joined {
                member.joined = true;
                if let Some(players) = players {
                    out.push(Due::Fire(Event::Signal {
                        owner: players,
                        name: "PlayerAdded".into(),
                        args: vec![Arg::Instance(player)],
                    }));
                }
                if auto {
                    out.push(Due::Load(player));
                }
            }
            for (_, character) in member.appearance.iter().filter(|(at, _)| *at <= frame) {
                out.push(Due::Fire(Event::Signal {
                    owner: player,
                    name: "CharacterAppearanceLoaded".into(),
                    args: vec![Arg::Instance(*character)],
                }));
            }
            member.appearance.retain(|(at, _)| *at > frame);
            // A dead character respawns RespawnTime later (PlayersCharacter
            // `respawn`: 60 frames for 1 s).
            let character = reference(dom, player, "Character");
            let died = character.and_then(|c| {
                humanoids
                    .characters
                    .values()
                    .find(|h| h.model == c && h.state == crate::humanoid::State::Dead)
                    .and_then(|h| h.died_at)
            });
            match (died, member.respawn_at) {
                (Some(died), None) if auto => {
                    let seconds = players.map_or(5.0, |p| number(dom, db, p, "RespawnTime"));
                    member.respawn_at = Some(died + (seconds * 60.0).round().max(0.0) as u64);
                }
                (Some(_), Some(at)) if frame >= at => {
                    member.respawn_at = None;
                    out.push(Due::Load(player));
                }
                (None, Some(_)) => member.respawn_at = None,
                _ => {}
            }
        }
        out
    }
    /// `LoadCharacter`'s first step: `CharacterRemoving` for the old
    /// character while it's still where it was.
    pub fn removing(&self, dom: &WeakDom, player: Ref) -> Option<(Ref, String, Vec<Arg>)> {
        let old = reference(dom, player, "Character")?;
        Some((player, "CharacterRemoving".into(), vec![Arg::Instance(old)]))
    }

    /// The second: the old character leaves (destroyed on the next frame),
    /// the new one is built, unparented, and becomes `Character`; then
    /// `CharacterAdded` (PlayersCharacter `added:nil`).
    pub fn build(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        player: Ref,
    ) -> Option<(Ref, String, Vec<Arg>)> {
        let index = self.members.iter().position(|m| m.player == player)?;
        if let Some(old) = reference(dom, player, "Character") {
            dom.unparent(old);
            self.old.push(old);
        }
        let character = self.new_character(dom, db, &self.members[index]);
        set(dom, db, player, "Character", Variant::Ref(character));
        Some((
            player,
            "CharacterAdded".into(),
            vec![Arg::Instance(character)],
        ))
    }

    /// The third: the new character goes on a spawn in Workspace, and its
    /// appearance counts as loaded on the next frame.
    pub fn place(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, player: Ref) {
        let Some(character) = reference(dom, player, "Character") else {
            return;
        };
        if dom.parent(character).is_some() {
            return;
        }
        let Some(workspace) = service(dom, "Workspace") else {
            return;
        };
        let mut spawn = self.pick_spawn(dom, db, player);
        // Like `Model:MoveTo`, a spawn someone is standing on puts the new
        // character on top of them.
        if let Some(top) = spawn.as_mut() {
            let others: Vec<Ref> = self
                .members
                .iter()
                .filter(|m| m.player != player)
                .filter_map(|m| reference(dom, m.player, "Character"))
                .filter(|c| dom.parent(*c).is_some())
                .collect();
            for other in others {
                if let Some(height) = standing_on(dom, db, other, top.position) {
                    top.position.y = top.position.y.max(height);
                }
            }
        }
        put_on(dom, db, character, spawn);
        dom.set_parent(character, Some(workspace));
        let frame = self.frame;
        let mut first = false;
        if let Some(member) = self.members.iter_mut().find(|m| m.player == player) {
            member.appearance.push((frame + 1, character));
            first = !std::mem::replace(&mut member.spawned, true);
        }
        self.refill_backpack(dom, player);
        self.refill_gui(dom, db, player, first);
    }

    /// A spawn's new `Backpack` (ToolEquip `backpack_new`): the old one goes
    /// with its tools, the new one gets `StarterPack`'s, then the player's
    /// `StarterGear`'s.
    fn refill_backpack(&mut self, dom: &mut WeakDom, player: Ref) {
        if let Some(old) = child_of_class(dom, player, "Backpack") {
            dom.unparent(old);
            self.old.push(old);
        }
        let backpack = dom.new_instance("Backpack", "Backpack", Some(player));
        let sources = [
            service(dom, "StarterPack"),
            child_of_class(dom, player, "StarterGear"),
        ];
        for source in sources.into_iter().flatten() {
            for child in dom
                .get(source)
                .map(|i| i.children().to_vec())
                .unwrap_or_default()
            {
                copy(dom, child, Some(backpack));
            }
        }
    }

    /// `StarterGui` into `PlayerGui` on a spawn (GuiRuntime): what's there
    /// goes, but for the `ScreenGui`s (any `LayerCollector`) with
    /// `ResetOnSpawn` off; then `StarterGui`'s contents are copied, those
    /// with `ResetOnSpawn` off on the player's first spawn only.
    fn refill_gui(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, player: Ref, first: bool) {
        let Some(gui) = child_of_class(dom, player, "PlayerGui") else {
            return;
        };
        let kept = |dom: &WeakDom, r: Ref| {
            dom.get(r)
                .is_some_and(|i| db.is_subclass_of(i.class(), "LayerCollector"))
                && !flag(dom, db, r, "ResetOnSpawn")
        };
        for child in dom
            .get(gui)
            .map(|i| i.children().to_vec())
            .unwrap_or_default()
        {
            if !kept(dom, child) {
                dom.unparent(child);
                self.old.push(child);
            }
        }
        let Some(starter) = service(dom, "StarterGui") else {
            return;
        };
        for child in dom
            .get(starter)
            .map(|i| i.children().to_vec())
            .unwrap_or_default()
        {
            if first || !kept(dom, child) {
                copy(dom, child, Some(gui));
            }
        }
    }

    fn new_character(&self, dom: &mut WeakDom, db: &ReflectionDatabase, local: &Member) -> Ref {
        let starter =
            service(dom, "StarterPlayer").and_then(|s| child_named(dom, s, "StarterCharacter"));
        let template = local.template.as_ref().or(self.template.as_ref());
        let model = match (starter, template) {
            (Some(starter), _) => copy(dom, starter, None),
            (None, Some(template)) => match template.root_refs().first() {
                Some(root) => graft(dom, template, *root, None),
                None => crate::character::noob(dom, &local.name, None),
            },
            (None, None) => crate::character::noob(dom, &local.name, None),
        };
        let _ = dom.set_name(model, &local.name);
        let r15 = child_of_class(dom, model, "Humanoid").is_some_and(|h| {
            matches!(
                db.stored_or_default(dom.get(h).unwrap(), "RigType"),
                Some((_, Variant::Enum(1)))
            )
        });
        crate::character::finish(dom, model, r15);
        let scripts = service(dom, "StarterPlayer")
            .and_then(|s| child_of_class(dom, s, "StarterCharacterScripts"));
        if let Some(scripts) = scripts {
            for child in dom
                .get(scripts)
                .map(|i| i.children().to_vec())
                .unwrap_or_default()
            {
                let name = dom
                    .get(child)
                    .map(|i| i.name().to_owned())
                    .unwrap_or_default();
                // A StarterCharacterScripts script replaces the character's
                // own of the same name (`Animate`, `Health`).
                if let Some(own) = child_named(dom, model, &name) {
                    dom.remove(own);
                }
                copy(dom, child, Some(model));
            }
        }
        model
    }

    /// An enabled SpawnLocation the player may use: neutral ones for
    /// everyone, the others for their team's colour only (PlayersCharacter
    /// `spawn_bc`, `spawn_b_only`). Several: one in turn.
    fn pick_spawn(
        &self,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        player: Ref,
    ) -> Option<CFrameData> {
        let neutral_player = flag(dom, db, player, "Neutral");
        let team_color = match dom
            .get(player)
            .and_then(|i| db.stored_or_default(i, "TeamColor"))
        {
            Some((_, Variant::BrickColor(c))) => Some(*c),
            _ => None,
        };
        let mut found = Vec::new();
        let mut stack: Vec<Ref> = service(dom, "Workspace").into_iter().collect();
        while let Some(at) = stack.pop() {
            let Some(i) = dom.get(at) else { continue };
            stack.extend(i.children().iter().copied());
            if i.class() != "SpawnLocation" || !flag(dom, db, at, "Enabled") {
                continue;
            }
            let neutral = flag(dom, db, at, "Neutral");
            let color = match db.stored_or_default(i, "TeamColor") {
                Some((_, Variant::BrickColor(c))) => Some(*c),
                _ => None,
            };
            if neutral || (!neutral_player && color == team_color) {
                found.push(at);
            }
        }
        if found.is_empty() {
            return None;
        }
        let at = found[(self.frame as usize) % found.len()];
        let mut top = cframe(dom, db, at)?;
        let up = vector(dom, db, at, "Size")[1] as f32 / 2.0;
        let r = top.rotation;
        top.position.x += r[1] * up;
        top.position.y += r[4] * up;
        top.position.z += r[7] * up;
        Some(top)
    }
}

/// Stands `character` with its lowest point on `spawn`'s top centre, facing
/// the way the spawn faces (turned about Y only), or at the no-spawn height
/// over the origin.
/// The top of `character` when it stands within reach of `at` (2 studs
/// either way), the height a character placed there would be put at.
fn standing_on(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    character: Ref,
    at: rbx_dom::Vector3Data,
) -> Option<f32> {
    let root = cframe(dom, db, child_named(dom, character, "HumanoidRootPart")?)?;
    let (dx, dz) = (root.position.x - at.x, root.position.z - at.z);
    if dx.abs() > 2.0 || dz.abs() > 2.0 || root.position.y < at.y {
        return None;
    }
    descendants(dom, character)
        .into_iter()
        .filter(|r| {
            dom.get(*r)
                .is_some_and(|i| db.is_subclass_of(i.class(), "BasePart"))
        })
        .filter_map(|p| {
            let c = cframe(dom, db, p)?;
            let s = vector(dom, db, p, "Size");
            let r = c.rotation;
            let reach = (f64::from(r[3]).abs() * s[0]
                + f64::from(r[4]).abs() * s[1]
                + f64::from(r[5]).abs() * s[2])
                / 2.0;
            Some((f64::from(c.position.y) + reach) as f32)
        })
        .reduce(f32::max)
}

fn put_on(dom: &mut WeakDom, db: &ReflectionDatabase, character: Ref, spawn: Option<CFrameData>) {
    let parts: Vec<Ref> = descendants(dom, character)
        .into_iter()
        .filter(|r| {
            dom.get(*r)
                .is_some_and(|i| db.is_subclass_of(i.class(), "BasePart"))
        })
        .collect();
    let root = child_named(dom, character, "HumanoidRootPart")
        .or_else(|| reference(dom, character, "PrimaryPart"))
        .or_else(|| parts.first().copied());
    let Some(root_cf) = root.and_then(|r| cframe(dom, db, r)) else {
        return;
    };
    let bottom = parts
        .iter()
        .filter_map(|p| {
            let c = cframe(dom, db, *p)?;
            let s = vector(dom, db, *p, "Size");
            let r = c.rotation;
            let reach = (f64::from(r[3]).abs() * s[0]
                + f64::from(r[4]).abs() * s[1]
                + f64::from(r[5]).abs() * s[2])
                / 2.0;
            Some(f64::from(c.position.y) - reach)
        })
        .fold(f64::INFINITY, f64::min);
    let bottom = if bottom.is_finite() {
        bottom as f32
    } else {
        root_cf.position.y
    };
    let (target, yaw_rot) = match spawn {
        Some(top) => {
            let r = top.rotation;
            let (lx, lz) = (-r[2], -r[8]);
            let len = (lx * lx + lz * lz).sqrt().max(1e-6);
            let (lx, lz) = (lx / len, lz / len);
            // Columns: right, up, back.
            let rot = [-lz, 0.0, -lx, 0.0, 1.0, 0.0, lx, 0.0, -lz];
            (top.position, rot)
        }
        None => (
            rbx_dom::Vector3Data {
                x: 0.0,
                y: NO_SPAWN_HEIGHT,
                z: 0.0,
            },
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        ),
    };
    // Old frame: the root's position at the model's bottom, no turn.
    let from = CFrameData {
        position: rbx_dom::Vector3Data {
            x: root_cf.position.x,
            y: bottom,
            z: root_cf.position.z,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };
    let to = CFrameData {
        position: target,
        rotation: yaw_rot,
    };
    let delta = mul(&to, &inverse(&from));
    for part in parts {
        if let Some(c) = cframe(dom, db, part) {
            set(dom, db, part, "CFrame", Variant::CFrame(mul(&delta, &c)));
        }
    }
}

fn descendants(dom: &WeakDom, root: Ref) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(at) = stack.pop() {
        if let Some(i) = dom.get(at) {
            stack.extend(i.children().iter().copied());
            out.push(at);
        }
    }
    out
}

/// What the simulation runs for the players this frame.
pub enum Due {
    Fire(Event),
    /// `LoadCharacter` for this player.
    Load(Ref),
}

/// A Players or Player method a script called.
pub(crate) fn call(
    shared: &mut crate::physics::Shared,
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    referent: Ref,
    class: &str,
    method: &str,
    args: Vec<Arg>,
) -> Result<CallReply, String> {
    let players = &mut shared.players;
    let single = |fire: Option<(Ref, String, Vec<Arg>)>, next: Option<&str>| CallReply {
        fire: fire.into_iter().collect(),
        and_then: next.map(str::to_owned),
        ..CallReply::default()
    };
    match (class, method) {
        ("Players", "GetPlayers") => Ok(CallReply::value(Arg::Instances(players_of(dom)))),
        ("Players", "GetPlayerFromCharacter") => {
            let character = match args.first() {
                Some(Arg::Instance(r)) => Some(*r),
                _ => None,
            };
            let found = players_of(dom)
                .into_iter()
                .find(|p| character.is_some() && reference(dom, *p, "Character") == character);
            Ok(CallReply::value(found.map_or(Arg::Nil, Arg::Instance)))
        }
        ("Players", "GetPlayerByUserId") => {
            let id = match args.first() {
                Some(Arg::Number(n)) => *n as i64,
                _ => return Err("Argument 1 missing or nil".into()),
            };
            let found = players_of(dom).into_iter().find(|p| {
                let user = dom.get(*p).and_then(|i| i.properties().get("UserId"));
                matches!(user, Some(Variant::Int64(u)) if *u == id)
            });
            Ok(CallReply::value(found.map_or(Arg::Nil, Arg::Instance)))
        }
        ("Players", "GetHumanoidDescriptionFromUserId#fetch") => {
            let id = match args.first() {
                Some(Arg::Number(n)) => *n as i64,
                _ => return Err("Argument 1 missing or nil".into()),
            };
            let avatars = players
                .avatars
                .as_ref()
                .ok_or("Avatars are not available offline")?;
            let description = avatars.description(id)?;
            let root = description
                .root_refs()
                .first()
                .copied()
                .ok_or("no description")?;
            let copy = graft(dom, &description, root, None);
            Ok(CallReply::value(Arg::Instance(copy)))
        }
        // The rig type argument isn't honoured yet: the avatar builder makes
        // the rig the description's packages imply.
        ("Players", "CreateHumanoidModelFromDescription") => {
            let Some(Arg::Instance(description)) = args.first() else {
                return Err("Argument 1 missing or nil".into());
            };
            if dom.get(*description).map(|i| i.class()) != Some("HumanoidDescription") {
                return Err("Argument 1 must be a HumanoidDescription".into());
            }
            let avatars = players
                .avatars
                .as_ref()
                .ok_or("Avatars are not available offline")?;
            let mut alone = WeakDom::new();
            graft(&mut alone, dom, *description, None);
            let built = avatars.character(&alone)?;
            let root = *built.root_refs().first().ok_or("no character was built")?;
            let model = graft(dom, &built, root, None);
            if let Some(humanoid) = child_of_class(dom, model, "Humanoid") {
                if let Some(old) = child_of_class(dom, humanoid, "HumanoidDescription") {
                    dom.remove(old);
                }
                copy(dom, *description, Some(humanoid));
            }
            Ok(CallReply::value(Arg::Instance(model)))
        }
        // A LocalScript can only kick its own player.
        ("Player", "Kick") => {
            if players.client && players.local != Some(referent) {
                return Err("Cannot kick a non-local Player from a LocalScript".into());
            }
            let message = match args.first() {
                Some(Arg::String(s)) => s.clone(),
                _ => String::new(),
            };
            if !players.kicked.iter().any(|(p, _)| *p == referent) {
                players.kicked.push((referent, message));
            }
            Ok(CallReply::default())
        }
        ("Player", "LoadCharacter") => {
            if players.client {
                return Err("LoadCharacter can only be called by the backend server".into());
            }
            if !players.members.iter().any(|m| m.player == referent) {
                return Err("LoadCharacter can only be called on a player in the game".into());
            }
            Ok(single(
                players.removing(dom, referent),
                Some("LoadCharacter#build"),
            ))
        }
        ("Player", "LoadCharacter#build") => Ok(single(
            players.build(dom, db, referent),
            Some("LoadCharacter#parent"),
        )),
        ("Player", "LoadCharacter#parent") => {
            players.place(dom, db, referent);
            Ok(CallReply::default())
        }
        // A client may dress a humanoid it made itself (a lobby's dummy),
        // not one the server replicated.
        ("Humanoid", "ApplyDescription")
            if players.client && !crate::ownership::client_made(referent) =>
        {
            Err("Humanoid::ApplyDescription() can only be called by the backend server".into())
        }
        ("Humanoid", "ApplyDescription") => {
            let Some(Arg::Instance(description)) = args.first() else {
                return Err("Argument 1 missing or nil".into());
            };
            apply_description(players, dom, referent, *description)?;
            Ok(CallReply::default())
        }
        ("Humanoid", "GetAppliedDescription") => {
            let applied = child_of_class(dom, referent, "HumanoidDescription");
            let copy = match applied {
                Some(applied) => copy(dom, applied, None),
                None => dom.new_instance("HumanoidDescription", "HumanoidDescription", None),
            };
            Ok(CallReply::value(Arg::Instance(copy)))
        }
        _ => Err(format!("{method} is not a valid member of {class}")),
    }
}

/// `Model:MoveTo(position)`: the model's pivot (its `PrimaryPart`, else its
/// bounds' centre) lands on `position`, rotation kept, then rises until no
/// colliding part in Workspace overlaps it (boxes, not exact shapes).
pub(crate) fn model_move_to(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    model: Ref,
    args: &[Arg],
) -> Result<CallReply, String> {
    let Some(Arg::Vector3([x, y, z])) = args.first() else {
        return Err("Unable to cast value to Vector3".into());
    };
    let Some(mut to) = rbx_lua::pivot::pivot(dom, db, model) else {
        return Ok(CallReply::default());
    };
    let is_part = |dom: &WeakDom, r: Ref| {
        dom.get(r)
            .is_some_and(|i| i.class() != "Terrain" && db.is_subclass_of(i.class(), "BasePart"))
    };
    let own: Vec<Ref> = descendants(dom, model)
        .into_iter()
        .filter(|r| is_part(dom, *r))
        .collect();
    let world_box = |dom: &WeakDom, part: Ref| {
        let c = cframe(dom, db, part)?;
        let s = vector(dom, db, part, "Size");
        let r = c.rotation.map(f64::from);
        let p = [c.position.x, c.position.y, c.position.z].map(f64::from);
        let reach: [f64; 3] = std::array::from_fn(|row| {
            (r[row * 3].abs() * s[0] + r[row * 3 + 1].abs() * s[1] + r[row * 3 + 2].abs() * s[2])
                / 2.0
        });
        Some((
            std::array::from_fn::<f64, 3, _>(|i| p[i] - reach[i]),
            std::array::from_fn::<f64, 3, _>(|i| p[i] + reach[i]),
        ))
    };
    let others: Vec<([f64; 3], [f64; 3])> = service(dom, "Workspace")
        .map(|w| descendants(dom, w))
        .unwrap_or_default()
        .into_iter()
        .filter(|r| is_part(dom, *r) && !own.contains(r) && flag(dom, db, *r, "CanCollide"))
        .filter_map(|r| world_box(dom, r))
        .collect();
    let shift = |from: f64| [f64::from(*x), f64::from(*y), f64::from(*z)].map(|v| v - from);
    let from = [to.position.x, to.position.y, to.position.z].map(f64::from);
    let offset = std::array::from_fn::<f64, 3, _>(|i| shift(from[i])[i]);
    let boxes: Vec<_> = own
        .iter()
        .filter_map(|p| world_box(dom, *p))
        .map(|(lo, hi)| {
            (
                std::array::from_fn::<f64, 3, _>(|i| lo[i] + offset[i]),
                std::array::from_fn::<f64, 3, _>(|i| hi[i] + offset[i]),
            )
        })
        .collect();
    let mut lift = 0.0f64;
    // Each rise can land the model in something higher up: settle.
    for _ in 0..64 {
        let overlap = |(lo, hi): &([f64; 3], [f64; 3]), (olo, ohi): &([f64; 3], [f64; 3])| {
            (0..3).all(|i| {
                let (a, b) = if i == 1 {
                    (lo[i] + lift, hi[i] + lift)
                } else {
                    (lo[i], hi[i])
                };
                a < ohi[i] - 1e-3 && olo[i] < b - 1e-3
            })
        };
        let rise = boxes
            .iter()
            .flat_map(|b| {
                others
                    .iter()
                    .filter(move |o| overlap(b, o))
                    .map(move |o| o.1[1] - (b.0[1] + lift))
            })
            .fold(0.0f64, f64::max);
        if rise <= 0.0 {
            break;
        }
        lift += rise;
    }
    to.position = rbx_dom::Vector3Data {
        x: *x,
        y: (f64::from(*y) + lift) as f32,
        z: *z,
    };
    if let Some(Err(error)) = rbx_lua::pivot::pivot_to(dom, db, model, &to) {
        return Err(error);
    }
    Ok(CallReply::default())
}
