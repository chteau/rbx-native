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

use rbx_cloud::{Avatar, AvatarAsset, Client, CloudError};
use rbx_dom::{BrickColor, Content, Ref, Variant, WeakDom};

use crate::shell::clipboard;
use crate::shell::freeze::{authorize, describe, fetch_asset, read_model};

use super::bundle::{self, Family, Piece};
use super::cframe::{Cf, V3};
use super::{animate, r15};
use super::{BodyColors, BodyScale, BodyShape, JointStyle, RigOptions, RigType, Scales};

pub(crate) const MOCK_VARIABLE: &str = "RBX_STUDIO_RIG_AVATAR_MOCK";

const TSHIRT: u32 = 2;
const SHIRT: u32 = 11;
const PANTS: u32 = 12;
const DYNAMIC_HEAD: u32 = 79;

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
    kind == 8 || (41..=47).contains(&kind) || (64..=72).contains(&kind)
}

fn is_clothing(kind: u32) -> bool {
    matches!(kind, TSHIRT | SHIRT | PANTS)
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
        || kind == DYNAMIC_HEAD
}

/// The rig this avatar's type, scales and colours call for.
pub(crate) fn options_for(avatar: &Avatar, joints: JointStyle, feet: V3) -> RigOptions {
    let rig_type = if avatar.avatar_type.eq_ignore_ascii_case("R6") {
        RigType::R6
    } else {
        RigType::R15
    };
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
    let color = |id: u32| BrickColor::from_number(id).map_or([163, 162, 165], |b| b.rgb);
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

/// Blocking: the avatar of `user` (`None` is the signed-in user) and every
/// asset it wears that [`dress`] and [`apply_packages`] can use.
pub(crate) fn fetch(user: Option<u64>) -> Result<Fetched, String> {
    if let Ok(mock) = std::env::var(MOCK_VARIABLE) {
        return mock_fetch(&mock, user);
    }
    let id = match user {
        Some(id) => id,
        None => authorize()?.1,
    };
    let client = Client::new(None);
    let avatar = client.avatar(id).map_err(|err| user_error(&err, id))?;
    ready(avatar, id, |asset| fetch_asset(&client, asset))
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
    let avatar = Avatar::from_json(&body).map_err(|err| user_error(&err.into(), id))?;
    ready(avatar, id, |asset| {
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
            dom: download(asset.id).and_then(|bytes| read_model(&bytes)),
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

/// What could not be used, one line each: failed downloads, and assets
/// (face parts, moods) no rig has a place for.
pub(crate) fn unapplied(avatar: &Avatar, worn: &[Worn]) -> Vec<String> {
    let mut notes: Vec<String> = worn
        .iter()
        .filter_map(|w| {
            w.dom
                .as_ref()
                .err()
                .map(|e| format!("{}: {e}", w.asset.name))
        })
        .collect();
    let r6 = avatar.avatar_type.eq_ignore_ascii_case("R6");
    let skipped: Vec<String> = avatar
        .assets
        .iter()
        .filter(|a| !is_used(a.asset_type.id) || (r6 && a.asset_type.id == DYNAMIC_HEAD))
        .map(|a| format!("{} ({})", a.name, a.asset_type.name))
        .collect();
    if !skipped.is_empty() {
        notes.push(format!(
            "Not applied, the rig has no place for them: {}",
            skipped.join(", ")
        ));
    }
    notes
}

/// Swaps the stock R15 meshes for the avatar's own body-part packages and
/// dynamic head; returns how many assets that used. An R6 rig has none.
pub(crate) fn apply_packages(options: &mut RigOptions, worn: &[Worn]) -> usize {
    if options.rig_type != RigType::R15 {
        return 0;
    }
    let mut used = 0;
    for w in worn {
        let (kind, Ok(dom)) = (w.asset.asset_type.id, &w.dom) else {
            continue;
        };
        if is_body_part(kind) {
            let pieces = pieces_in(dom);
            used += usize::from(!pieces.is_empty());
            options.pieces.extend(pieces);
        } else if kind == DYNAMIC_HEAD {
            used += usize::from(dynamic_head(options, dom));
        }
    }
    used
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

/// Puts every downloaded accessory, piece of clothing, R6 body part and
/// animation on `rig`; returns how many were used. R15 packages and heads
/// are [`apply_packages`]' business, done before the rig is built.
pub(crate) fn dress(dom: &mut WeakDom, rig: Ref, worn: &[Worn]) -> usize {
    let r15 = child_named(dom, rig, "UpperTorso").is_some();
    worn.iter()
        .filter_map(|w| Some((w.asset.asset_type.id, w.dom.as_ref().ok()?)))
        .filter(|&(kind, source)| {
            if is_accessory(kind) {
                wear_accessory(dom, rig, source)
            } else if is_clothing(kind) {
                wear_clothing(dom, rig, source)
            } else if is_animation(kind) {
                animate::replace(dom, rig, source) > 0
            } else if is_body_part(kind) && !r15 {
                wear_character_meshes(dom, rig, source)
            } else {
                false
            }
        })
        .count()
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
fn wear_clothing(rig_dom: &mut WeakDom, rig: Ref, source: &WeakDom) -> bool {
    let Some((_, item)) = find_of(
        source,
        source.root_refs(),
        &["Shirt", "Pants", "ShirtGraphic"],
    ) else {
        return false;
    };
    let Some(instance) = source.get(item) else {
        return false;
    };
    let template = match instance.class() {
        "Shirt" => "ShirtTemplate",
        "Pants" => "PantsTemplate",
        _ => return clipboard::graft(rig_dom, source, item, rig).is_some(),
    };
    let Some(value) = instance.properties().get(template) else {
        return false;
    };
    let own = child_of_class(rig_dom, rig, instance.class())
        .unwrap_or_else(|| rig_dom.new_instance(instance.class(), instance.class(), Some(rig)));
    set(rig_dom, own, template, value.clone());
    true
}

/// An R6 body-part package is a set of `CharacterMesh`es.
fn wear_character_meshes(dom: &mut WeakDom, rig: Ref, source: &WeakDom) -> bool {
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
    worn > 0
}

fn wear_accessory(dom: &mut WeakDom, rig: Ref, source: &WeakDom) -> bool {
    let Some((_, accessory)) = find_of(source, source.root_refs(), &["Accessory", "Hat"]) else {
        return false;
    };
    let Some(worn) = clipboard::graft(dom, source, accessory, rig) else {
        return false;
    };
    let Some(handle) = child_named(dom, worn, "Handle") else {
        return false;
    };
    // A legacy hat has no attachment of its own and sits on the head.
    let own = child_of_class(dom, handle, "Attachment");
    let wanted = own
        .and_then(|a| dom.get(a).map(|i| i.name().to_owned()))
        .unwrap_or_else(|| "HatAttachment".into());
    let on_accessory = descendants(dom, worn);
    let Some(body) = descendants(dom, rig).into_iter().find(|r| {
        !on_accessory.contains(r)
            && dom
                .get(*r)
                .is_some_and(|i| i.class() == "Attachment" && i.name() == wanted)
    }) else {
        return false;
    };
    let Some(part) = dom.parent(body) else {
        return false;
    };
    let c0 = own.map_or(Cf::at([0.; 3]), |a| cframe_of(dom, a));
    let c1 = cframe_of(dom, body);
    let at = cframe_of(dom, part).joined(&c1, &c0);
    set(dom, handle, "CFrame", Variant::CFrame(at.data()));
    let weld = dom.new_instance("Weld", "AccessoryWeld", Some(handle));
    set(dom, weld, "Part0", Variant::Ref(handle));
    set(dom, weld, "Part1", Variant::Ref(part));
    set(dom, weld, "C0", Variant::CFrame(c0.data()));
    set(dom, weld, "C1", Variant::CFrame(c1.data()));
    true
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
