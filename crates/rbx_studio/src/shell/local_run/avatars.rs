//! Who plays a Play (local) run, and what they look like.
//!
//! Signed in (the editor's Open Cloud key names its owner): their avatar,
//! from the public avatar endpoint, built the way Insert › Rig › Player
//! builds one, onto our own rig. Each run refreshes the cache under
//! `cache_root()/avatars/`: `<user>/avatar.json` as the endpoint sent it,
//! `<user>/user.json` with the name, and `last_user`, who played last;
//! every asset worn goes to the shared asset cache (`rbx_assets`). Offline
//! the cache stands in; with no key, or offline with no cache, the noob
//! plays. The same cache serves `GetHumanoidDescriptionFromUserId` and
//! `ApplyDescription` when Roblox can't be reached.
//!
//! `RBX_STUDIO_LOCAL_PLAYER=noob` plays as the noob whatever the key, for a
//! capture of the signed-out look.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use rbx_cloud::{ApiKey, AssetType, Avatar, AvatarAsset, AvatarBodyColors, AvatarScales, Client};
use rbx_dom::{Ref, Variant, WeakDom};
use serde::{Deserialize, Serialize};

use crate::shell::rig::{
    apply_packages, build_rig, describe_avatar, dress, gather, options_for, BodyColors, Fetched,
    JointStyle, RigOptions,
};

/// The player a run is played as.
#[derive(Clone)]
pub(super) struct Player {
    pub(super) user_id: i64,
    pub(super) name: String,
    /// Their character; `None` for the blocky last-resort noob.
    pub(super) character: Option<WeakDom>,
    /// Where the look came from, for the Output.
    pub(super) note: String,
}

#[derive(Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
}

fn root() -> Option<PathBuf> {
    rbx_assets::cache_root().map(|root| root.join("avatars"))
}

fn noob(note: String) -> Player {
    Player {
        user_id: -1,
        name: "Player1".to_owned(),
        character: rig_builder_noob(),
        note,
    }
}

/// The noob: the rig Studio's Rig Builder makes (R15, Block), when its
/// meshes can be had, online or all in the asset cache. `None` otherwise,
/// and the simulation's own blocky noob plays.
fn rig_builder_noob() -> Option<WeakDom> {
    let options = RigOptions::rig_builder_block();
    if rbx_net::is_offline() {
        let cache = rbx_assets::AssetCache::new(None).ok()?;
        if !options
            .asset_ids()
            .into_iter()
            .all(|id| cache.get_id(id).is_some())
        {
            return None;
        }
    }
    let mut scratch = WeakDom::new();
    let folder = scratch.new_instance("Folder", "Folder", None);
    let rig = build_rig(&mut scratch, &options, folder);
    let mut model = WeakDom::new();
    rbx_sim::players::graft(&mut model, &scratch, rig, None);
    Some(model)
}

/// Works out the player; blocking (network), so off the UI thread.
/// The avatar lookups for a run that restarts (a teleport), its player
/// already resolved.
pub(super) fn lookups() -> StudioAvatars {
    StudioAvatars::new(Client::new(ApiKey::from_env_or_config()))
}

pub(super) fn resolve() -> (Player, StudioAvatars) {
    let key = ApiKey::from_env_or_config();
    let avatars = StudioAvatars::new(Client::new(key.clone()));
    if key.is_none() || std::env::var("RBX_STUDIO_LOCAL_PLAYER").is_ok_and(|v| v == "noob") {
        let note = "Not signed in (no Open Cloud key): playing as the noob".to_owned();
        return (noob(note), avatars);
    }
    let player = match avatars.client.introspect() {
        Ok(info) => {
            let user = info.authorized_user_id;
            match avatars.online(user) {
                Ok(fetched) => avatars.player(user, &fetched, "Playing as {name}"),
                Err(why) => match avatars.cached(user) {
                    Some(fetched) => avatars.player(
                        user,
                        &fetched,
                        &format!("Your avatar couldn\u{2019}t be fetched ({why}): playing as {{name}} from the cache"),
                    ),
                    None => noob(format!(
                        "Your avatar couldn\u{2019}t be fetched ({why}) and none is cached: playing as the noob"
                    )),
                },
            }
        }
        Err(err @ (rbx_cloud::CloudError::Http { .. } | rbx_cloud::CloudError::Refused { .. })) => {
            noob(format!(
                "The Open Cloud key was refused ({err}): playing as the noob"
            ))
        }
        Err(_) => match last_user().and_then(|user| Some((user, avatars.cached(user)?))) {
            Some((user, fetched)) => {
                avatars.player(user, &fetched, "Offline: playing as {name} from the cache")
            }
            None => noob("Offline with no cached avatar: playing as the noob".to_owned()),
        },
    };
    (player, avatars)
}

