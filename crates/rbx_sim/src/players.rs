//! The Players service with one local player: joining, characters
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

/// The player playing.
#[derive(Clone, Debug)]
pub struct Local {
    pub player: Ref,
    pub user_id: i64,
    pub name: String,
}

#[derive(Default)]
pub struct Players {
    pub local: Option<Local>,
    /// The local player's character as a DOM whose one root is the model,
    /// copied for every spawn; the noob when unset.
    pub template: Option<WeakDom>,
    pub avatars: Option<Box<dyn Avatars>>,
    /// `CharacterAppearanceLoaded` waiting for a frame: (frame, character).
    appearance: Vec<(u64, Ref)>,
    /// Old characters to destroy, once their removal has been seen.
    pub old: Vec<Ref>,
    respawn_at: Option<u64>,
    joined: bool,
    pub frame: u64,
}

const PLAYERS_METHODS: &[&str] = &[
    "GetPlayers",
    "GetPlayerFromCharacter",
    "GetPlayerByUserId",
    "GetHumanoidDescriptionFromUserId",
];
const PLAYER_METHODS: &[&str] = &[
    "LoadCharacter",
    "LoadCharacter#parent",
    "LoadCharacter#build",
];

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    (class == "Players" && PLAYERS_METHODS.contains(&key))
        || (class == "Player" && PLAYER_METHODS.contains(&key))
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

