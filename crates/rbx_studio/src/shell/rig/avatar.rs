//! "My Avatar" and "Player": a Roblox user's body scales, colours and worn
//! assets, laid over a rig `build_rig` makes of them.
//!
//! The public avatar endpoint and the asset delivery service need neither
//! cookie nor key, only a user id, so any player can be imported with no
//! stored API key. "My Avatar" uses the key just to learn whose id that is.
//!
//! What is applied:
//! - hats, hair, face and layered accessories (asset types 8, 41-47, 64-72),
//!   welded on at the rig's matching attachment as Studio's `AccessoryWeld`
//!   does (a layered one keeps its `WrapLayer`);
//! - shirts, pants and T-shirts, onto the rig's own `Shirt`/`Pants`;
//! - R15 body-part packages (27-31) and the dynamic head (79), which replace
//!   the stock meshes, cages, textures and attachments;
//! - R6 body-part packages, as their `CharacterMesh`es;
//! - animation packages (48, 50-55, 61), into the `Animate` script.
//!
//! `RBX_STUDIO_RIG_AVATAR_MOCK` stands in for the network. Pointing it at a
//! file uses that avatar JSON for every import. Pointing it at a directory
//! reads `<UserId>.json` (`me.json` for My Avatar) from it; a
//! `<UserId>.status` file holding `404`, `429` or `network` makes that import
//! fail that way instead. Each worn asset is `<id>.rbxm`/`.rbxmx` beside it.

use std::collections::BTreeMap;
use std::path::Path;

use rbx_cloud::{ApiKey, Avatar, AvatarAsset, Client, CloudError, Grant, KeyedRefusal};
use rbx_dom::{BrickColor, Content, Ref, Variant, WeakDom, DEFAULT_BRICK_COLOR};

use crate::shell::clipboard;
use crate::shell::freeze::{authorize, describe, read_model};

use super::bundle::{self, Family, Piece};
use super::cframe::{Cf, V3};
use super::{animate, r15};
use super::{BodyColors, BodyScale, BodyShape, JointStyle, RigOptions, RigType, Scales};

pub(crate) const MOCK_VARIABLE: &str = "RBX_STUDIO_RIG_AVATAR_MOCK";

const TSHIRT: u32 = 2;
const SHIRT: u32 = 11;
const PANTS: u32 = 12;
const FACE: u32 = 18;
const DYNAMIC_HEAD: u32 = 79;
const MOOD: u32 = 78;

/// One worn asset and the model downloaded for it.
pub(crate) struct Worn {
    pub(crate) asset: AvatarAsset,
    pub(crate) dom: Result<WeakDom, String>,
}

pub(crate) struct Fetched {
    pub(crate) avatar: Avatar,
    pub(crate) worn: Vec<Worn>,
}

fn is_accessory(kind: u32) -> bool {
    kind == 8 || (41..=47).contains(&kind) || (64..=72).contains(&kind) || matches!(kind, 76 | 77)
}

fn is_clothing(kind: u32) -> bool {
    matches!(kind, TSHIRT | SHIRT | PANTS)
}

/// Face, lip and eye makeup: decals in the head's own UVs.
fn is_makeup(kind: u32) -> bool {
    (88..=90).contains(&kind)
}

fn is_body_part(kind: u32) -> bool {
    (27..=31).contains(&kind)
}

fn is_animation(kind: u32) -> bool {
    matches!(kind, 48 | 50..=55 | 61)
}

fn is_used(kind: u32) -> bool {
    is_accessory(kind)
        || is_clothing(kind)
        || is_body_part(kind)
        || is_animation(kind)
        || is_makeup(kind)
        || matches!(kind, FACE | DYNAMIC_HEAD)
}

/// What became of one worn asset.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Fate {
    Used,
    /// Worn, but not as Roblox draws it: the line says how it differs.
    Approximated(String),
    Left(String),
    /// Not this step's business: another one decides.
    Elsewhere,
}

impl Fate {
    fn used(ok: bool, why: &str) -> Fate {
        if ok {
            Fate::Used
        } else {
            Fate::Left(why.into())
        }
    }
}

/// The rig this avatar's scales and colours call for, of its own type or of
/// `rig_type` when the caller overrides it (its items are then built onto
/// the other body).
pub(crate) fn options_for(
    avatar: &Avatar,
    joints: JointStyle,
    feet: V3,
    rig_type: Option<RigType>,
) -> RigOptions {
    let rig_type = rig_type.unwrap_or_else(|| own_type(avatar));
    let mut options = RigOptions::new(rig_type, BodyShape::Masculine, BodyScale::Classic, joints);
    let s = &avatar.scales;
    options.set_scales(Scales {
        height: s.height as f32,
        width: s.width as f32,
        depth: s.depth as f32,
        head: s.head as f32,
        body_type: s.body_type as f32,
        proportion: s.proportion as f32,
    });
    let c = &avatar.body_colors;
    let color = |id: u32| {
        BrickColor::from_number(id)
            .or_else(|| BrickColor::from_number(DEFAULT_BRICK_COLOR))
            .map_or([163, 162, 165], |b| b.rgb)
    };
    options.colors = BodyColors {
        head: color(c.head),
        torso: color(c.torso),
        left_arm: color(c.left_arm),
        right_arm: color(c.right_arm),
        left_leg: color(c.left_leg),
        right_leg: color(c.right_leg),
    };
    options.feet = feet;
    options
}

