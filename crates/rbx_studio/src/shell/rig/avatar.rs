//! "My Avatar": the signed-in user's body scales, colours and worn assets,
//! laid over a rig `build_rig` makes of them.
//!
//! The public avatar endpoint needs neither cookie nor key, only the user id
//! the stored Open Cloud key belongs to. Hats and accessories (asset types 8
//! and 41-47) are welded on at the rig's matching attachment, as Studio's
//! `AccessoryWeld`; shirts and pants are copied in. Body-part packages,
//! dynamic heads and animations are named in the result and not applied: the
//! rig is made of blocks, and no animation playback exists to use them.
//!
//! `RBX_STUDIO_RIG_AVATAR_MOCK=<avatar.json>` stands in for the network: the
//! JSON is the endpoint's body and each worn asset is `<id>.rbxm`/`.rbxmx`
//! beside it.

use std::path::Path;

use rbx_cloud::{Avatar, AvatarAsset};
use rbx_dom::{BrickColor, Ref, Variant, WeakDom};

use crate::shell::clipboard;
use crate::shell::freeze::{authorize, describe, fetch_asset, read_model};

use super::cframe::Cf;
use super::{BodyColors, BodyScale, BodyShape, JointStyle, RigOptions, RigType, Scales, V3};

pub(crate) const MOCK_VARIABLE: &str = "RBX_STUDIO_RIG_AVATAR_MOCK";

const SHIRT: u32 = 11;
const PANTS: u32 = 12;

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
    kind == 8 || (41..=47).contains(&kind)
}

