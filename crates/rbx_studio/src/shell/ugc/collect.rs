//! An `Accessory`, `MeshPart` or `Model` from the place as an [`rbx_ugc::Item`]:
//! [`plan`] reads the tree on the UI thread, [`resolve`] downloads and
//! parses the meshes and textures it names off it.

use std::collections::BTreeMap;

use rbx_dom::{Content, Ref, Variant, WeakDom};
use rbx_ugc::spec::R15_PARTS;
use rbx_ugc::{
    AssetType, CFrame, Cage, CageKind, Finding, Geometry, Item, Part, Placement, Rule, Severity,
    Texture, IDENTITY,
};

use super::{Collected, ItemKind};

mod head;
mod resolve;
mod studio;

pub(super) use head::{head_part, head_plan, resolve_head, HeadMesh};
pub(super) use resolve::mesh_geometry;
pub(super) use resolve::{fetch_cached, resolve};

const SURFACE_SLOTS: [&str; 4] = ["ColorMap", "NormalMap", "MetalnessMap", "RoughnessMap"];

/// Where a mesh's bytes come from: an uploaded asset, or a file this editor
/// made and keeps in the local asset cache (`rbxasset://rbx-native-local/...`).
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell::ugc) enum Key {
    Id(u64),
    Local(String),
}

#[derive(Debug, Clone, PartialEq)]
struct PartPlan {
    name: String,
    mesh: Option<Key>,
    size: [f32; 3],
    cframe: CFrame,
    attachments: Vec<Placement>,
}

#[derive(Debug, Clone, PartialEq)]
struct TexturePlan {
    owner: String,
    slot: String,
    id: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct CagePlan {
    kind: CageKind,
    name: String,
    mesh: Key,
}

/// What [`resolve`] needs from the tree, with asset ids still unfetched.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Plan {
    name: String,
    kind: ItemKind,
    parts: Vec<PartPlan>,
    textures: Vec<TexturePlan>,
    cages: Vec<CagePlan>,
    emissive: bool,
    /// Findings only the tree can give, see `studio`.
    notes: Vec<Finding>,
}