/// The rig type the avatar itself uses.
pub(crate) fn own_type(avatar: &Avatar) -> RigType {
    if avatar.avatar_type.eq_ignore_ascii_case("R6") {
        RigType::R6
    } else {
        RigType::R15
    }
}

/// Blocking: the avatar of `user` (`None` is the signed-in user) and every
/// asset it wears that [`dress`] and [`apply_packages`] can use.
pub(crate) fn fetch(user: Option<u64>) -> Result<Fetched, String> {
    if let Ok(mock) = std::env::var(MOCK_VARIABLE) {
        return mock_fetch(&mock, user);
    }
    // The avatar and most assets are public, but an asset the anonymous
    // route refuses (HTTP 401) is tried through the stored Open Cloud key
    // ([`Client::asset`] falls back to it), for Player as for My Avatar.
    let (client, id, keyed) = match user {
        Some(id) => {
            let key = ApiKey::from_env_or_config();
            let keyed = key.is_some();
            (Client::new(key), id, keyed)
        }
        None => {
            let (client, id) = authorize()?;
            (client, id, true)
        }
    };
    let avatar = client.avatar(id).map_err(|err| user_error(&err, id))?;
    // Asked once, and only after a keyed refusal: what the key grants.
    let grant = std::cell::OnceCell::new();
    ready(avatar, id, |asset| download(&client, asset, keyed, &grant))
}

const DOWNLOAD_ATTEMPTS: u32 = 3;

/// One asset's bytes. Only a failure that may pass (network, rate limit,
/// server error) is retried: a 401 or 404 will answer the same next time.
fn download(
    client: &Client,
    id: u64,
    keyed: bool,
    grant: &std::cell::OnceCell<Option<Grant>>,
) -> Result<Vec<u8>, String> {
    let mut attempt = 1;
    loop {
        match client.asset(id) {
            Ok(content) => return Ok(content.bytes),
            Err(err) if attempt < DOWNLOAD_ATTEMPTS && is_transient(&err) => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            Err(err) => {
                let legacy = matches!(err, CloudError::KeyedAssetRefused { .. }).then(|| {
                    grant.get_or_init(|| {
                        let info = client.introspect().ok()?;
                        let report = rbx_cloud::check_scopes(&info);
                        report
                            .checks
                            .into_iter()
                            .find(|c| c.permission.scope == LEGACY_ASSET)
                            .map(|c| c.grant)
                    })
                });
                return Err(download_error(&err, keyed, legacy.and_then(Option::as_ref)));
            }
        }
    }
}

fn is_transient(err: &CloudError) -> bool {
    matches!(
        err,
        CloudError::Transport(_)
            | CloudError::RateLimited { .. }
            | CloudError::Http { status: 500.., .. }
    )
}

const LEGACY_ASSET: &str = "legacy-asset:manage";
const KEY_PAGE: &str = "Creator Hub \u{203a} Open Cloud \u{203a} API Keys";

/// Why `err` ended a download, in words that say what to change. `legacy` is
/// what the stored key grants for [`LEGACY_ASSET`] when that is known.
fn download_error(err: &CloudError, keyed: bool, legacy: Option<&Grant>) -> String {
    match err {
        CloudError::AuthRequired { .. } => format!(
            "HTTP 401: Roblox serves this only to a signed-in account, and no Open Cloud key is stored (Home \u{203a} Manage key; it needs {LEGACY_ASSET})"
        ),
        CloudError::KeyedAssetRefused {
            status, why, detail, ..
        } => {
            let add = format!("add {LEGACY_ASSET} at {KEY_PAGE} (edit the key, add the legacy-asset system, operation Manage)");
            match (legacy, why) {
                (Some(Grant::Missing), _) | (None, KeyedRefusal::Scope) => {
                    format!("HTTP {status}: the stored key lacks the {LEGACY_ASSET} permission: {add}")
                }
                (Some(Grant::Universes(_)), KeyedRefusal::Scope | KeyedRefusal::NoAccess) => format!(
                    "HTTP {status}: the stored key grants {LEGACY_ASSET} only for specific experiences, which do not cover this asset: edit the key at {KEY_PAGE} to allow it for every experience"
                ),
                (_, KeyedRefusal::InvalidKey) => format!(
                    "HTTP {status}: Roblox rejected the stored key ({detail}); it may be revoked, expired or limited to another IP address: check it at {KEY_PAGE}"
                ),
                _ => format!(
                    "HTTP {status}: not publicly downloadable, and the stored key\u{2019}s account may not read it: Roblox releases clothing and items only to accounts that own or created them ({detail})"
                ),
            }
        }
        CloudError::Http {
            status: status @ (401 | 403),
            ..
        } if keyed => format!(
            "HTTP {status}: not publicly downloadable, and the stored Open Cloud key\u{2019}s account may not read it either"
        ),
        CloudError::Http { status: 404, .. } => "HTTP 404: deleted or moderated".into(),
        CloudError::RateLimited { .. } => "HTTP 429: Roblox is rate limiting downloads".into(),
        other => other.to_string(),
    }
}