/// `Players.LocalPlayer`: the one player. Local Run is server and client
/// in one, so server scripts see it too.
pub(crate) fn property(
    shared: &crate::physics::Shared,
    _referent: Ref,
    class: &str,
    key: &str,
) -> Option<Arg> {
    if class == "Players" && key == "LocalPlayer" {
        return Some(
            shared
                .players
                .local
                .as_ref()
                .map_or(Arg::Nil, |l| Arg::Instance(l.player)),
        );
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
    let mut order = Vec::new();
    let mut stack = vec![root];
    while let Some(at) = stack.pop() {
        if let Some(i) = from.get(at) {
            order.push((
                at,
                i.class().to_owned(),
                i.name().to_owned(),
                i.properties().clone(),
            ));
            stack.extend(i.children().iter().rev());
        }
    }
    let mut map = HashMap::new();
    for (old, class, name, _) in &order {
        let new_parent = if *old == root {
            parent
        } else {
            from.parent(*old).and_then(|p| map.get(&p).copied())
        };
        map.insert(*old, to.new_instance(class, name, new_parent));
    }
    for (old, _, _, props) in order {
        let new = map[&old];
        for (key, value) in props {
            let value = match value {
                Variant::Ref(r) => Variant::Ref(map.get(&r).copied().unwrap_or(r)),
                other => other,
            };
            let _ = to.set_property(new, &key, value);
        }
    }
    map[&root]
}

/// Copies within one DOM.
fn copy(dom: &mut WeakDom, root: Ref, parent: Option<Ref>) -> Ref {
    let source = dom.clone();
    graft(dom, &source, root, parent)
}

fn players_of(shared: &Players, dom: &WeakDom) -> Vec<Ref> {
    shared
        .local
        .iter()
        .map(|l| l.player)
        .filter(|p| dom.get(*p).is_some())
        .collect()
}

impl Players {
    /// Adds the local player under `Players`, with `Backpack`, `PlayerGui`
    /// and `PlayerScripts` (a copy of `StarterPlayerScripts`). `PlayerAdded`
    /// and the first character come on the next frame.
    pub fn join(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        name: &str,
        user_id: i64,
    ) -> Option<Ref> {
        let players = service(dom, "Players")?;
        let player = dom.new_instance("Player", name, Some(players));
        set(dom, db, player, "UserId", Variant::Int64(user_id));
        set(dom, db, player, "DisplayName", Variant::String(name.into()));
        dom.new_instance("Backpack", "Backpack", Some(player));
        dom.new_instance("PlayerGui", "PlayerGui", Some(player));
        let scripts = dom.new_instance("PlayerScripts", "PlayerScripts", Some(player));
        let starter = service(dom, "StarterPlayer")
            .and_then(|s| child_of_class(dom, s, "StarterPlayerScripts"));
        if let Some(starter) = starter {
            for child in dom
                .get(starter)
                .map(|i| i.children().to_vec())
                .unwrap_or_default()
            {
                copy(dom, child, Some(scripts));
            }
        }
        self.local = Some(Local {
            player,
            user_id,
            name: name.into(),
        });
        self.joined = false;
        Some(player)
    }

    /// Instances whose LocalScripts the local client runs: the player
    /// (`Backpack`, `PlayerGui`, `PlayerScripts`) and its character.
    pub fn client_roots(&self, dom: &WeakDom) -> Vec<Ref> {
        let Some(local) = &self.local else {
            return Vec::new();
        };
        let mut roots = vec![local.player];
        if let Some(character) = reference(dom, local.player, "Character") {
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
        let Some(local) = self.local.clone() else {
            return out;
        };
        if dom.get(local.player).is_none() {
            return out;
        }
        let players = service(dom, "Players");
        if !self.joined {
            self.joined = true;
            if let Some(players) = players {
                out.push(Due::Fire(Event::Signal {
                    owner: players,
                    name: "PlayerAdded".into(),
                    args: vec![Arg::Instance(local.player)],
                }));
            }
            if players.is_some_and(|p| flag(dom, db, p, "CharacterAutoLoads")) {
                out.push(Due::Load);
            }
        }
        let frame = self.frame;
        for (_, character) in self.appearance.iter().filter(|(at, _)| *at <= frame) {
            out.push(Due::Fire(Event::Signal {
                owner: local.player,
                name: "CharacterAppearanceLoaded".into(),
                args: vec![Arg::Instance(*character)],
            }));
        }
        self.appearance.retain(|(at, _)| *at > frame);
        // A dead character respawns RespawnTime later (PlayersCharacter
        // `respawn`: 60 frames for 1 s).
        let character = reference(dom, local.player, "Character");
        let died = character.and_then(|c| {
            humanoids
                .characters
                .values()
                .find(|h| h.model == c && h.state == crate::humanoid::State::Dead)
                .and_then(|h| h.died_at)
        });
        match (died, self.respawn_at) {
            (Some(died), None)
                if players.is_some_and(|p| flag(dom, db, p, "CharacterAutoLoads")) =>
            {
                let seconds = players.map_or(5.0, |p| number(dom, db, p, "RespawnTime"));
                self.respawn_at = Some(died + (seconds * 60.0).round().max(0.0) as u64);
            }
            (Some(_), Some(at)) if frame >= at => {
                self.respawn_at = None;
                out.push(Due::Load);
            }
            (None, Some(_)) => self.respawn_at = None,
            _ => {}
        }
        out
    }

    /// `LoadCharacter`'s first step: `CharacterRemoving` for the old
    /// character while it's still where it was.
    pub fn removing(&self, dom: &WeakDom) -> Option<(Ref, String, Vec<Arg>)> {
        let local = self.local.as_ref()?;
        let old = reference(dom, local.player, "Character")?;
        Some((
            local.player,
            "CharacterRemoving".into(),
            vec![Arg::Instance(old)],
        ))
    }

    /// The second: the old character leaves (destroyed on the next frame),
    /// the new one is built, unparented, and becomes `Character`; then
    /// `CharacterAdded` (PlayersCharacter `added:nil`).
    pub fn build(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
    ) -> Option<(Ref, String, Vec<Arg>)> {
        let local = self.local.clone()?;
        if let Some(old) = reference(dom, local.player, "Character") {
            dom.unparent(old);
            self.old.push(old);
        }
        let character = self.new_character(dom, db, &local);
        set(dom, db, local.player, "Character", Variant::Ref(character));
        Some((
            local.player,
            "CharacterAdded".into(),
            vec![Arg::Instance(character)],
        ))
    }

    /// The third: the new character goes on a spawn in Workspace, and its
    /// appearance counts as loaded on the next frame.
    pub fn place(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase) {
        let Some(local) = self.local.clone() else {
            return;
        };
        let Some(character) = reference(dom, local.player, "Character") else {
            return;
        };
        if dom.parent(character).is_some() {
            return;
        }
        let Some(workspace) = service(dom, "Workspace") else {
            return;
        };
        let spawn = self.pick_spawn(dom, db, local.player);
        put_on(dom, db, character, spawn);
        dom.set_parent(character, Some(workspace));
        self.appearance.push((self.frame + 1, character));
    }

    fn new_character(&self, dom: &mut WeakDom, db: &ReflectionDatabase, local: &Local) -> Ref {
        let starter =
            service(dom, "StarterPlayer").and_then(|s| child_named(dom, s, "StarterCharacter"));
        let model = match (starter, &self.template) {
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
    Load,
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
        ("Players", "GetPlayers") => Ok(CallReply::value(Arg::Instances(players_of(players, dom)))),
        ("Players", "GetPlayerFromCharacter") => {
            let character = match args.first() {
                Some(Arg::Instance(r)) => Some(*r),
                _ => None,
            };
            let found = players_of(players, dom)
                .into_iter()
                .find(|p| character.is_some() && reference(dom, *p, "Character") == character);
            Ok(CallReply::value(found.map_or(Arg::Nil, Arg::Instance)))
        }
        ("Players", "GetPlayerByUserId") => {
            let id = match args.first() {
                Some(Arg::Number(n)) => *n as i64,
                _ => return Err("Argument 1 missing or nil".into()),
            };
            let found = players
                .local
                .as_ref()
                .filter(|l| l.user_id == id && dom.get(l.player).is_some())
                .map(|l| l.player);
            Ok(CallReply::value(found.map_or(Arg::Nil, Arg::Instance)))
        }
        ("Players", "GetHumanoidDescriptionFromUserId") => {
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
        ("Player", "LoadCharacter") => {
            if players.local.as_ref().is_none_or(|l| l.player != referent) {
                return Err("LoadCharacter can only be called on a player in the game".into());
            }
            Ok(single(players.removing(dom), Some("LoadCharacter#build")))
        }
        ("Player", "LoadCharacter#build") => {
            Ok(single(players.build(dom, db), Some("LoadCharacter#parent")))
        }
        ("Player", "LoadCharacter#parent") => {
            players.place(dom, db);
            Ok(CallReply::default())
        }
        _ => Err(format!("{method} is not a valid member of {class}")),
    }
}