fn uri<'a>(props: &'a BTreeMap<String, Variant>, key: &str) -> Option<&'a str> {
    let text = match props.get(key)? {
        Variant::String(text) => text.as_str(),
        Variant::Content(Content::Uri(uri)) => uri.as_str(),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

fn asset_id(text: &str) -> Option<u64> {
    match rbx_assets::AssetRef::parse(text).ok()? {
        rbx_assets::AssetRef::Id(id) => Some(id),
        _ => None,
    }
}

const LOCAL: &str = "rbxasset://rbx-native-local/";

pub(in crate::shell::ugc) fn mesh_key(text: &str) -> Option<Key> {
    match text.strip_prefix(LOCAL) {
        Some(path) => Some(Key::Local(format!("rbx-native-local/{path}"))),
        None => asset_id(text).map(Key::Id),
    }
}

fn cframe_of(props: &BTreeMap<String, Variant>) -> CFrame {
    cframe_at(props, "CFrame")
}

fn cframe_at(props: &BTreeMap<String, Variant>, key: &str) -> CFrame {
    match props.get(key) {
        Some(Variant::CFrame(c)) => {
            let r = c.rotation;
            [
                c.position.x,
                c.position.y,
                c.position.z,
                r[0],
                r[1],
                r[2],
                r[3],
                r[4],
                r[5],
                r[6],
                r[7],
                r[8],
            ]
        }
        _ => IDENTITY,
    }
}

fn is_base_part(class: &str) -> bool {
    matches!(class, "MeshPart" | "Part")
}

fn descendants(dom: &WeakDom, root: Ref, out: &mut Vec<Ref>) {
    let Some(inst) = dom.get(root) else { return };
    for &child in inst.children() {
        out.push(child);
        descendants(dom, child, out);
    }
}

fn part_plan(dom: &WeakDom, part: Ref) -> Option<PartPlan> {
    let inst = dom.get(part)?;
    let props = inst.properties();
    // Files store a part's size as `size`; `Size` is what a new part gets.
    let size = match props.get("size").or_else(|| props.get("Size")) {
        Some(Variant::Vector3(v)) => [v.x, v.y, v.z],
        _ => [1.0; 3],
    };
    let attachments = inst
        .children()
        .iter()
        .filter_map(|&c| dom.get(c))
        .filter(|c| c.class() == "Attachment")
        .map(|c| Placement {
            name: c.name().to_string(),
            cframe: cframe_of(c.properties()),
        })
        .collect();
    Some(PartPlan {
        name: inst.name().to_string(),
        mesh: ["MeshId", "MeshContent"]
            .iter()
            .find_map(|k| uri(props, k))
            .and_then(mesh_key),
        size,
        cframe: cframe_of(props),
        attachments,
    })
}

fn collect_textures(dom: &WeakDom, part: Ref, plan: &mut Plan) {
    let Some(inst) = dom.get(part) else { return };
    let props = inst.properties();
    if let Some((slot, id)) = ["TextureID", "TextureContent"]
        .into_iter()
        .find_map(|k| uri(props, k).and_then(asset_id).map(|id| (k, id)))
    {
        plan.textures.push(TexturePlan {
            owner: inst.name().to_string(),
            slot: slot.to_string(),
            id,
        });
    }
    for &child in inst.children() {
        let Some(sa) = dom.get(child).filter(|c| c.class() == "SurfaceAppearance") else {
            continue;
        };
        let sp = sa.properties();
        for slot in SURFACE_SLOTS {
            let content = format!("{slot}Content");
            if let Some(id) = [slot, content.as_str()]
                .iter()
                .find_map(|k| uri(sp, k))
                .and_then(asset_id)
            {
                plan.textures.push(TexturePlan {
                    owner: sa.name().to_string(),
                    slot: slot.to_string(),
                    id,
                });
            }
        }
        if uri(sp, "EmissiveMaskContent").is_some() || uri(sp, "EmissiveMask").is_some() {
            plan.emissive = true;
        }
    }
}

fn wrap_cages(dom: &WeakDom, part: Ref, item: &str, plan: &mut Plan) {
    let Some(inst) = dom.get(part) else { return };
    for &child in inst.children() {
        let Some(wrap) = dom.get(child).filter(|c| c.class() == "WrapLayer") else {
            continue;
        };
        let props = wrap.properties();
        for (key, kind, suffix) in [
            ("ReferenceMeshId", CageKind::Inner, "InnerCage"),
            ("CageMeshId", CageKind::Outer, "OuterCage"),
        ] {
            if let Some(mesh) = uri(props, key).and_then(mesh_key) {
                plan.cages.push(CagePlan {
                    kind,
                    name: format!("{item}_{suffix}"),
                    mesh,
                });
            }
        }
    }
}

pub(super) fn plan(dom: &WeakDom, root: Ref, kind: ItemKind) -> Result<Plan, String> {
    let inst = dom
        .get(root)
        .ok_or_else(|| "That item is no longer in the place.".to_string())?;
    let mut plan = Plan {
        name: inst.name().to_string(),
        kind,
        parts: Vec::new(),
        textures: Vec::new(),
        cages: Vec::new(),
        emissive: false,
        notes: Vec::new(),
    };
    let single = match inst.class() {
        "Accessory" => {
            let kids = || inst.children().iter().copied();
            let is_part = |r: &Ref| dom.get(*r).is_some_and(|c| is_base_part(c.class()));
            let handle = kids()
                .find(|r| is_part(r) && dom.get(*r).is_some_and(|c| c.name() == "Handle"))
                .or_else(|| kids().find(is_part))
                .ok_or_else(|| {
                    format!(
                        "Could not find required MeshPart called Handle inside of {}",
                        inst.name()
                    )
                })?;
            let tree = studio::node(dom, root);
            if !tree
                .children
                .iter()
                .any(|c| c.name == "Handle" && matches!(c.class.as_str(), "MeshPart" | "Part"))
            {
                plan.notes.push(Finding {
                    rule: Rule::Schema,
                    severity: Severity::Error,
                    subject: plan.name.clone(),
                    measured: None,
                    limit: None,
                    message: format!(
                        "Could not find required MeshPart called Handle inside of {}",
                        plan.name
                    ),
                });
            }
            plan.notes.extend(rbx_ugc::schema::check(&tree, kind.asset));
            Some(handle)
        }
        "MeshPart" | "Part" => Some(root),
        "Model" => {
            if matches!(kind.asset, AssetType::LeftShoe | AssetType::RightShoe) {
                plan.notes
                    .extend(rbx_ugc::schema::shoe_pair(&studio::node(dom, root)));
            }
            None
        }
        _ => return Err("Pick an Accessory, a MeshPart or a Model".to_string()),
    };
    if let Some(part) = single {
        studio::handle_rules(dom, part, &mut plan.kind, &plan.name, &mut plan.notes);
        plan.parts.extend(part_plan(dom, part));
        collect_textures(dom, part, &mut plan);
        let name = plan.name.clone();
        wrap_cages(dom, part, &name, &mut plan);
        return Ok(plan);
    }
    let mut all = Vec::new();
    descendants(dom, root, &mut all);
    let body = matches!(kind.asset, AssetType::Body | AssetType::DynamicHead);
    for r in all {
        let Some(child) = dom.get(r).filter(|c| c.class() == "MeshPart") else {
            continue;
        };
        let name = child.name();
        if body {
            if !R15_PARTS.contains(&name) {
                continue;
            }
        } else if let Some((kind, mesh)) = cage_of(name, child.properties()) {
            plan.cages.push(CagePlan {
                kind,
                name: name.to_string(),
                mesh,
            });
            continue;
        }
        plan.parts.extend(part_plan(dom, r));
        collect_textures(dom, r, &mut plan);
    }
    Ok(plan)
}

fn cage_of(name: &str, props: &BTreeMap<String, Variant>) -> Option<(CageKind, Key)> {
    let kind = if name.ends_with("_InnerCage") {
        CageKind::Inner
    } else if name.ends_with("_OuterCage") {
        CageKind::Outer
    } else {
        return None;
    };
    let mesh = ["MeshId", "MeshContent"]
        .iter()
        .find_map(|k| uri(props, k))
        .and_then(mesh_key)?;
    Some((kind, mesh))
}

#[cfg(test)]
#[path = "collect/tests.rs"]
mod tests;