fn mock_fetch(mock: &str, user: Option<u64>) -> Result<Fetched, String> {
    let path = Path::new(mock);
    let id = user.unwrap_or(0);
    let (file, dir) = if path.is_dir() {
        let key = user.map_or_else(|| "me".to_string(), |id| id.to_string());
        if let Ok(status) = std::fs::read_to_string(path.join(format!("{key}.status"))) {
            return Err(user_error(&simulated(status.trim()), id));
        }
        (path.join(format!("{key}.json")), path)
    } else {
        (path.to_path_buf(), path.parent().unwrap_or(Path::new(".")))
    };
    let body = std::fs::read(&file).map_err(|err| {
        if path.is_dir() {
            user_error(&simulated("404"), id)
        } else {
            format!("{mock}: {err}")
        }
    })?;
    let avatar = Avatar::from_json(&body).map_err(|err| user_error(&err, id))?;
    ready(avatar, id, |asset| {
        // `<asset>.status` fails that one download as the client would.
        if let Ok(status) = std::fs::read_to_string(dir.join(format!("{asset}.status"))) {
            // The anonymous route's 401 is `AuthRequired`, not a plain `Http`.
            let failure = match status.trim() {
                "401" => CloudError::AuthRequired { asset_id: asset },
                other => simulated(other),
            };
            return Err(download_error(&failure, false, None));
        }
        ["rbxm", "rbxmx"]
            .iter()
            .find_map(|ext| std::fs::read(dir.join(format!("{asset}.{ext}"))).ok())
            .ok_or_else(|| format!("asset {asset}: no {asset}.rbxm beside {mock}"))
    })
}

/// The failure a `<UserId>.status` mock file asks for.
fn simulated(status: &str) -> CloudError {
    match status {
        "429" => CloudError::RateLimited {
            retry_after: Some(30),
        },
        "network" => CloudError::Transport("simulated network failure".into()),
        other => CloudError::Http {
            status: other.parse().unwrap_or(404),
            url: "mock".into(),
        },
    }
}

/// An avatar worth inserting: one that wears something. An empty one is a
/// private or never-dressed account, and a rig of defaults would pass for it.
fn ready(
    avatar: Avatar,
    user: u64,
    download: impl Fn(u64) -> Result<Vec<u8>, String>,
) -> Result<Fetched, String> {
    if avatar.assets.is_empty() {
        let whose = if user == 0 {
            "this avatar".to_string()
        } else {
            format!("user {user}\u{2019}s avatar")
        };
        return Err(format!(
            "{whose} wears no assets, so there is nothing to import (Roblox gives the same empty avatar to ids that don't exist, private avatars and never-dressed accounts)"
        ));
    }
    Ok(gather(avatar, download))
}

fn gather(avatar: Avatar, download: impl Fn(u64) -> Result<Vec<u8>, String>) -> Fetched {
    let worn = avatar
        .assets
        .iter()
        .filter(|a| is_used(a.asset_type.id))
        .map(|asset| Worn {
            asset: asset.clone(),
            dom: download(asset.id).and_then(|bytes| {
                read_model(&bytes).map_err(|e| format!("not a readable model: {e}"))
            }),
        })
        .collect();
    Fetched { avatar, worn }
}

/// The message for a failed avatar request.
fn user_error(err: &CloudError, user: u64) -> String {
    match err {
        CloudError::Http {
            status: 400 | 404, ..
        }
        | CloudError::Refused {
            status: 400 | 404, ..
        } => format!("No Roblox user has the id {user}"),
        CloudError::Http {
            status: status @ 500..,
            ..
        } => format!(
            "Roblox has no avatar for user {user} (HTTP {status}): the id may not exist, or the account may be banned"
        ),
        CloudError::RateLimited { retry_after } => format!(
            "Roblox is rate limiting avatar requests (HTTP 429){}. Wait a moment and insert again",
            retry_after.map_or_else(String::new, |s| format!(", retry in {s}s"))
        ),
        CloudError::Transport(message) => {
            format!("Couldn\u{2019}t reach Roblox ({message}). Check your connection")
        }
        CloudError::Json(_) | CloudError::UnexpectedShape(_) => format!(
            "Roblox sent an avatar for user {user} that can\u{2019}t be read ({err}); the account may be banned or private"
        ),
        other => describe(other),
    }
}

