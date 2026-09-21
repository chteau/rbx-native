//! A part's assembly — the parts rigidly joined to it — and the three values
//! Studio shows for it, worked out only as far as Roblox documents them:
//!
//! - An assembly is the parts connected by joints (`physics/assemblies.md`):
//!   every `JointInstance` and `WeldConstraint` with both parts set that is
//!   enabled and active. Active means the joint sits in `Workspace` (or, for
//!   a `JointInstance`, `JointsService`) and both its parts in `Workspace`
//!   (`JointInstance.Active`, `WeldConstraint.Active`). A part outside
//!   `Workspace` has no assembly, so none of the three shows for it.
//! - With an anchored part the assembly's mass is infinite, and that part
//!   is its root and its centre of mass (`BasePart.AssemblyMass`,
//!   `AssemblyCenterOfMass`, `physics/assemblies.md`). Two anchored parts
//!   split an assembly in a way the docs leave open, so its centre and root
//!   are not shown.
//! - Otherwise its mass is the sum of its parts' but for the massless ones
//!   that are not the root, and the root is the first part left by: not
//!   massless, then the highest `RootPriority`. The last rule, by size with
//!   undocumented multipliers, is not reproduced: where it would decide,
//!   the root — and whatever depends on it — is not shown.
//! - A `WeldConstraint` saves whether it is enabled in an undocumented
//!   `State`; one whose `State` is not the default leaves its assembly
//!   unknown.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use rbx_dom::{Change, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::super::Properties;

/// The default `WeldConstraint.State`, rbx-dom's record of a new one.
const DEFAULT_WELD_STATE: i32 = 3;

/// Every property a joint's part in an assembly depends on, under each name
/// the DOM may hold it as.
const JOINT_PROPERTIES: &[&str] = &[
    "Part0",
    "Part1",
    "Part0Internal",
    "Part1Internal",
    "Enabled",
    "State",
];

/// Whether `change` can change which parts are joined: an instance added,
/// removed or reparented (in or out of `Workspace`), or a joint's own
/// property written. A part moving, the common case by far, cannot.
pub(in crate::properties) fn moves_joints(change: &Change) -> bool {
    match change {
        Change::Property { name, .. } => JOINT_PROPERTIES.contains(&name.as_str()),
        Change::Added(_) | Change::Removed(_) | Change::Parent { .. } => true,
    }
}

/// Which parts are joined to which, for one state of the DOM.
pub(in crate::properties) struct Joints {
    in_workspace: HashSet<Ref>,
    edges: HashMap<Ref, Vec<Ref>>,
    /// Parts a `WeldConstraint` with an unknown `State` touches.
    unknown: HashSet<Ref>,
}

impl Joints {
    fn build(db: &ReflectionDatabase, dom: &WeakDom) -> Joints {
        let service = |class: &str| {
            dom.root_refs()
                .iter()
                .copied()
                .find(|&root| dom.get(root).is_some_and(|i| i.class() == class))
        };
        let in_workspace = service("Workspace")
            .map(|workspace| subtree(dom, workspace))
            .unwrap_or_default();
        let mut holders: Vec<Ref> = in_workspace.iter().copied().collect();
        holders.extend(
            service("JointsService")
                .map_or_else(Vec::new, |s| subtree(dom, s).into_iter().collect()),
        );

        let mut joints = Joints {
            edges: HashMap::new(),
            unknown: HashSet::new(),
            in_workspace,
        };
        for joint in holders
            .into_iter()
            .filter_map(|reference| dom.get(reference))
        {
            let class = joint.class();
            let weld_constraint = class == "WeldConstraint";
            if !weld_constraint && !db.is_subclass_of(class, "JointInstance") {
                continue;
            }
            let part = |name| match db.stored_or_default(joint, name)?.1 {
                Variant::Ref(part) if joints.in_workspace.contains(part) => Some(*part),
                _ => None,
            };
            let (Some(part0), Some(part1)) = (part("Part0"), part("Part1")) else {
                continue;
            };
            // Only a `JointInstance` is active in `JointsService` as well.
            if weld_constraint && !joints.in_workspace.contains(&joint.referent()) {
                continue;
            }
            let enabled = if weld_constraint {
                match joint.properties().get("State") {
                    Some(Variant::Int32(state)) if *state != DEFAULT_WELD_STATE => {
                        joints.unknown.extend([part0, part1]);
                        continue;
                    }
                    _ => !matches!(
                        joint.properties().get("Enabled"),
                        Some(Variant::Bool(false))
                    ),
                }
            } else {
                !matches!(
                    db.stored_or_default(joint, "Enabled"),
                    Some((_, Variant::Bool(false)))
                )
            };
            if enabled {
                joints.edges.entry(part0).or_default().push(part1);
                joints.edges.entry(part1).or_default().push(part0);
            }
        }
        joints
    }

    /// Every part in `part`'s assembly, `part` first; `None` when it has
    /// none or it cannot be told.
    fn assembly(&self, part: Ref) -> Option<Vec<Ref>> {
        if !self.in_workspace.contains(&part) {
            return None;
        }
        let mut members = vec![part];
        let mut seen: HashSet<Ref> = members.iter().copied().collect();
        let mut index = 0;
        while let Some(&current) = members.get(index) {
            index += 1;
            if self.unknown.contains(&current) {
                return None;
            }
            for &next in self.edges.get(&current).into_iter().flatten() {
                if seen.insert(next) {
                    members.push(next);
                }
            }
        }
        Some(members)
    }
}

/// Every instance under `root`, `root` included.
fn subtree(dom: &WeakDom, root: Ref) -> HashSet<Ref> {
    let mut found = HashSet::new();
    let mut stack = vec![root];
    while let Some(reference) = stack.pop() {
        if let Some(instance) = dom.get(reference) {
            found.insert(reference);
            stack.extend(instance.children().iter().copied());
        }
    }
    found
}

