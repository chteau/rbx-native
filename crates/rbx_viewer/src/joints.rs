//! Poses parts from their joints' `Transform`, the way Roblox's `Animator`
//! does: an animation never writes a part's `CFrame`, it writes each
//! `Motor6D`'s (or `AnimationConstraint`'s) `Transform`, and the joint puts
//! `Part1` at `Part0 · C0 · Transform · C1⁻¹`.
//!
//! The editor's DOM keeps every part at rest. This works on the render
//! thread's mirror of it: after each batch of edits is mirrored, the parts
//! hanging off a joint with a non-identity `Transform` are laid out from
//! their assembly's root and written into the mirror, and the batch grows
//! one `CFrame` change per part moved, so the scene, skinned meshes and
//! picking follow as they would any edit. Parts welded to a posed part
//! (an accessory's `Handle`) keep their place on it. A `Transform` back at
//! identity puts its parts back where the editor holds them.
//!
//! Dormant, at no cost per batch, until a `Transform` is first written.

use std::collections::{HashMap, HashSet, VecDeque};

use glam::{Mat4, Vec4};
use rbx_dom::{CFrameData, Change, Ref, Variant, Vector3Data, WeakDom};

use crate::scene::{cframe_matrix, descendants};

/// Below this, a matrix entry is the same one: well under what a pose moves.
const SAME: f32 = 1e-5;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// A `Motor6D` or `Motor`: `C0`, `C1` and `Transform` on the joint.
    Motor,
    /// An `AnimationConstraint`: its attachments' frames stand for `C0`
    /// and `C1`.
    Animation { a0: Ref, a1: Ref },
    /// A weld of any kind: `Part1` keeps the place on `Part0` it rests at.
    Rigid,
}

#[derive(Clone, Copy, Debug)]
struct Link {
    joint: Ref,
    kind: Kind,
    part0: Ref,
    part1: Ref,
}

/// See the module docs.
#[derive(Default)]
pub struct JointSolver {
    /// Every joint in the mirror, read while anything is posed.
    links: Option<Vec<Link>>,
    /// Each part moved off its rest: the rest, and what was written.
    moved: HashMap<Ref, (CFrameData, CFrameData)>,
}

impl JointSolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether any part is held off its rest.
    pub fn posing(&self) -> bool {
        !self.moved.is_empty()
    }

    /// Re-poses `dom` (a mirror just brought in step with the editor's DOM
    /// by `changes`) and returns the `CFrame` changes that took, to apply
    /// with `changes`.
    pub fn solve(&mut self, dom: &mut WeakDom, changes: &[Change]) -> Vec<Change> {
        // A snapshot of a moved part carries the editor's copy of it, which
        // is the rest: whatever no longer reads as what was written here is
        // a new rest.
        let mut gone = Vec::new();
        for (&part, (rest, written)) in &mut self.moved {
            match cframe(dom, part) {
                Some(now) if !same(&now, written) => *rest = now,
                Some(_) => {}
                None => gone.push(part),
            }
        }
        for part in gone {
            self.moved.remove(&part);
        }
        if changes.iter().any(restructures) {
            self.links = None;
        }
        let transformed = changes
            .iter()
            .any(|c| matches!(c, Change::Property { name, .. } if name == "Transform"));
        if self.links.is_none() && self.moved.is_empty() && !transformed {
            return Vec::new();
        }
        let links = self.links.get_or_insert_with(|| index(dom));
        let posed = layout(dom, links, &self.moved);

        let mut out = Vec::new();
        let mut write = |dom: &mut WeakDom, part: Ref, value: CFrameData| {
            if let Some(instance) = dom.get_mut(part) {
                instance
                    .properties_mut()
                    .insert("CFrame".to_owned(), Variant::CFrame(value));
                out.push(Change::Property {
                    referent: part,
                    name: "CFrame".to_owned(),
                });
            }
        };
        let back: Vec<Ref> = self
            .moved
            .keys()
            .copied()
            .filter(|p| !posed.contains_key(p))
            .collect();
        for part in back {
            if let Some((rest, _)) = self.moved.remove(&part) {
                write(dom, part, rest);
            }
        }
        for (part, frame) in posed {
            let rest = match self.moved.get(&part) {
                Some((rest, _)) => *rest,
                None => match cframe(dom, part) {
                    Some(c) => c,
                    None => continue,
                },
            };
            let value = to_data(&frame);
            if same(&value, &rest) {
                if let Some((rest, _)) = self.moved.remove(&part) {
                    write(dom, part, rest);
                }
                continue;
            }
            if cframe(dom, part).is_some_and(|now| !same(&now, &value)) {
                write(dom, part, value);
            }
            self.moved.insert(part, (rest, value));
        }
        if self.moved.is_empty() {
            self.links = None;
        }
        out
    }
}