/// How many worn items were used, and one line for every avatar asset that
/// was not (name, id, type, reason), so nothing is dropped silently. `fates`
/// is [`merge`]d from [`apply_packages`] and [`dress`], one per `worn` entry.
pub(crate) fn settle(avatar: &Avatar, worn: &[Worn], fates: &[Fate]) -> (usize, Vec<String>) {
    let (mut used, mut notes, mut next) = (0, Vec::new(), 0);
    for asset in &avatar.assets {
        let kind = asset.asset_type.id;
        let why = if is_used(kind) {
            next += 1;
            match (&worn[next - 1].dom, &fates[next - 1]) {
                (Err(err), _) if kind == DYNAMIC_HEAD => {
                    Some(format!("{err}; the rig keeps its stock head and face"))
                }
                (Err(err), _) => Some(err.clone()),
                (_, Fate::Left(why)) => Some(why.clone()),
                (_, Fate::Elsewhere) => Some("no step of the import took it".into()),
                (_, Fate::Used) => None,
                (_, Fate::Approximated(how)) => {
                    notes.push(format!(
                        "{} (id {}, {}): {how}",
                        asset.name, asset.id, asset.asset_type.name
                    ));
                    None
                }
            }
        } else if kind == MOOD {
            Some("a face mood animation: it drives the dynamic head\u{2019}s FaceControls through an Animator, which the editor does not run, so the head keeps its default expression".into())
        } else {
            Some(format!(
                "the rig has no place for a {}",
                asset.asset_type.name
            ))
        };
        match why {
            None => used += 1,
            Some(why) => notes.push(format!(
                "{} (id {}, {}): {why}",
                asset.name, asset.id, asset.asset_type.name
            )),
        }
    }
    (used, notes)
}

/// The step that took each asset, from the two that may.
pub(crate) fn merge(early: Vec<Fate>, late: Vec<Fate>) -> Vec<Fate> {
    early
        .into_iter()
        .zip(late)
        .map(|(a, b)| if a == Fate::Elsewhere { b } else { a })
        .collect()
}

/// Swaps the stock R15 meshes for the avatar's own body-part packages and
/// dynamic head. An R6 rig has none, and leaves them to [`dress`].
pub(crate) fn apply_packages(options: &mut RigOptions, worn: &[Worn]) -> Vec<Fate> {
    worn.iter()
        .map(|w| {
            let (kind, Ok(dom)) = (w.asset.asset_type.id, &w.dom) else {
                return Fate::Elsewhere;
            };
            if options.rig_type != RigType::R15 {
                Fate::Elsewhere
            } else if is_body_part(kind) {
                let pieces = pieces_in(dom);
                let why = if has_class(dom, "CharacterMesh") {
                    "an R6 package (CharacterMeshes); the R15 rig takes R15 parts"
                } else {
                    "no R15 part in the package carries every rig attachment the stock part has"
                };
                let ok = !pieces.is_empty();
                options.pieces.extend(pieces);
                Fate::used(ok, why)
            } else if kind == DYNAMIC_HEAD {
                Fate::used(
                    dynamic_head(options, dom),
                    "no head mesh found in the model",
                )
            } else {
                Fate::Elsewhere
            }
        })
        .collect()
}

fn has_class(dom: &WeakDom, class: &str) -> bool {
    find_of(dom, dom.root_refs(), &[class]).is_some()
}

/// Every R15 part a package supplies whole: its mesh and every rig attachment
/// the stock part has, or the joints would not meet.
fn pieces_in(dom: &WeakDom) -> BTreeMap<String, Piece> {
    let mut found: BTreeMap<String, (bool, Piece)> = BTreeMap::new();
    for root in dom.root_refs() {
        for node in descendants(dom, *root) {
            let Some(piece) = piece_of(dom, node) else {
                continue;
            };
            let Some(stock) = bundle::piece(Family::Classic, BodyShape::Masculine, &piece.name)
            else {
                continue;
            };
            if !stock
                .rig
                .iter()
                .all(|(n, _)| piece.rig.iter().any(|(p, _)| p == n))
            {
                continue;
            }
            // Packages carry an artist-intent and a "fixed" copy of each part.
            let fixed = ancestor_named(dom, node, "R15Fixed");
            if found.get(&piece.name).is_none_or(|(was, _)| fixed && !was) {
                found.insert(piece.name.clone(), (fixed, piece));
            }
        }
    }
    found.into_iter().map(|(k, (_, p))| (k, p)).collect()
}

fn piece_of(dom: &WeakDom, part: Ref) -> Option<Piece> {
    let node = dom.get(part)?;
    if node.class() != "MeshPart" || !r15::PARTS.contains(&node.name()) {
        return None;
    }
    let init = match node
        .properties()
        .get("InitialSize")
        .or_else(|| node.properties().get("size"))
    {
        Some(Variant::Vector3(v)) if v.x > 0. && v.y > 0. && v.z > 0. => [v.x, v.y, v.z],
        _ => return None,
    };
    let (mut rig, mut extra) = (Vec::new(), Vec::new());
    let mut cage = None;
    for &child in node.children() {
        let Some(c) = dom.get(child) else { continue };
        match c.class() {
            "Attachment" => {
                let entry = (c.name().to_owned(), cframe_of(dom, child).p);
                if c.name().ends_with("RigAttachment") {
                    rig.push(entry);
                } else {
                    extra.push(entry);
                }
            }
            "WrapTarget" => {
                cage = content_id(dom, child, "CageMeshId")
                    .map(|id| (id, cframe_of_property(dom, child, "CageOrigin").p));
            }
            _ => {}
        }
    }
    Some(Piece {
        name: node.name().to_owned(),
        mesh: content_id(dom, part, "MeshId")?,
        texture: content_id(dom, part, "TextureID"),
        surface: None,
        face: true,
        original: init,
        family: bundle::Family::Classic,
        init,
        cage,
        rig,
        extra,
    })
}