/// One assembly's three values, worked out once for all of its parts.
#[derive(Debug, Default)]
pub(in crate::properties) struct Assembly {
    mass: Option<f32>,
    center: Option<Vector3Data>,
    root: Option<Ref>,
}

impl Properties {
    fn joints(&self, dom: &WeakDom) -> Rc<Joints> {
        if let Some(joints) = &*self.joints.borrow() {
            return Rc::clone(joints);
        }
        let joints = Rc::new(Joints::build(&self.db, dom));
        *self.joints.borrow_mut() = Some(Rc::clone(&joints));
        joints
    }

    /// `AssemblyMass`, `AssemblyCenterOfMass` or `AssemblyRootPart` for
    /// `part` — see this module's doc for when each is left out.
    pub(super) fn assembly(&self, dom: &WeakDom, part: Ref, name: &str) -> Option<Variant> {
        let assembly = self.assembly_of(dom, part)?;
        match name {
            "AssemblyMass" => assembly.mass.map(Variant::Float32),
            "AssemblyRootPart" => assembly.root.map(Variant::Ref),
            _ => assembly.center.map(Variant::Vector3),
        }
    }

    /// `part`'s assembly, worked out on the first ask since the DOM last
    /// changed and kept for every part in it: three rows each, for every
    /// selected part, would otherwise walk the same assembly again each time.
    fn assembly_of(&self, dom: &WeakDom, part: Ref) -> Option<Rc<Assembly>> {
        if let Some(assembly) = self.assemblies.borrow().get(&part) {
            return Some(Rc::clone(assembly));
        }
        let members = self.joints(dom).assembly(part)?;
        let assembly = Rc::new(self.work_out(dom, &members));
        let mut memo = self.assemblies.borrow_mut();
        for member in members {
            memo.insert(member, Rc::clone(&assembly));
        }
        Some(assembly)
    }

    fn work_out(&self, dom: &WeakDom, members: &[Ref]) -> Assembly {
        let Some(parts) = members
            .iter()
            .map(|&member| Some((member, dom.get(member)?)))
            .collect::<Option<Vec<(Ref, &Instance)>>>()
        else {
            return Assembly::default();
        };
        let flag = |instance: &Instance, name| {
            matches!(self.read(instance, name), Some(Variant::Bool(true)))
        };
        let anchored: Vec<&(Ref, &Instance)> = parts
            .iter()
            .filter(|(_, instance)| flag(instance, "Anchored"))
            .collect();
        if let [&(root, instance)] = anchored.as_slice() {
            return Assembly {
                mass: Some(f32::INFINITY),
                center: self.world_center(instance),
                root: Some(root),
            };
        }
        if !anchored.is_empty() {
            return Assembly {
                mass: Some(f32::INFINITY),
                ..Assembly::default()
            };
        }

        let massive: Vec<&(Ref, &Instance)> = parts
            .iter()
            .filter(|(_, instance)| !flag(instance, "Massless"))
            .collect();
        let candidates = if massive.is_empty() {
            parts.iter().collect()
        } else {
            massive.clone()
        };
        let priority = |instance: &Instance| match self.read(instance, "RootPriority") {
            Some(Variant::Int32(priority)) => priority,
            _ => 0,
        };
        let root = candidates
            .iter()
            .map(|(_, instance)| priority(instance))
            .max()
            .and_then(|highest| {
                let mut roots = candidates
                    .iter()
                    .filter(|(_, instance)| priority(instance) == highest);
                match (roots.next(), roots.next()) {
                    (Some(root), None) => Some(**root),
                    _ => None,
                }
            });

        // Whatever counts towards the assembly's mass: every part with mass,
        // or — when all of them are massless — the root alone.
        let counted: Vec<(Ref, &Instance)> = if massive.is_empty() {
            root.into_iter().collect()
        } else {
            massive.into_iter().copied().collect()
        };
        let weighed: Option<Vec<(f32, &Instance)>> = counted
            .iter()
            .map(|(_, instance)| Some((self.mass(dom, instance)?, *instance)))
            .collect();
        let Some(weighed) = weighed.filter(|weighed| !weighed.is_empty()) else {
            return Assembly {
                root: root.map(|(root, _)| root),
                ..Assembly::default()
            };
        };
        let total: f32 = weighed.iter().map(|(mass, _)| mass).sum();
        let center = weighed
            .iter()
            .map(|(mass, instance)| Some((*mass, self.world_center(instance)?)))
            .collect::<Option<Vec<_>>>()
            .filter(|_| total > 0.0)
            .map(|centers| {
                let mut weighted = [0.0f32; 3];
                for (mass, center) in centers {
                    for (sum, axis) in weighted.iter_mut().zip([center.x, center.y, center.z]) {
                        *sum += mass * axis;
                    }
                }
                Vector3Data {
                    x: weighted[0] / total,
                    y: weighted[1] / total,
                    z: weighted[2] / total,
                }
            });
        Assembly {
            mass: Some(total),
            center,
            root: root.map(|(root, _)| root),
        }
    }

    /// A part's centre of mass in world space.
    fn world_center(&self, instance: &Instance) -> Option<Vector3Data> {
        let local = self.center_of_mass(instance)?;
        let Variant::CFrame(frame) = self.read(instance, "CFrame")? else {
            return None;
        };
        let r = frame.rotation;
        let p = frame.position;
        Some(Vector3Data {
            x: p.x + r[0] * local.x + r[1] * local.y + r[2] * local.z,
            y: p.y + r[3] * local.x + r[4] * local.y + r[5] * local.z,
            z: p.z + r[6] * local.x + r[7] * local.y + r[8] * local.z,
        })
    }
}