/// A change that can add, drop or rewire a joint.
fn restructures(change: &Change) -> bool {
    match change {
        Change::Property { name, .. } => matches!(
            name.as_str(),
            "Part0" | "Part1" | "Attachment0" | "Attachment1" | "Enabled"
        ),
        Change::Parent { .. } | Change::Added(_) | Change::Removed(_) | Change::Class(_) => true,
    }
}

fn index(dom: &WeakDom) -> Vec<Link> {
    let mut links = Vec::new();
    for joint in descendants(dom) {
        let Some(instance) = dom.get(joint) else {
            continue;
        };
        if matches!(
            instance.properties().get("Enabled"),
            Some(Variant::Bool(false))
        ) {
            continue;
        }
        let parts = |a: &str, b: &str| Some((reference(dom, joint, a)?, reference(dom, joint, b)?));
        let attached = || {
            let (a0, a1) = parts("Attachment0", "Attachment1")?;
            Some((a0, a1, dom.parent(a0)?, dom.parent(a1)?))
        };
        let link = match instance.class() {
            "Motor6D" | "Motor" => parts("Part0", "Part1").map(|(p0, p1)| (Kind::Motor, p0, p1)),
            "AnimationConstraint" => {
                attached().map(|(a0, a1, p0, p1)| (Kind::Animation { a0, a1 }, p0, p1))
            }
            "Weld" | "ManualWeld" | "Snap" | "Glue" | "WeldConstraint" => {
                parts("Part0", "Part1").map(|(p0, p1)| (Kind::Rigid, p0, p1))
            }
            "RigidConstraint" => attached().map(|(_, _, p0, p1)| (Kind::Rigid, p0, p1)),
            _ => None,
        };
        if let Some((kind, part0, part1)) = link {
            if part0 != part1 {
                links.push(Link {
                    joint,
                    kind,
                    part0,
                    part1,
                });
            }
        }
    }
    links
}

/// `Part1` relative to `Part0` for a joint that animates: `C0 · T · C1⁻¹`.
fn offset(dom: &WeakDom, link: &Link) -> Option<Mat4> {
    let (c0, c1) = match link.kind {
        Kind::Motor => (
            matrix(dom, link.joint, "C0").unwrap_or(Mat4::IDENTITY),
            matrix(dom, link.joint, "C1").unwrap_or(Mat4::IDENTITY),
        ),
        Kind::Animation { a0, a1 } => (
            matrix(dom, a0, "CFrame").unwrap_or(Mat4::IDENTITY),
            matrix(dom, a1, "CFrame").unwrap_or(Mat4::IDENTITY),
        ),
        Kind::Rigid => return None,
    };
    let transform = matrix(dom, link.joint, "Transform").unwrap_or(Mat4::IDENTITY);
    Some(c0 * transform * c1.inverse())
}

fn animated(dom: &WeakDom, link: &Link) -> bool {
    link.kind != Kind::Rigid
        && matrix(dom, link.joint, "Transform")
            .is_some_and(|t| !t.abs_diff_eq(Mat4::IDENTITY, SAME))
}