/// Points the Head at the dynamic head's mesh and texture.
fn dynamic_head(options: &mut RigOptions, dom: &WeakDom) -> bool {
    let Some((_, mesh)) = find_of(dom, dom.root_refs(), &["SpecialMesh", "MeshPart"]) else {
        return false;
    };
    let (Some(id), Some(head)) = (
        content_id(dom, mesh, "MeshId"),
        options.pieces.get_mut("Head"),
    ) else {
        return false;
    };
    head.mesh = id;
    head.face = false;
    head.texture =
        content_id(dom, mesh, "TextureId").or_else(|| content_id(dom, mesh, "TextureID"));
    true
}

/// The asset number in `rbxassetid://N` and `…/asset/?id=N` alike.
fn asset_id(text: &str) -> Option<u64> {
    text.split(|c: char| !c.is_ascii_digit())
        .rfind(|t| !t.is_empty())?
        .parse()
        .ok()
        .filter(|&id| id != 0)
}

fn content_id(dom: &WeakDom, node: Ref, key: &str) -> Option<u64> {
    match dom.get(node)?.properties().get(key)? {
        Variant::Content(Content::Uri(uri)) => asset_id(uri),
        _ => None,
    }
}

fn ancestor_named(dom: &WeakDom, node: Ref, name: &str) -> bool {
    let mut at = dom.parent(node);
    while let Some(parent) = at {
        if dom.get(parent).is_some_and(|i| i.name() == name) {
            return true;
        }
        at = dom.parent(parent);
    }
    false
}

/// Puts every downloaded accessory, piece of clothing, face, R6 body part and
/// animation on `rig`. R15 packages and heads are [`apply_packages`]'
/// business, done before the rig is built.
pub(crate) fn dress(dom: &mut WeakDom, rig: Ref, worn: &[Worn]) -> Vec<Fate> {
    let r15 = child_named(dom, rig, "UpperTorso").is_some();
    worn.iter()
        .map(|w| {
            let (kind, Ok(source)) = (w.asset.asset_type.id, &w.dom) else {
                return Fate::Elsewhere;
            };
            let result = if is_accessory(kind) {
                wear_accessory(dom, rig, source, kind, r15)
            } else if is_clothing(kind) {
                wear_clothing(dom, rig, source).map(|()| None)
            } else if kind == FACE {
                wear_face(dom, rig, source).map(|()| None)
            } else if is_makeup(kind) {
                wear_makeup(dom, rig, source, r15).map(|()| None)
            } else if is_animation(kind) {
                if animate::replace(dom, rig, source) > 0 {
                    Ok(None)
                } else {
                    Err("no animation in it matches a state of the Animate script".into())
                }
            } else if is_body_part(kind) && !r15 {
                wear_character_meshes(dom, rig, source).map(|()| None)
            } else if kind == DYNAMIC_HEAD && !r15 {
                Err("a dynamic head is R15-only; the R6 rig keeps its block head".into())
            } else if kind == DYNAMIC_HEAD {
                neutral_face(dom, rig).map(|()| None)
            } else {
                return Fate::Elsewhere;
            };
            match result {
                Ok(None) => Fate::Used,
                Ok(Some(how)) => Fate::Approximated(how),
                Err(why) => Fate::Left(why),
            }
        })
        .collect()
}

fn descendants(dom: &WeakDom, root: Ref) -> Vec<Ref> {
    let mut found = Vec::new();
    let mut pending = vec![root];
    while let Some(next) = pending.pop() {
        let Some(instance) = dom.get(next) else {
            continue;
        };
        found.push(next);
        pending.extend(instance.children());
    }
    found
}

fn find_of(dom: &WeakDom, roots: &[Ref], class: &[&str]) -> Option<(Ref, Ref)> {
    roots.iter().find_map(|&root| {
        descendants(dom, root)
            .into_iter()
            .find(|&r| dom.get(r).is_some_and(|i| class.contains(&i.class())))
            .map(|found| (root, found))
    })
}

fn cframe_of(dom: &WeakDom, node: Ref) -> Cf {
    cframe_of_property(dom, node, "CFrame")
}

fn cframe_of_property(dom: &WeakDom, node: Ref, key: &str) -> Cf {
    match dom.get(node).and_then(|i| i.properties().get(key)) {
        Some(Variant::CFrame(data)) => Cf::from_data(data),
        _ => Cf::at([0.; 3]),
    }
}