/// What an offline pack keeps for Play (local)'s player: the Rig Builder
/// noob's meshes and, signed in and online, your avatar (refreshed into
/// the avatar cache, which an offline run plays from) with every asset it
/// wears. Blocking (network). The note says whose avatar it is.
pub(super) fn pack_seeds() -> (Vec<u64>, String) {
    let mut ids = RigOptions::rig_builder_block().asset_ids();
    let key = ApiKey::from_env_or_config();
    if key.is_none() {
        return (ids, "Not signed in: only the noob is packed".to_owned());
    }
    let avatars = StudioAvatars::new(Client::new(key));
    let user = match avatars.client.introspect() {
        Ok(info) => info.authorized_user_id,
        Err(err) => {
            return (
                ids,
                format!("Your avatar couldn\u{2019}t be packed ({err})"),
            )
        }
    };
    if let Err(why) = avatars.online(user) {
        return (
            ids,
            format!("Your avatar couldn\u{2019}t be packed ({why})"),
        );
    }
    let worn = root()
        .and_then(|root| fs::read(root.join(user.to_string()).join("avatar.json")).ok())
        .and_then(|json| Avatar::from_json(&json).ok())
        .map(|avatar| avatar.assets.iter().map(|a| a.id).collect::<Vec<_>>())
        .unwrap_or_default();
    ids.extend(worn);
    (ids, "Your avatar is packed".to_owned())
}

