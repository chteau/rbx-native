//! `unanchored-parts` and `heavy-geometry`.

use std::collections::{BTreeMap, HashMap};

use rbx_assets::AssetRef;
use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{content_uri, instances, MESH_TRIANGLE_LIMIT, UNION_TRIANGLES};
use crate::{full_name, Check, Finding, Severity};

/// Ancestors whose parts are meant to move: a character's (a model with a
/// `Humanoid` or `AnimationController`), and what a character carries.
const CARRIED: [&str; 3] = ["Tool", "Accessory", "Accoutrement"];
const RIGGED: [&str; 2] = ["Humanoid", "AnimationController"];

/// Legacy body movers: a part holding one is held up by it.
const BODY_MOVERS: [&str; 7] = [
    "BodyPosition",
    "BodyVelocity",
    "BodyGyro",
    "BodyForce",
    "BodyThrust",
    "BodyAngularVelocity",
    "RocketPropulsion",
];

/// How many parts a finding names before "…".
const NAMED: usize = 3;

pub(super) fn unanchored(dom: &WeakDom, db: &ReflectionDatabase) -> Vec<Finding> {
    let Some(workspace) = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
    else {
        return Vec::new();
    };

    // Every simulated part, and every instance that could join two.
    let mut parts: Vec<Ref> = Vec::new();
    let mut links: Vec<&Instance> = Vec::new();
    let mut stack: Vec<Ref> = vec![workspace];
    while let Some(r) = stack.pop() {
        let Some(instance) = dom.get(r) else { continue };
        let class = instance.class();
        if CARRIED.contains(&class)
            || class == "Camera"
            || (class == "Model" && has_child(dom, instance, &RIGGED))
        {
            continue;
        }
        if db.is_subclass_of(class, "BasePart") && class != "Terrain" {
            parts.push(r);
        } else if r != workspace {
            links.push(instance);
        }
        stack.extend_from_slice(instance.children());
    }
    let index: HashMap<Ref, usize> = parts.iter().enumerate().map(|(i, r)| (*r, i)).collect();
    let mut sets = Sets::new(parts.len());
    let mut held = vec![false; parts.len()];

    for (i, r) in parts.iter().enumerate() {
        let instance = dom.get(*r).expect("collected above");
        held[i] = instance.properties().get("Anchored") == Some(&Variant::Bool(true))
            || has_child(dom, instance, &BODY_MOVERS);
    }
    let part_of = |key: &str, link: &Instance, through_attachment: bool| {
        let Some(Variant::Ref(target)) = link.properties().get(key) else {
            return None;
        };
        let target = if through_attachment {
            dom.parent(*target)?
        } else {
            *target
        };
        index.get(&target).copied()
    };
    for link in links {
        let class = link.class();
        if class == "NoCollisionConstraint" {
            continue;
        }
        let ends = if db.is_subclass_of(class, "Constraint") {
            [
                part_of("Attachment0", link, true),
                part_of("Attachment1", link, true),
            ]
        } else {
            [
                part_of("Part0", link, false).or_else(|| part_of("Part0Internal", link, false)),
                part_of("Part1", link, false).or_else(|| part_of("Part1Internal", link, false)),
            ]
        };
        match ends {
            [Some(a), Some(b)] => sets.join(a, b),
            // A one-attachment mover (`AlignPosition`, `LinearVelocity`)
            // holds its part on its own.
            [Some(a), None] | [None, Some(a)] if db.is_subclass_of(class, "Constraint") => {
                held[a] = true
            }
            _ => {}
        }
    }
    let mut held_sets = vec![false; parts.len()];
    for (i, held) in held.iter().enumerate() {
        if *held {
            held_sets[sets.find(i)] = true;
        }
    }

    // Loose parts by the top-level model they're in (or the Workspace).
    let mut groups: BTreeMap<String, (Ref, Vec<&str>)> = BTreeMap::new();
    for (i, r) in parts.iter().enumerate() {
        if held_sets[sets.find(i)] {
            continue;
        }
        let group = top_model(dom, *r, workspace);
        groups
            .entry(full_name(dom, group))
            .or_insert_with(|| (group, Vec::new()))
            .1
            .push(dom.get(*r).map_or("", |i| i.name()));
    }
    groups
        .into_iter()
        .map(|(path, (group, names))| {
            let mut named: Vec<String> = names.iter().take(NAMED).map(|n| format!("`{n}`")).collect();
            if names.len() > NAMED {
                named.push("\u{2026}".into());
            }
            Finding {
                check: Check::UnanchoredParts,
                severity: Severity::Medium,
                instance: group,
                path,
                line: None,
                message: format!(
                    "{} unanchored part(s) aren't welded or constrained to anything anchored, so they fall when the game starts: {}.",
                    names.len(),
                    named.join(", ")
                ),
            }
        })
        .collect()
}

fn has_child(dom: &WeakDom, instance: &Instance, classes: &[&str]) -> bool {
    instance
        .children()
        .iter()
        .any(|c| dom.get(*c).is_some_and(|c| classes.contains(&c.class())))
}

/// The outermost `Model` holding `r` below the Workspace, or the Workspace.
fn top_model(dom: &WeakDom, r: Ref, workspace: Ref) -> Ref {
    let mut top = workspace;
    let mut at = dom.parent(r);
    while let Some(a) = at.filter(|a| *a != workspace) {
        if dom.get(a).is_some_and(|i| i.class() == "Model") {
            top = a;
        }
        at = dom.parent(a);
    }
    top
}

/// Union-find over part indices.
struct Sets(Vec<usize>);

impl Sets {
    fn new(len: usize) -> Self {
        Sets((0..len).collect())
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        self.0[a] = b;
    }
}

pub(super) fn geometry(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    fetch: &mut dyn FnMut(&AssetRef) -> Option<Vec<u8>>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut measured: HashMap<AssetRef, Option<usize>> = HashMap::new();
    for instance in instances(dom) {
        let class = instance.class();
        let finding = |severity, message| Finding {
            check: Check::HeavyGeometry,
            severity,
            instance: instance.referent(),
            path: full_name(dom, instance.referent()),
            line: None,
            message,
        };
        if db.is_subclass_of(class, "PartOperation") {
            let count = match instance.properties().get("TriangleCount") {
                Some(&Variant::Int32(n)) => n as i64,
                Some(&Variant::Int64(n)) => n,
                _ => 0,
            };
            if count > UNION_TRIANGLES as i64 {
                findings.push(finding(
                    Severity::Medium,
                    format!("This union draws {count} triangles, over the budget of {UNION_TRIANGLES}: export it as a mesh, or split it."),
                ));
            }
        } else if db.is_subclass_of(class, "MeshPart") {
            let Some(asset) = ["MeshContent", "MeshId"].into_iter().find_map(|key| {
                let uri = content_uri(db, class, key, instance.properties().get(key)?)?;
                AssetRef::parse(uri).ok().filter(|a| *a != AssetRef::Empty)
            }) else {
                continue;
            };
            let count = *measured
                .entry(asset.clone())
                .or_insert_with(|| Some(rbx_mesh::parse(&fetch(&asset)?).ok()?.triangle_count()));
            if let Some(count) = count.filter(|n| *n > MESH_TRIANGLE_LIMIT) {
                findings.push(finding(
                    Severity::High,
                    format!("This mesh has {count} triangles, over Roblox's limit of {MESH_TRIANGLE_LIMIT} for a MeshPart: decimate it before uploading."),
                ));
            }
        }
    }
    findings
}