/// Shirts and pants go into the rig's own `Shirt` and `Pants`; a T-shirt is
/// a `ShirtGraphic` the rig does not have, so it is copied in.
fn wear_clothing(rig_dom: &mut WeakDom, rig: Ref, source: &WeakDom) -> Result<(), String> {
    let (_, item) = find_of(
        source,
        source.root_refs(),
        &["Shirt", "Pants", "ShirtGraphic"],
    )
    .ok_or("no Shirt, Pants or ShirtGraphic in the model")?;
    let instance = source.get(item).ok_or("unreadable model")?;
    let template = match instance.class() {
        "Shirt" => "ShirtTemplate",
        "Pants" => "PantsTemplate",
        _ => "Graphic",
    };
    let value = instance
        .properties()
        .get(template)
        .ok_or("the clothing has no texture")?;
    let value = normalised_template(value)?;
    let own = child_of_class(rig_dom, rig, instance.class())
        .unwrap_or_else(|| rig_dom.new_instance(instance.class(), instance.class(), Some(rig)));
    set(rig_dom, own, template, value);
    Ok(())
}

/// Clothing carries the legacy `http://www.roblox.com/asset/?id=N ` form
/// (sometimes with a trailing space); the rig stores `rbxassetid://N`.
fn normalised_template(value: &Variant) -> Result<Variant, String> {
    let Variant::Content(Content::Uri(text)) = value else {
        return Ok(value.clone());
    };
    match rbx_assets::AssetRef::parse(text) {
        Ok(rbx_assets::AssetRef::Id(id)) => {
            Ok(Variant::Content(Content::Uri(format!("rbxassetid://{id}"))))
        }
        Ok(rbx_assets::AssetRef::Empty) => Err("the clothing has an empty texture".into()),
        _ => Ok(value.clone()),
    }
}

/// A classic face is a `Decal`; it replaces the texture of the Head's own.
fn wear_face(dom: &mut WeakDom, rig: Ref, source: &WeakDom) -> Result<(), String> {
    let (_, decal) =
        find_of(source, source.root_refs(), &["Decal"]).ok_or("no Decal in the model")?;
    let texture = source
        .get(decal)
        .and_then(|d| d.properties().get("Texture"))
        .ok_or("the face has no texture")?
        .clone();
    let head = child_named(dom, rig, "Head").ok_or("the rig has no Head")?;
    let own = child_of_class(dom, head, "Decal")
        .unwrap_or_else(|| dom.new_instance("Decal", "face", Some(head)));
    set(dom, own, "Texture", texture);
    Ok(())
}

/// A dynamic head is posed by a `FaceControls` on the Head; every FACS
/// property left unset is its neutral 0, which is the face the head ships with.
fn neutral_face(dom: &mut WeakDom, rig: Ref) -> Result<(), String> {
    let head = child_named(dom, rig, "Head").ok_or("the rig has no Head")?;
    if child_of_class(dom, head, "FaceControls").is_none() {
        dom.new_instance("FaceControls", "FaceControls", Some(head));
    }
    Ok(())
}

/// An R6 body-part package is a set of `CharacterMesh`es.
fn wear_character_meshes(dom: &mut WeakDom, rig: Ref, source: &WeakDom) -> Result<(), String> {
    let meshes: Vec<Ref> = source
        .root_refs()
        .iter()
        .flat_map(|&root| descendants(source, root))
        .filter(|&r| source.get(r).is_some_and(|i| i.class() == "CharacterMesh"))
        .collect();
    let worn = meshes
        .iter()
        .filter(|&&mesh| clipboard::graft(dom, source, mesh, rig).is_some())
        .count();
    if worn > 0 {
        Ok(())
    } else {
        Err("an R15 package (parts, not CharacterMeshes); the R6 rig has block parts".into())
    }
}

/// Layered clothing (asset types 64 to 72) carries a `WrapLayer` its mesh is
/// fitted by; every other accessory is rigid.
const LAYERED: std::ops::RangeInclusive<u32> = 64..=72;

const LAYERED_NOTE: &str = "layered clothing: Roblox deforms it to the body through its cage; here it is placed undeformed, which fits the default-proportioned rig and drifts on a scaled or custom body";

fn wear_accessory(
    dom: &mut WeakDom,
    rig: Ref,
    source: &WeakDom,
    kind: u32,
    r15: bool,
) -> Result<Option<String>, String> {
    let layered = LAYERED.contains(&kind);
    if layered && !r15 {
        return Err("layered clothing is fitted to the R15 body cage; the R6 rig has none".into());
    }
    let (_, accessory) = find_of(source, source.root_refs(), &["Accessory", "Hat"])
        .ok_or("no Accessory or Hat in the model")?;
    let worn = clipboard::graft(dom, source, accessory, rig).ok_or("could not copy it in")?;
    let wrap = child_named(dom, worn, "Handle")
        .and_then(|handle| child_of_class(dom, handle, "WrapLayer"));
    let welded = match wrap {
        Some(wrap) => weld_layered(dom, rig, worn, wrap, kind),
        None => weld_accessory(dom, rig, worn),
    };
    welded
        .inspect_err(|_| {
            dom.remove(worn);
        })
        .map(|()| layered.then(|| LAYERED_NOTE.to_owned()))
}