fn is_clothing(kind: u32) -> bool {
    kind == SHIRT || kind == PANTS
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
    options.scales = Some(Scales {
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

/// Blocking: the avatar and every asset it wears that [`dress`] can use.
pub(crate) fn fetch() -> Result<Fetched, String> {
    if let Ok(mock) = std::env::var(MOCK_VARIABLE) {
        let path = Path::new(&mock);
        let body = std::fs::read(path).map_err(|err| format!("{mock}: {err}"))?;
        let avatar = Avatar::from_json(&body).map_err(|err| format!("{mock}: {err}"))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        return Ok(gather(avatar, |id| {
            ["rbxm", "rbxmx"]
                .iter()
                .find_map(|ext| std::fs::read(dir.join(format!("{id}.{ext}"))).ok())
                .ok_or_else(|| format!("asset {id}: no {id}.rbxm beside {mock}"))
        }));
    }
    let (client, user) = authorize()?;
    let avatar = client.avatar(user).map_err(|err| describe(&err))?;
    Ok(gather(avatar, |id| fetch_asset(&client, id)))
}

fn gather(avatar: Avatar, download: impl Fn(u64) -> Result<Vec<u8>, String>) -> Fetched {
    let worn = avatar
        .assets
        .iter()
        .filter(|a| is_accessory(a.asset_type.id) || is_clothing(a.asset_type.id))
        .map(|asset| Worn {
            asset: asset.clone(),
            dom: download(asset.id).and_then(|bytes| read_model(&bytes)),
        })
        .collect();
    Fetched { avatar, worn }
}

/// What [`dress`] could not do, one line each.
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
    let skipped = avatar
        .assets
        .iter()
        .filter(|a| !is_accessory(a.asset_type.id) && !is_clothing(a.asset_type.id))
        .count();
    if skipped > 0 {
        notes.push(format!(
            "{skipped} body part, head or animation asset(s) not applied: the rig is made of blocks"
        ));
    }
    notes
}

/// Puts every downloaded accessory and piece of clothing on `rig`; returns
/// how many were worn.
pub(crate) fn dress(dom: &mut WeakDom, rig: Ref, worn: &[Worn]) -> usize {
    worn.iter()
        .filter_map(|w| Some((w.asset.asset_type.id, w.dom.as_ref().ok()?)))
        .filter(|&(kind, source)| {
            if is_accessory(kind) {
                wear_accessory(dom, rig, source)
            } else {
                wear_clothing(dom, rig, source)
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
    match dom.get(node).and_then(|i| i.properties().get("CFrame")) {
        Some(Variant::CFrame(data)) => Cf::from_data(data),
        _ => Cf::at([0.; 3]),
    }
}

fn wear_clothing(rig_dom: &mut WeakDom, rig: Ref, source: &WeakDom) -> bool {
    let Some((_, item)) = find_of(source, source.root_refs(), &["Shirt", "Pants"]) else {
        return false;
    };
    clipboard::graft(rig_dom, source, item, rig).is_some()
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
mod tests {
    use super::*;
    use crate::shell::rig::build_rig;

    const AVATAR: &[u8] = br#"{"scales":{"height":1.0,"width":0.9,"head":1.0,"depth":0.9,"proportion":0.5,"bodyType":1.0},"playerAvatarType":"R15","bodyColors":{"headColorId":24,"torsoColorId":23,"rightArmColorId":24,"leftArmColorId":24,"rightLegColorId":119,"leftLegColorId":119},"assets":[{"id":1,"name":"Cap","assetType":{"id":8,"name":"Hat"}},{"id":2,"name":"Shirt","assetType":{"id":11,"name":"Shirt"}},{"id":3,"name":"Torso","assetType":{"id":27,"name":"Torso"}}]}"#;

    fn cap() -> WeakDom {
        let mut dom = WeakDom::new();
        let hat = dom.new_instance("Accessory", "Cap", None);
        let handle = dom.new_instance("Part", "Handle", Some(hat));
        let at = dom.new_instance("Attachment", "HatAttachment", Some(handle));
        set(
            &mut dom,
            at,
            "CFrame",
            Variant::CFrame(Cf::at([0., -0.5, 0.]).data()),
        );
        dom
    }

    fn shirt() -> WeakDom {
        let mut dom = WeakDom::new();
        dom.new_instance("Shirt", "Shirt", None);
        dom
    }

    fn worn(avatar: &Avatar) -> Vec<Worn> {
        avatar
            .assets
            .iter()
            .filter(|a| a.id < 3)
            .map(|a| Worn {
                asset: a.clone(),
                dom: Ok(if a.id == 1 { cap() } else { shirt() }),
            })
            .collect()
    }

    #[test]
    fn options_follow_the_avatar() {
        let avatar = Avatar::from_json(AVATAR).unwrap();
        let o = options_for(&avatar, JointStyle::AnimationConstraint, [1., 2., 3.]);
        assert_eq!(o.rig_type, RigType::R15);
        assert_eq!(o.scales.unwrap().width, 0.9);
        assert_eq!(o.colors.left_leg, BrickColor::from_number(119).unwrap().rgb);
        assert_eq!(o.feet, [1., 2., 3.]);
    }

    #[test]
    fn a_cap_is_welded_to_the_head_attachment_and_clothing_is_copied() {
        let avatar = Avatar::from_json(AVATAR).unwrap();
        let mut dom = WeakDom::new();
        let root = dom.new_instance("DataModel", "Game", None);
        let options = options_for(&avatar, JointStyle::AnimationConstraint, [0.; 3]);
        let rig = build_rig(&mut dom, &options, root);
        assert_eq!(dress(&mut dom, rig, &worn(&avatar)), 2);
        let all = descendants(&dom, rig);
        let weld = all
            .iter()
            .copied()
            .find(|&r| dom.get(r).is_some_and(|i| i.name() == "AccessoryWeld"))
            .unwrap();
        let w = dom.get(weld).unwrap();
        let Some(Variant::Ref(part1)) = w.properties().get("Part1") else {
            panic!("weld has no Part1");
        };
        assert_eq!(dom.get(*part1).unwrap().name(), "Head");
        let handle = dom.parent(weld).unwrap();
        assert_eq!(dom.get(handle).unwrap().name(), "Handle");
        // The handle ends where the weld says: on the head's top.
        let head = cframe_of(&dom, *part1);
        let top = head.mul(&Cf::at([0., 0.5, 0.])).p[1];
        let hat = cframe_of(&dom, handle).p[1];
        assert!((hat - (top + 0.5)).abs() < 0.3, "{hat} vs {top}");
        assert!(all
            .iter()
            .any(|&r| dom.get(r).is_some_and(|i| i.class() == "Shirt")));
    }

    #[test]
    fn what_is_not_applied_is_named() {
        let avatar = Avatar::from_json(AVATAR).unwrap();
        let notes = unapplied(&avatar, &worn(&avatar));
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("not applied"));
    }

    #[test]
    fn the_mock_loads_assets_beside_the_json() {
        let dir = std::env::temp_dir().join(format!("rig-mock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("avatar.json"), AVATAR).unwrap();
        for (id, dom) in [(1, cap()), (2, shirt())] {
            let bytes = rbx_binary::serialize(&dom).unwrap();
            std::fs::write(dir.join(format!("{id}.rbxm")), bytes).unwrap();
        }
        let fetched = {
            let path = dir.join("avatar.json");
            let avatar = Avatar::from_json(&std::fs::read(&path).unwrap()).unwrap();
            gather(avatar, |id| {
                std::fs::read(dir.join(format!("{id}.rbxm"))).map_err(|e| e.to_string())
            })
        };
        assert_eq!(fetched.worn.len(), 2);
        assert!(fetched.worn.iter().all(|w| w.dom.is_ok()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