/// Every part of an assembly holding an animated joint, posed; the rest of
/// the place is left out.
fn layout(
    dom: &WeakDom,
    links: &[Link],
    moved: &HashMap<Ref, (CFrameData, CFrameData)>,
) -> HashMap<Ref, Mat4> {
    let rest = |part: Ref| -> Option<Mat4> {
        match moved.get(&part) {
            Some((rest, _)) => Some(cframe_matrix(rest)),
            None => matrix(dom, part, "CFrame"),
        }
    };
    let mut touching: HashMap<Ref, Vec<usize>> = HashMap::new();
    let mut driver: HashMap<Ref, usize> = HashMap::new();
    for (i, link) in links.iter().enumerate() {
        touching.entry(link.part0).or_default().push(i);
        touching.entry(link.part1).or_default().push(i);
        if link.kind != Kind::Rigid {
            driver.entry(link.part1).or_insert(i);
        }
    }
    // Each animated joint's assembly is laid out from its root: the part
    // the chain of joints driving it climbs to.
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    for link in links.iter().filter(|l| animated(dom, l)) {
        let mut part = link.part0;
        let mut climbed = HashSet::from([part]);
        while let Some(&up) = driver.get(&part) {
            if !climbed.insert(links[up].part0) {
                break;
            }
            part = links[up].part0;
        }
        if seen.insert(part) {
            roots.push(part);
        }
    }

    let mut posed: HashMap<Ref, Mat4> = HashMap::new();
    let mut queue = VecDeque::new();
    for root in roots {
        if posed.contains_key(&root) {
            continue;
        }
        let Some(at) = rest(root) else {
            continue;
        };
        posed.insert(root, at);
        queue.push_back(root);
        while let Some(part) = queue.pop_front() {
            let here = posed[&part];
            for &i in touching.get(&part).into_iter().flatten() {
                let link = &links[i];
                let (other, forward) = match part == link.part0 {
                    true => (link.part1, true),
                    false => (link.part0, false),
                };
                if posed.contains_key(&other) {
                    continue;
                }
                let placed = match offset(dom, link) {
                    Some(off) if forward => here * off,
                    Some(off) => here * off.inverse(),
                    None => match (rest(part), rest(other)) {
                        (Some(a), Some(b)) => here * a.inverse() * b,
                        _ => continue,
                    },
                };
                posed.insert(other, placed);
                queue.push_back(other);
            }
        }
    }
    posed
}

fn reference(dom: &WeakDom, at: Ref, property: &str) -> Option<Ref> {
    match dom.get(at)?.properties().get(property)? {
        Variant::Ref(to) if dom.get(*to).is_some() => Some(*to),
        _ => None,
    }
}

fn cframe(dom: &WeakDom, at: Ref) -> Option<CFrameData> {
    match dom.get(at)?.properties().get("CFrame")? {
        Variant::CFrame(c) => Some(*c),
        _ => None,
    }
}

fn matrix(dom: &WeakDom, at: Ref, property: &str) -> Option<Mat4> {
    match dom.get(at)?.properties().get(property)? {
        Variant::CFrame(c) => Some(cframe_matrix(c)),
        _ => None,
    }
}

/// [`cframe_matrix`] undone: a rigid transform back to row-major `CFrame`.
fn to_data(m: &Mat4) -> CFrameData {
    let column = |j: usize| -> Vec4 { m.col(j) };
    let (x, y, z, w) = (column(0), column(1), column(2), column(3));
    CFrameData {
        position: Vector3Data {
            x: w.x,
            y: w.y,
            z: w.z,
        },
        rotation: [x.x, y.x, z.x, x.y, y.y, z.y, x.z, y.z, z.z],
    }
}

fn same(a: &CFrameData, b: &CFrameData) -> bool {
    let p = |v: &Vector3Data| [v.x, v.y, v.z];
    p(&a.position)
        .iter()
        .zip(p(&b.position))
        .all(|(x, y)| (x - y).abs() < SAME * 10.0)
        && a.rotation
            .iter()
            .zip(b.rotation)
            .all(|(x, y)| (x - y).abs() < SAME)
}

#[cfg(test)]
mod tests;