/// The body part a layered garment is cut for.
fn layered_part(kind: u32) -> &'static str {
    match kind {
        66 | 69 | 72 => "LowerTorso",
        70 => "LeftFoot",
        71 => "RightFoot",
        _ => "UpperTorso",
    }
}

/// Welds a layered garment the way Roblox seats it: its mesh is modelled in
/// the cage's body space, `ReferenceOrigin` away from the body part it is cut
/// for. A garment with an attachment of its own (shoes carry a foot one) goes
/// by that, like any accessory.
fn weld_layered(
    dom: &mut WeakDom,
    rig: Ref,
    worn: Ref,
    wrap: Ref,
    kind: u32,
) -> Result<(), String> {
    let handle = child_named(dom, worn, "Handle").ok_or("it has no Handle")?;
    if child_of_class(dom, handle, "Attachment").is_some() && weld_accessory(dom, rig, worn).is_ok()
    {
        return Ok(());
    }
    let name = layered_part(kind);
    let part = child_named(dom, rig, name).ok_or_else(|| format!("the rig has no {name}"))?;
    let reference = cframe_of_property(dom, wrap, "ReferenceOrigin");
    let at = cframe_of(dom, part).mul(&reference);
    set(dom, handle, "CFrame", Variant::CFrame(at.data()));
    let weld = dom.new_instance("Weld", "AccessoryWeld", Some(handle));
    set(dom, weld, "Part0", Variant::Ref(handle));
    set(dom, weld, "Part1", Variant::Ref(part));
    set(dom, weld, "C0", Variant::CFrame(reference.inverse().data()));
    set(dom, weld, "C1", Variant::CFrame(Cf::at([0.; 3]).data()));
    Ok(())
}

/// Lays makeup over the rig's head: the image is in the head's UVs, so the
/// `Decal` keeps its `WrapTextureTransfer` child and the viewer paints it into
/// the head's colour map rather than projecting it.
fn wear_makeup(dom: &mut WeakDom, rig: Ref, source: &WeakDom, r15: bool) -> Result<(), String> {
    if !r15 {
        return Err(
            "makeup is painted in the R15 head\u{2019}s UVs; the R6 head is a block".into(),
        );
    }
    let decal = descendants(source, source.root_refs()[0])
        .into_iter()
        .chain(source.root_refs().iter().copied())
        .find(|&r| {
            source.get(r).is_some_and(|i| {
                i.class() == "Decal"
                    && i.children().iter().any(|&c| {
                        source
                            .get(c)
                            .is_some_and(|c| c.class() == "WrapTextureTransfer")
                    })
            })
        })
        .ok_or("no Decal with a WrapTextureTransfer in the model")?;
    let head = child_named(dom, rig, "Head").ok_or("the rig has no Head")?;
    clipboard::graft(dom, source, decal, head).ok_or("could not copy it in")?;
    Ok(())
}

/// Welds the grafted accessory to the rig attachment named like its own.
fn weld_accessory(dom: &mut WeakDom, rig: Ref, worn: Ref) -> Result<(), String> {
    let handle = child_named(dom, worn, "Handle").ok_or("it has no Handle")?;
    // A legacy hat has no attachment of its own and sits on the head.
    let own = child_of_class(dom, handle, "Attachment");
    let wanted = own
        .and_then(|a| dom.get(a).map(|i| i.name().to_owned()))
        .unwrap_or_else(|| "HatAttachment".into());
    let on_accessory = descendants(dom, worn);
    let body = descendants(dom, rig)
        .into_iter()
        .find(|r| {
            !on_accessory.contains(r)
                && dom
                    .get(*r)
                    .is_some_and(|i| i.class() == "Attachment" && i.name() == wanted)
        })
        .ok_or_else(|| format!("the rig has no {wanted} to wear it on"))?;
    let part = dom.parent(body).ok_or("the attachment has no part")?;
    let c0 = own.map_or(Cf::at([0.; 3]), |a| cframe_of(dom, a));
    let c1 = cframe_of(dom, body);
    let at = cframe_of(dom, part).joined(&c1, &c0);
    set(dom, handle, "CFrame", Variant::CFrame(at.data()));
    let weld = dom.new_instance("Weld", "AccessoryWeld", Some(handle));
    set(dom, weld, "Part0", Variant::Ref(handle));
    set(dom, weld, "Part1", Variant::Ref(part));
    set(dom, weld, "C0", Variant::CFrame(c0.data()));
    set(dom, weld, "C1", Variant::CFrame(c1.data()));
    Ok(())
}

fn set(dom: &mut WeakDom, node: Ref, key: &str, value: Variant) {
    if let Some(instance) = dom.get_mut(node) {
        instance.properties_mut().insert(key.into(), value);
    }
}

fn child_named(dom: &WeakDom, parent: Ref, name: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|&c| dom.get(c).is_some_and(|i| i.name() == name))
}

fn child_of_class(dom: &WeakDom, parent: Ref, class: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|&c| dom.get(c).is_some_and(|i| i.class() == class))
}