fn last_user() -> Option<u64> {
    fs::read_to_string(root()?.join("last_user"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn cached_name(user: u64) -> Option<String> {
    let text = fs::read(root()?.join(user.to_string()).join("user.json")).ok()?;
    serde_json::from_slice::<User>(&text).ok().map(|u| u.name)
}

/// Our rig dressed as `fetched` says: a DOM whose one root is the model,
/// feet at the origin. `colors` overrides the avatar's body colours.
fn build(fetched: &Fetched, colors: Option<BodyColors>) -> WeakDom {
    let mut options = options_for(&fetched.avatar, JointStyle::Motor6D, [0.0; 3], None);
    apply_packages(&mut options, &fetched.worn);
    if let Some(colors) = colors {
        options.colors = colors;
    }
    let mut scratch = WeakDom::new();
    let folder = scratch.new_instance("Folder", "Folder", None);
    let rig = build_rig(&mut scratch, &options, folder);
    dress(&mut scratch, rig, &fetched.worn);
    describe_avatar(&mut scratch, rig, &fetched.avatar);
    let mut model = WeakDom::new();
    rbx_sim::players::graft(&mut model, &scratch, rig, None);
    model
}

/// The app's avatars for the simulation: Roblox's public endpoints, the
/// avatar cache when they can't be reached.
pub(super) struct StudioAvatars {
    client: Client,
    /// Asset types seen on fetched avatars, so a description's ids are
    /// dressed as what they are.
    types: Mutex<HashMap<u64, u32>>,
}

impl StudioAvatars {
    fn new(client: Client) -> Self {
        StudioAvatars {
            client,
            types: Mutex::new(HashMap::new()),
        }
    }

    fn player(&self, user: u64, fetched: &Fetched, note: &str) -> Player {
        let name = cached_name(user).unwrap_or_else(|| format!("Player{user}"));
        Player {
            user_id: user as i64,
            note: note.replace("{name}", &name),
            name,
            character: Some(build(fetched, None)),
        }
    }

    fn learn(&self, avatar: &Avatar) {
        if let Ok(mut types) = self.types.lock() {
            types.extend(avatar.assets.iter().map(|a| (a.id, a.asset_type.id)));
        }
    }

    /// The avatar from Roblox, written to the cache on the way.
    fn online(&self, user: u64) -> Result<Fetched, String> {
        let avatar = self.client.avatar(user).map_err(|err| err.to_string())?;
        self.learn(&avatar);
        let dir = root().map(|root| root.join(user.to_string()));
        if let Some(dir) = &dir {
            let _ = fs::create_dir_all(dir);
            if let Ok(json) = serde_json::to_vec(&avatar) {
                let _ = fs::write(dir.join("avatar.json"), json);
            }
            if let Ok((name, _)) = self.client.user_names(user) {
                if let Ok(json) = serde_json::to_vec(&User { id: user, name }) {
                    let _ = fs::write(dir.join("user.json"), json);
                }
            }
            if let Some(root) = root() {
                let _ = fs::write(root.join("last_user"), user.to_string());
            }
        }
        Ok(gather(avatar, |id| self.download(id)))
    }

    /// The avatar as the cache last saw it.
    fn cached(&self, user: u64) -> Option<Fetched> {
        let dir = root()?.join(user.to_string());
        let avatar = Avatar::from_json(&fs::read(dir.join("avatar.json")).ok()?).ok()?;
        self.learn(&avatar);
        Some(gather(avatar, |id| self.download(id)))
    }

    /// An asset by id: from the shared asset cache when it's there (an
    /// id's content doesn't change; older builds kept avatar assets under
    /// `avatars/assets/`, still read), else from Roblox, kept for next time.
    fn download(&self, id: u64) -> Result<Vec<u8>, String> {
        let cache = rbx_assets::AssetCache::new(None).ok();
        if let Some(bytes) = cache.as_ref().and_then(|c| c.get_id(id)) {
            return Ok(bytes);
        }
        let legacy = root().map(|root| root.join("assets").join(format!("{id}.rbxm")));
        let bytes = match legacy.and_then(|file| fs::read(file).ok()) {
            Some(bytes) => bytes,
            None => {
                self.client
                    .asset(id)
                    .map_err(|err| format!("asset {id}: {err}"))?
                    .bytes
            }
        };
        if let Some(cache) = cache {
            let _ = cache.put_id(id, &bytes);
        }
        Ok(bytes)
    }

    /// What `description` asks for, as the avatar endpoint would say it.
    fn avatar_of(&self, dom: &WeakDom, description: Ref) -> Avatar {
        let types = self.types.lock().map(|t| t.clone()).unwrap_or_default();
        let get = |r: Ref, key: &str| dom.get(r).and_then(|i| i.properties().get(key)).cloned();
        let number = |key: &str, default: f64| match get(description, key) {
            Some(Variant::Float32(v)) => f64::from(v),
            Some(Variant::Float64(v)) => v,
            _ => default,
        };
        let mut assets = Vec::new();
        let mut add = |id: i64, kind: u32| {
            let id = id as u64;
            if id > 0 && !assets.iter().any(|a: &AvatarAsset| a.id == id) {
                let kind = types.get(&id).copied().unwrap_or(kind);
                assets.push(AvatarAsset {
                    id,
                    name: String::new(),
                    asset_type: AssetType {
                        id: kind,
                        name: String::new(),
                    },
                });
            }
        };
        let id_of = |value: Option<Variant>| match value {
            Some(Variant::Int64(v)) => v,
            Some(Variant::Int32(v)) => i64::from(v),
            Some(Variant::Float64(v)) => v as i64,
            _ => 0,
        };
        for (key, kind) in [
            ("Face", 18),
            ("GraphicTShirt", 2),
            ("Shirt", 11),
            ("Pants", 12),
            ("Head", 17),
            ("Torso", 27),
            ("RightArm", 28),
            ("LeftArm", 29),
            ("LeftLeg", 30),
            ("RightLeg", 31),
            ("ClimbAnimation", 48),
            ("FallAnimation", 50),
            ("IdleAnimation", 51),
            ("JumpAnimation", 52),
            ("RunAnimation", 53),
            ("SwimAnimation", 54),
            ("WalkAnimation", 55),
            ("MoodAnimation", 78),
        ] {
            add(id_of(get(description, key)), kind);
        }
        for (key, kind) in [
            ("HatAccessory", 8),
            ("HairAccessory", 41),
            ("FaceAccessory", 42),
            ("NeckAccessory", 43),
            ("ShouldersAccessory", 44),
            ("FrontAccessory", 45),
            ("BackAccessory", 46),
            ("WaistAccessory", 47),
        ] {
            if let Some(Variant::String(list)) = get(description, key) {
                for id in list.split(',').filter_map(|s| s.trim().parse().ok()) {
                    add(id, kind);
                }
            }
        }
        let children = dom
            .get(description)
            .map(|d| d.children().to_vec())
            .unwrap_or_default();
        for child in children {
            let slot = match get(child, "BodyPart").or_else(|| get(child, "AccessoryType")) {
                Some(Variant::Enum(e)) => e,
                _ => continue,
            };
            let class = dom
                .get(child)
                .map(|i| i.class().to_owned())
                .unwrap_or_default();
            let kind = match class.as_str() {
                "BodyPartDescription" => [17, 27, 29, 28, 30, 31].get(slot as usize).copied(),
                "AccessoryDescription" => {
                    let kinds = [
                        8, 41, 42, 43, 44, 45, 46, 47, 64, 65, 66, 67, 68, 69, 70, 71, 72,
                    ];
                    (slot as usize)
                        .checked_sub(1)
                        .and_then(|i| kinds.get(i))
                        .copied()
                }
                _ => None,
            };
            if let Some(kind) = kind {
                add(id_of(get(child, "AssetId")), kind);
            }
        }
        Avatar {
            scales: AvatarScales {
                height: number("HeightScale", 1.0),
                width: number("WidthScale", 1.0),
                head: number("HeadScale", 1.0),
                depth: number("DepthScale", 1.0),
                proportion: number("ProportionScale", 1.0),
                body_type: number("BodyTypeScale", 0.3),
            },
            avatar_type: "R15".to_owned(),
            body_colors: AvatarBodyColors {
                head: 194,
                torso: 194,
                right_arm: 194,
                left_arm: 194,
                right_leg: 194,
                left_leg: 194,
            },
            assets,
        }
    }
}

/// The body colours a description names: its body parts' `Color`, else
/// the `HeadColor`-style properties.
fn colors_of(dom: &WeakDom, description: Ref) -> BodyColors {
    let rgb = |value: Option<&Variant>| match value {
        Some(Variant::Color3(c)) => Some([c.r, c.g, c.b].map(|v| (v * 255.0).round() as u8)),
        _ => None,
    };
    let mut slots: [Option<[u8; 3]>; 6] = [None; 6];
    for child in dom
        .get(description)
        .map(|d| d.children().to_vec())
        .unwrap_or_default()
    {
        let Some(part) = dom.get(child) else { continue };
        if let Some(Variant::Enum(slot)) = part.properties().get("BodyPart") {
            if let Some(entry) = slots.get_mut(*slot as usize) {
                *entry = rgb(part.properties().get("Color"));
            }
        }
    }
    let props = dom.get(description).map(|d| d.properties());
    let named = |key: &str| props.and_then(|p| rgb(p.get(key)));
    let base = BodyColors::default();
    BodyColors {
        head: slots[0].or_else(|| named("HeadColor")).unwrap_or(base.head),
        torso: slots[1]
            .or_else(|| named("TorsoColor"))
            .unwrap_or(base.torso),
        left_arm: slots[2]
            .or_else(|| named("LeftArmColor"))
            .unwrap_or(base.left_arm),
        right_arm: slots[3]
            .or_else(|| named("RightArmColor"))
            .unwrap_or(base.right_arm),
        left_leg: slots[4]
            .or_else(|| named("LeftLegColor"))
            .unwrap_or(base.left_leg),
        right_leg: slots[5]
            .or_else(|| named("RightLegColor"))
            .unwrap_or(base.right_leg),
    }
}

impl rbx_sim::Avatars for StudioAvatars {
    fn description(&self, user_id: i64) -> Result<WeakDom, String> {
        let user = u64::try_from(user_id).map_err(|_| format!("User {user_id} has no avatar"))?;
        let fetched = match self.online(user) {
            Ok(fetched) => fetched,
            Err(why) => self.cached(user).ok_or(why)?,
        };
        description_of(&fetched)
    }

    fn outfit(&self, outfit_id: i64) -> Result<WeakDom, String> {
        let id = u64::try_from(outfit_id).map_err(|_| format!("Outfit {outfit_id} not found"))?;
        let avatar = self.client.outfit(id).map_err(|err| err.to_string())?;
        self.learn(&avatar);
        description_of(&gather(avatar, |id| self.download(id)))
    }

    fn character(&self, description: &WeakDom) -> Result<WeakDom, String> {
        let root = *description.root_refs().first().ok_or("no description")?;
        let avatar = self.avatar_of(description, root);
        let fetched = gather(avatar, |id| self.download(id));
        Ok(build(&fetched, Some(colors_of(description, root))))
    }
}

/// The `HumanoidDescription` our rig dressed as `fetched` gets, alone in a
/// DOM.
fn description_of(fetched: &Fetched) -> Result<WeakDom, String> {
    let model = build(fetched, None);
    let rig = *model.root_refs().first().ok_or("no rig was built")?;
    let description = find(&model, rig, "Humanoid")
        .and_then(|humanoid| find(&model, humanoid, "HumanoidDescription"))
        .ok_or("the rig has no HumanoidDescription")?;
    let mut out = WeakDom::new();
    rbx_sim::players::graft(&mut out, &model, description, None);
    Ok(out)
}

fn find(dom: &WeakDom, parent: Ref, name: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))
}
