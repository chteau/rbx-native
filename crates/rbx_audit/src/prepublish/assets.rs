//! `broken-asset` and `assets-not-checked`.

use std::collections::{BTreeMap, HashMap};

use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{content_uri, instances, place_finding, place_ref, Presence};
use crate::{full_name, Check, Finding, Severity};

/// Classes that draw or play nothing without an asset: the properties any
/// one of which must be filled, and how bad an empty one is.
const REQUIRED: [(&str, &[&str], Severity); 6] = [
    ("MeshPart", &["MeshId", "MeshContent"], Severity::Medium),
    ("Animation", &["AnimationId"], Severity::Medium),
    ("Sound", &["SoundId", "AudioContent"], Severity::Low),
    ("Decal", &["Texture", "TextureContent"], Severity::Low),
    ("Texture", &["Texture", "TextureContent"], Severity::Low),
    // Only a `FileMesh` one; see `required`.
    ("SpecialMesh", &["MeshId"], Severity::Medium),
];

/// `SpecialMesh.MeshType`'s `FileMesh`.
const FILE_MESH: u32 = 5;

/// Every asset id the place's content properties name, sorted, each once:
/// what to ask the asset service about.
pub fn asset_ids(dom: &WeakDom) -> Vec<u64> {
    let db = ReflectionDatabase::shared();
    let mut ids: Vec<u64> = uses(dom, db).0.into_keys().collect();
    ids.sort_unstable();
    ids
}

type Uses = BTreeMap<u64, Vec<(Ref, String)>>;

/// The instances and properties using each id, and the malformed
/// references (instance, property, value).
fn uses(dom: &WeakDom, db: &ReflectionDatabase) -> (Uses, Vec<(Ref, String, String)>) {
    let mut ids = Uses::new();
    let mut malformed = Vec::new();
    for instance in instances(dom) {
        for (key, value) in instance.properties() {
            let Some(uri) = content_uri(db, instance.class(), key, value) else {
                continue;
            };
            match AssetRef::parse(uri) {
                Ok(AssetRef::Id(0)) | Err(_) => {
                    malformed.push((instance.referent(), key.clone(), uri.to_string()))
                }
                Ok(AssetRef::Id(id)) => ids
                    .entry(id)
                    .or_default()
                    .push((instance.referent(), key.clone())),
                Ok(_) => {}
            }
        }
    }
    (ids, malformed)
}

pub(super) fn check(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    presence: Option<&HashMap<u64, Presence>>,
) -> Vec<Finding> {
    let (ids, malformed) = uses(dom, db);
    let mut findings: Vec<Finding> = malformed
        .into_iter()
        .map(|(instance, key, uri)| Finding {
            check: Check::BrokenAsset,
            severity: Severity::High,
            instance,
            path: full_name(dom, instance),
            line: None,
            message: format!("`{key}` is `{uri}`, which isn't an asset reference Roblox can load: use `rbxassetid://<id>`."),
        })
        .collect();

    let mut unchecked = 0;
    for (id, users) in &ids {
        let answer = presence.and_then(|map| map.get(id));
        let (severity, what) = match answer {
            None => {
                unchecked += 1;
                continue;
            }
            Some(Presence::Exists) => continue,
            Some(Presence::Missing) => (Severity::High, "doesn't exist on Roblox"),
            Some(Presence::Restricted) => (
                Severity::Medium,
                "is refused by Roblox's asset service (moderated, or shared with nobody)",
            ),
        };
        let (instance, key) = &users[0];
        let others = match users.len() - 1 {
            0 => String::new(),
            1 => " (and 1 other use)".to_string(),
            n => format!(" (and {n} other uses)"),
        };
        findings.push(Finding {
            check: Check::BrokenAsset,
            severity,
            instance: *instance,
            path: full_name(dom, *instance),
            line: None,
            message: format!("`{key}` names asset {id}, which {what}{others}."),
        });
    }
    if unchecked > 0 {
        if let Some(place) = place_ref(dom) {
            let why = if presence.is_none() {
                "Roblox's asset service wasn't reached (offline?)"
            } else {
                "Roblox's asset service didn't answer for them"
            };
            findings.push(place_finding(
                dom,
                place,
                Check::AssetsNotChecked,
                Severity::Low,
                format!("{unchecked} asset id(s) not checked: {why}. Run the check again online."),
            ));
        }
    }

    for instance in instances(dom) {
        if let Some((key, severity)) = required(db, instance) {
            findings.push(Finding {
                check: Check::BrokenAsset,
                severity,
                instance: instance.referent(),
                path: full_name(dom, instance.referent()),
                line: None,
                message: format!(
                    "`{key}` is empty: this {} shows or plays nothing.",
                    instance.class()
                ),
            });
        }
    }
    findings
}

/// The property an instance needs and leaves empty, and the severity.
fn required(
    db: &ReflectionDatabase,
    instance: &rbx_dom::Instance,
) -> Option<(&'static str, Severity)> {
    let (class, keys, severity) = REQUIRED
        .iter()
        .find(|(class, ..)| *class == instance.class())?;
    let properties = instance.properties();
    if *class == "SpecialMesh" && properties.get("MeshType") != Some(&Variant::Enum(FILE_MESH)) {
        return None;
    }
    let filled = keys.iter().any(|key| match properties.get(*key) {
        Some(Variant::Content(rbx_dom::Content::Object(_))) => true,
        Some(value) => content_uri(db, class, key, value).is_some_and(|uri| !uri.trim().is_empty()),
        None => false,
    });
    (!filled).then_some((keys[0], *severity))
}