#[cfg(test)]
mod tests;

/// Fills the rig's `HumanoidDescription` with the ids and colours of the
/// avatar's JSON, as Studio's own character import records them.
pub(crate) fn describe_avatar(dom: &mut WeakDom, rig: Ref, avatar: &Avatar) {
    let Some(description) = child_named(dom, rig, "Humanoid")
        .and_then(|humanoid| child_named(dom, humanoid, "HumanoidDescription"))
    else {
        return;
    };
    let id_of = |kind: u32| {
        avatar
            .assets
            .iter()
            .find(|a| a.asset_type.id == kind)
            .map(|a| a.id as i64)
    };
    let mut set = |property: &str, value: Variant| {
        let _ = dom.set_property(description, property, value);
    };
    for (kind, property) in [
        (FACE, "Face"),
        (TSHIRT, "GraphicTShirt"),
        (SHIRT, "Shirt"),
        (PANTS, "Pants"),
        (48, "ClimbAnimation"),
        (50, "FallAnimation"),
        (51, "IdleAnimation"),
        (52, "JumpAnimation"),
        (MOOD, "MoodAnimation"),
        (53, "RunAnimation"),
        (54, "SwimAnimation"),
        (55, "WalkAnimation"),
    ] {
        if let Some(id) = id_of(kind) {
            set(property, Variant::Int64(id));
        }
    }
    if let Some(mood) = id_of(MOOD) {
        let controls = child_named(dom, rig, "Head")
            .and_then(|head| child_of_class(dom, head, "FaceControls"));
        if let Some(controls) = controls {
            let note = dom.new_instance("StringValue", "MoodAnimationId", Some(controls));
            let _ = dom.set_property(
                note,
                "Value",
                Variant::String(format!("rbxassetid://{mood}")),
            );
        }
    }
    let c = &avatar.body_colors;
    let slots = [
        (0, id_of(DYNAMIC_HEAD).or_else(|| id_of(17)), c.head),
        (1, id_of(27), c.torso),
        (2, id_of(29), c.left_arm),
        (3, id_of(28), c.right_arm),
        (4, id_of(30), c.left_leg),
        (5, id_of(31), c.right_leg),
    ];
    let parts: Vec<Ref> = dom
        .get(description)
        .map(|d| d.children().to_vec())
        .unwrap_or_default();
    for part in parts {
        let Some(slot) = dom
            .get(part)
            .and_then(|p| match p.properties().get("BodyPart") {
                Some(&Variant::Enum(e)) => Some(e as usize),
                _ => None,
            })
        else {
            continue;
        };
        let Some(&(_, id, color)) = slots.get(slot) else {
            continue;
        };
        let rgb = BrickColor::from_number(color)
            .or_else(|| BrickColor::from_number(DEFAULT_BRICK_COLOR))
            .map_or([163, 162, 165], |b| b.rgb);
        let _ = dom.set_property(
            part,
            "Color",
            Variant::Color3(rbx_dom::Color3Data {
                r: f32::from(rgb[0]) / 255.,
                g: f32::from(rgb[1]) / 255.,
                b: f32::from(rgb[2]) / 255.,
            }),
        );
        if let Some(id) = id {
            let _ = dom.set_property(part, "AssetId", Variant::Int64(id));
        }
    }
    let mut order: BTreeMap<u32, i32> = BTreeMap::new();
    for asset in &avatar.assets {
        let kind = asset.asset_type.id;
        let Some(accessory) = accessory_type(kind) else {
            continue;
        };
        let slot = order.entry(accessory).or_insert(0);
        *slot += 1;
        let node = dom.new_instance(
            "AccessoryDescription",
            "AccessoryDescription",
            Some(description),
        );
        let zero = rbx_dom::Vector3Data {
            x: 0.,
            y: 0.,
            z: 0.,
        };
        for (property, value) in [
            ("AccessoryType", Variant::Enum(accessory)),
            ("AssetId", Variant::Int64(asset.id as i64)),
            ("IsLayered", Variant::Bool((64..=72).contains(&kind))),
            ("Order", Variant::Int32(*slot)),
            ("Position", Variant::Vector3(zero)),
            ("Rotation", Variant::Vector3(zero)),
            ("Puffiness", Variant::Float32(1.)),
            (
                "Scale",
                Variant::Vector3(rbx_dom::Vector3Data {
                    x: 1.,
                    y: 1.,
                    z: 1.,
                }),
            ),
        ] {
            let _ = dom.set_property(node, property, value);
        }
    }
}

/// `Enum.AccessoryType` of an accessory asset type.
fn accessory_type(kind: u32) -> Option<u32> {
    Some(match kind {
        8 => 1,
        41 => 2,
        42 => 3,
        43 => 4,
        44 => 5,
        45 => 6,
        46 => 7,
        47 => 8,
        64 => 9,
        65 => 10,
        66 => 11,
        67 => 12,
        68 => 13,
        69 => 14,
        70 => 15,
        71 => 16,
        72 => 17,
        _ => return None,
    })
}
