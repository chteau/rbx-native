//! Cutting the mesh into the 15 R15 parts (option A of the spec): geodesic
//! ownership, plane cuts restricted to each joint's neighbourhood, exact
//! triangle splitting, caps.
//!
//! Why not plain plane cuts: under an A-pose armpit, or at the crotch, the
//! plane through a joint also crosses the torso or the other leg. Each
//! surface point is therefore first given to the nearest *bone* measured
//! through the body's volume (so a hand resting on a thigh still belongs to
//! the hand), and a joint's plane may only move faces between the two parts
//! it joins, and only within a ball around the joint that just holds the
//! limb's cross-section. Inside the ball the cut is exact: triangles are
//! split on the plane, so the rig attachment lies on the cut as Roblox's
//! validation requires. Outside it, the ownership boundary stands, smoothed.

use std::collections::{HashMap, HashSet};

use glam::{Vec2, Vec3};

use crate::fit::Fit;
use crate::mesh::{bounds, smooth_normals, weld, Mesh};
use crate::rig::{Joint, Part, Side};
use crate::voxel::Grid;
use crate::Error;

/// One body part's mesh, ready to become a `MeshPart`.
#[derive(Debug, Clone, PartialEq)]
pub struct PartMesh {
    pub part: Part,
    /// Centre of the part's bounds in Roblox space: the `MeshPart`'s
    /// position.
    pub centre: Vec3,
    /// Size of the part's bounds: the `MeshPart`'s `Size` and `InitialSize`.
    pub size: Vec3,
    /// Vertices relative to `centre`, unrotated, watertight.
    pub mesh: Mesh,
    /// Rig attachments (`<Joint>RigAttachment`) relative to `centre`. Roblox
    /// requires them unrotated, so a position is all they carry. The part's
    /// own joint comes first, then its children's.
    pub attachments: Vec<(String, Vec3)>,
    /// The R15 accessory attachments (`HatAttachment`, `LeftGripAttachment`
    /// and the rest) the Mannequin has on this part, relative to `centre`
    /// and unrotated.
    pub accessories: Vec<(String, Vec3)>,
    /// How many of `mesh.triangles`, at the end, are caps closing the cuts
    /// (or holes the source already had).
    pub cap_triangles: usize,
}

/// The 15 parts, in [`Part::ALL`] order.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub parts: Vec<PartMesh>,
    /// Triangle budgets exceeded and other things worth a look.
    pub warnings: Vec<String>,
}

impl Body {
    pub fn part(&self, part: Part) -> &PartMesh {
        &self.parts[part.index()]
    }

    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(|p| p.mesh.triangles.len()).sum()
    }
}

/// Roblox's Marketplace triangle budgets per body asset.
pub const BUDGETS: [(&str, &[Part], usize); 6] = [
    ("DynamicHead", &[Part::Head], 4000),
    ("Torso", &[Part::UpperTorso, Part::LowerTorso], 1750),
    (
        "LeftArm",
        &[Part::LeftUpperArm, Part::LeftLowerArm, Part::LeftHand],
        1248,
    ),
    (
        "RightArm",
        &[Part::RightUpperArm, Part::RightLowerArm, Part::RightHand],
        1248,
    ),
    (
        "LeftLeg",
        &[Part::LeftUpperLeg, Part::LeftLowerLeg, Part::LeftFoot],
        1248,
    ),
    (
        "RightLeg",
        &[Part::RightUpperLeg, Part::RightLowerLeg, Part::RightFoot],
        1248,
    ),
];

/// The hip cut leans this far from square to the thigh, rising towards the
/// outside: the "panty line" from the crotch to above the hip joint. 34° is
/// the Mannequin's: its crotch is 0.04·h below and 0.059·h inside the hip.
const HIP_TILT_DEGREES: f32 = 34.;

/// Cuts `mesh` (in its file's space) into the 15 parts placed by `fit`.
pub fn split(mesh: &Mesh, fit: &Fit) -> Result<Body, Error> {
    split_with(mesh, fit, 128)
}

/// [`split`] with the ownership voxel grid at `resolution` cells along the
/// longest side.
pub fn split_with(mesh: &Mesh, fit: &Fit, resolution: usize) -> Result<Body, Error> {
    let triangles: Vec<[u32; 3]> = mesh.valid_triangles().collect();
    if triangles.is_empty() {
        return Err(Error::Empty);
    }
    let positions: Vec<Vec3> = mesh.positions.iter().map(|&p| fit.to_roblox(p)).collect();
    let (lo, hi) = bounds(triangles.iter().flatten().map(|&i| positions[i as usize]));
    let h = (hi.y - lo.y).max(f32::MIN_POSITIVE);
    let welded = weld(&positions, 1e-5 * (hi - lo).length());

    let faces: Vec<[u32; 3]> = triangles
        .iter()
        .copied()
        .filter(|t| {
            let w = t.map(|i| welded.of[i as usize]);
            w[0] != w[1] && w[1] != w[2] && w[0] != w[2]
        })
        .collect();
    let welded_faces: Vec<[u32; 3]> = faces
        .iter()
        .map(|t| t.map(|i| welded.of[i as usize]))
        .collect();
    let normals = if mesh.normals.len() == mesh.positions.len() {
        mesh.normals
            .iter()
            .map(|&n| {
                fit.direction_to_roblox(n)
                    .try_normalize()
                    .unwrap_or(Vec3::Y)
            })
            .collect()
    } else {
        let smooth = smooth_normals(&welded.positions, &welded_faces);
        welded.of.iter().map(|&w| smooth[w as usize]).collect()
    };
    let uvs = (mesh.uvs.len() == mesh.positions.len()).then(|| mesh.uvs.clone());

    let grid = Grid::build(&welded.positions, &welded_faces, resolution);
    let owners = ownership(&grid, &welded.positions, fit);
    let faces = faces
        .iter()
        .zip(&welded_faces)
        .map(|(&r, w)| Face {
            r,
            label: Part::ALL[majority(w.map(|w| owners[w as usize])) as usize],
            locked: false,
        })
        .collect();
    let mut work = Work {
        pos: welded.positions,
        rw: welded.of,
        normal: normals,
        uv: uvs,
        faces,
    };

    let eps = 1e-5 * h;
    for joint in [
        Joint::Waist,
        Joint::Neck,
        Joint::LeftShoulder,
        Joint::RightShoulder,
        Joint::LeftHip,
        Joint::RightHip,
        Joint::LeftElbow,
        Joint::RightElbow,
        Joint::LeftKnee,
        Joint::RightKnee,
        Joint::LeftWrist,
        Joint::RightWrist,
        Joint::LeftAnkle,
        Joint::RightAnkle,
    ] {
        let (point, normal) = plane(fit, joint);
        let radius = reach(&grid, fit, joint, normal, h);
        let parent = joint.parent_part().expect("only Root has no parent part");
        // Only the shoulder and hip planes run on through the torso.
        let through_torso = matches!(
            joint,
            Joint::LeftShoulder | Joint::RightShoulder | Joint::LeftHip | Joint::RightHip
        );
        let side = joint.side().filter(|_| through_torso);
        work.cut(point, normal, radius, parent, joint.part(), side, eps);
    }
    work.smooth(2);

    let mut parts = Vec::with_capacity(15);
    for part in Part::ALL {
        let piece = work.extract(part, fit);
        if piece.mesh.triangles.is_empty() {
            return Err(Error::EmptyPart(part));
        }
        parts.push(piece);
    }
    let mut warnings = Vec::new();
    for (asset, members, budget) in BUDGETS {
        let count: usize = members
            .iter()
            .map(|p| parts[p.index()].mesh.triangles.len())
            .sum();
        if count > budget {
            warnings.push(format!(
                "{asset} has {count} triangles, over Roblox\u{2019}s budget of {budget} for Marketplace bodies; decimate the mesh to publish it there."
            ));
        }
    }
    Ok(Body { parts, warnings })
}

fn majority(o: [u16; 3]) -> u16 {
    if o[1] == o[2] {
        o[1]
    } else {
        o[0]
    }
}

/// The bone segments each part owns. The torso parts own a little skeleton
/// each (spine plus collarbones, spine plus pelvis) so the flesh of the
/// shoulders and hips is measured against something torso-shaped.
pub(crate) fn segments(fit: &Fit) -> Vec<(Part, Vec3, Vec3)> {
    let j = |joint| fit.joint(joint);
    let e = &fit.extremities;
    let spine_at = |y: f32| {
        let (w, n) = (j(Joint::Waist), j(Joint::Neck));
        let t = ((y - w.y) / (n.y - w.y)).clamp(0., 1.);
        w.lerp(n, if t.is_finite() { t } else { 1. })
    };
    let mut out = vec![
        (Part::Head, j(Joint::Neck), e.head),
        (Part::UpperTorso, j(Joint::Waist), j(Joint::Neck)),
        (Part::LowerTorso, j(Joint::Root), j(Joint::Waist)),
    ];
    for side in [Side::Left, Side::Right] {
        let (s, el, w, hp, k, a) = match side {
            Side::Left => (
                Joint::LeftShoulder,
                Joint::LeftElbow,
                Joint::LeftWrist,
                Joint::LeftHip,
                Joint::LeftKnee,
                Joint::LeftAnkle,
            ),
            Side::Right => (
                Joint::RightShoulder,
                Joint::RightElbow,
                Joint::RightWrist,
                Joint::RightHip,
                Joint::RightKnee,
                Joint::RightAnkle,
            ),
        };
        let ankle = j(a);
        out.extend([
            (Part::UpperTorso, spine_at(j(s).y), j(s)),
            (Part::LowerTorso, j(Joint::Root), j(hp)),
            (s.part(), j(s), j(el)),
            (el.part(), j(el), j(w)),
            (w.part(), j(w), e.hand(side)),
            (hp.part(), j(hp), j(k)),
            (k.part(), j(k), ankle),
            (a.part(), ankle, e.foot(side)),
            // The heel, below the ankle.
            (a.part(), ankle, Vec3::new(ankle.x, 0., ankle.z)),
        ]);
    }
    out
}

/// Owner part (as a [`Part::ALL`] index) of every welded vertex: the bone
/// nearest through the volume.
pub(crate) fn ownership(grid: &Grid, positions: &[Vec3], fit: &Fit) -> Vec<u16> {
    let segs = segments(fit);
    let mut seeds = Vec::new();
    let mut claimed = HashSet::new();
    for (part, a, b) in &segs {
        let n = (((*b - *a).length() / (0.5 * grid.cell)).ceil() as usize).max(1);
        for k in 0..=n {
            let p = a.lerp(*b, k as f32 / n as f32);
            if let Some(v) = grid.voxel(p).filter(|&v| grid.solid[v]) {
                if claimed.insert(v) {
                    seeds.push((v, part.index() as u16));
                }
            }
        }
    }
    let geo = grid.geodesic(&seeds);
    let nearest_bone = |p: Vec3| {
        segs.iter()
            .min_by(|x, y| {
                crate::fit::segment_distance(p, x.1, x.2)
                    .total_cmp(&crate::fit::segment_distance(p, y.1, y.2))
            })
            .map_or(Part::UpperTorso.index() as u16, |s| s.0.index() as u16)
    };
    positions
        .iter()
        .map(|&p| {
            // The vertex's own voxel, else the closest labelled neighbour.
            let q = ((p - grid.origin) / grid.cell).floor();
            let mut best: Option<(f32, u16)> = None;
            for dz in -1i64..=1 {
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let (x, y, z) = (q.x as i64 + dx, q.y as i64 + dy, q.z as i64 + dz);
                        if x < 0
                            || y < 0
                            || z < 0
                            || x >= grid.dims[0] as i64
                            || y >= grid.dims[1] as i64
                            || z >= grid.dims[2] as i64
                        {
                            continue;
                        }
                        let v = grid.index(x as usize, y as usize, z as usize);
                        if geo.label[v] == u16::MAX {
                            continue;
                        }
                        let d = geo.dist[v] * grid.cell + (grid.centre(v) - p).length();
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, geo.label[v]));
                        }
                    }
                }
            }
            best.map_or_else(|| nearest_bone(p), |(_, l)| l)
        })
        .collect()
}

/// The cut plane of `joint`: a point on it and the normal pointing into the
/// child part.
pub(crate) fn plane(fit: &Fit, joint: Joint) -> (Vec3, Vec3) {
    let j = |x: Joint| fit.joint(x);
    let p = j(joint);
    let dir = |a: Vec3, b: Vec3| (b - a).try_normalize().unwrap_or(Vec3::NEG_Y);
    let normal = match joint {
        Joint::Root | Joint::Waist | Joint::Neck => Vec3::Y,
        Joint::LeftShoulder | Joint::RightShoulder => dir(p, j(child_joint(joint).expect("arm"))),
        Joint::LeftElbow | Joint::RightElbow | Joint::LeftKnee | Joint::RightKnee => {
            let parent = j(parent_joint(joint));
            let child = j(child_joint(joint).expect("limb"));
            (dir(parent, p) + dir(p, child))
                .try_normalize()
                .unwrap_or(dir(p, child))
        }
        Joint::LeftWrist | Joint::RightWrist | Joint::LeftAnkle | Joint::RightAnkle => {
            dir(j(parent_joint(joint)), p)
        }
        Joint::LeftHip | Joint::RightHip => {
            let thigh = dir(p, j(child_joint(joint).expect("leg")));
            let side = joint.side().expect("paired").sign();
            let lateral = Vec3::new(side, 0., 0.);
            let lateral = (lateral - thigh * lateral.dot(thigh))
                .try_normalize()
                .unwrap_or(lateral);
            let t = HIP_TILT_DEGREES.to_radians();
            (thigh * t.cos() + lateral * t.sin()).normalize()
        }
    };
    (p, normal)
}

fn child_joint(joint: Joint) -> Option<Joint> {
    Joint::ALL
        .into_iter()
        .find(|j| j.parent_part() == Some(joint.part()) && j.side() == joint.side())
}

fn parent_joint(joint: Joint) -> Joint {
    joint.parent_part().map_or(Joint::Root, |p| p.joint())
}

/// How far from `joint` its cut may reach: past the limb's cross-section
/// next to the joint, and no farther. Shoulder and hip planes run through
/// the torso, so their limb is measured down the bone instead, at the
/// thinnest of three probes: near the shoulder of an A-pose arm the section
/// still runs into the torso's side, and a ball that wide would hand the
/// whole flank to the arm.
pub(crate) fn reach(grid: &Grid, fit: &Fit, joint: Joint, normal: Vec3, h: f32) -> f32 {
    let p = fit.joint(joint);
    let measure = |probe: Vec3, along: Vec3| {
        let s = grid.section(probe, along, 0.25 * h);
        (s.found && !s.truncated).then_some(s.reach)
    };
    let limb = match joint {
        Joint::LeftShoulder | Joint::RightShoulder | Joint::LeftHip | Joint::RightHip => {
            let child = fit.joint(child_joint(joint).expect("limb"));
            let along = (child - p).try_normalize().unwrap_or(normal);
            [0.3, 0.5, 0.7]
                .into_iter()
                .filter_map(|t| measure(p.lerp(child, t), along))
                .reduce(f32::min)
                .map(|r| r.min(0.12 * h))
        }
        _ => measure(p + normal * 2. * grid.cell, normal),
    };
    limb.map_or(0.08 * h, |r| 1.3 * r + 2. * grid.cell)
}

#[derive(Debug, Clone, Copy)]
struct Face {
    /// Render vertices.
    r: [u32; 3],
    label: Part,
    /// Placed by a cut plane; smoothing leaves it alone.
    locked: bool,
}

struct Work {
    /// Welded positions.
    pos: Vec<Vec3>,
    /// Render vertex → welded vertex.
    rw: Vec<u32>,
    normal: Vec<Vec3>,
    uv: Option<Vec<Vec2>>,
    faces: Vec<Face>,
}

fn edge(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

impl Work {
    fn welded(&self, f: &Face) -> [u32; 3] {
        f.r.map(|r| self.rw[r as usize])
    }

    /// Welded edge → faces using it.
    fn adjacency(&self) -> HashMap<(u32, u32), Vec<usize>> {
        let mut map: HashMap<(u32, u32), Vec<usize>> = HashMap::with_capacity(self.faces.len() * 2);
        for (i, f) in self.faces.iter().enumerate() {
            let w = self.welded(f);
            for k in 0..3 {
                map.entry(edge(w[k], w[(k + 1) % 3])).or_default().push(i);
            }
        }
        map
    }

    /// Moves faces between `parent` and `child` by the plane through `point`
    /// with `normal` (pointing into the child), within `radius` of `point`.
    #[allow(clippy::too_many_arguments)]
    fn cut(
        &mut self,
        point: Vec3,
        normal: Vec3,
        radius: f32,
        parent: Part,
        child: Part,
        side: Option<Side>,
        eps: f32,
    ) {
        let d: Vec<f32> = self.pos.iter().map(|p| (*p - point).dot(normal)).collect();
        let sign = |w: u32| {
            let v = d[w as usize];
            if v > eps {
                1i8
            } else if v < -eps {
                -1
            } else {
                0
            }
        };
        let candidate: Vec<bool> = self
            .faces
            .iter()
            .map(|f| {
                if f.label != parent && f.label != child {
                    return false;
                }
                let t = self.welded(f).map(|w| self.pos[w as usize]);
                // A shoulder or hip never takes torso flesh from the other side.
                if let (Some(side), true) = (side, f.label == parent) {
                    if (t[0] + t[1] + t[2]).x / 3. * side.sign() < 0. {
                        return false;
                    }
                }
                triangle_distance(point, t) <= radius
            })
            .collect();

        // Every face sharing a crossing edge with a face that is split must
        // be split as well, or the shared edge would gain a vertex on one
        // side only (a T-junction, and a hole once parts are separated).
        let adjacency = self.adjacency();
        let mut crossing: HashSet<(u32, u32)> = HashSet::new();
        let mut queue = Vec::new();
        let crossing_edges = |w: [u32; 3]| {
            (0..3).filter_map(move |k| {
                let (a, b) = (w[k], w[(k + 1) % 3]);
                (sign(a) * sign(b) < 0).then_some(edge(a, b))
            })
        };
        for (i, f) in self.faces.iter().enumerate() {
            if candidate[i] {
                for e in crossing_edges(self.welded(f)) {
                    if crossing.insert(e) {
                        queue.push(e);
                    }
                }
            }
        }
        let mut split = vec![false; self.faces.len()];
        while let Some(e) = queue.pop() {
            for &i in adjacency.get(&e).into_iter().flatten() {
                if split[i] {
                    continue;
                }
                split[i] = true;
                for e2 in crossing_edges(self.welded(&self.faces[i])) {
                    if crossing.insert(e2) {
                        queue.push(e2);
                    }
                }
            }
        }

        let mut wmap: HashMap<(u32, u32), u32> = HashMap::new();
        let mut rmap: HashMap<(u32, u32), u32> = HashMap::new();
        let old = std::mem::take(&mut self.faces);
        let mut faces = Vec::with_capacity(old.len() + old.len() / 8);
        for (i, f) in old.into_iter().enumerate() {
            let w = f.r.map(|r| self.rw[r as usize]);
            let s = w.map(sign);
            let relabel = |positive: bool, negative: bool, f: &Face| {
                if !candidate[i] {
                    (f.label, f.locked)
                } else if positive {
                    (child, true)
                } else if negative {
                    (parent, true)
                } else {
                    (f.label, f.locked)
                }
            };
            if !split[i] || !(s.contains(&1) && s.contains(&-1)) {
                let (label, locked) = relabel(s.contains(&1), s.contains(&-1), &f);
                faces.push(Face { label, locked, ..f });
                continue;
            }
            let (mut above, mut below) = (Vec::with_capacity(4), Vec::with_capacity(4));
            for k in 0..3 {
                let (a, b) = (f.r[k], f.r[(k + 1) % 3]);
                let (sa, sb) = (s[k], s[(k + 1) % 3]);
                if sa >= 0 {
                    above.push(a);
                }
                if sa <= 0 {
                    below.push(a);
                }
                if sa * sb < 0 {
                    let v = self.edge_vertex(a, b, &d, &mut wmap, &mut rmap);
                    above.push(v);
                    below.push(v);
                }
            }
            for (poly, positive) in [(above, true), (below, false)] {
                let (label, locked) = relabel(positive, !positive, &f);
                for k in 1..poly.len().saturating_sub(1) {
                    faces.push(Face {
                        r: [poly[0], poly[k], poly[k + 1]],
                        label,
                        locked,
                    });
                }
            }
        }
        self.faces = faces;
    }

    /// The render vertex where edge `a`–`b` crosses the plane (distances
    /// `d`), shared by every face using the same edge.
    fn edge_vertex(
        &mut self,
        a: u32,
        b: u32,
        d: &[f32],
        wmap: &mut HashMap<(u32, u32), u32>,
        rmap: &mut HashMap<(u32, u32), u32>,
    ) -> u32 {
        let (wa, wb) = (self.rw[a as usize], self.rw[b as usize]);
        let key = edge(wa, wb);
        let w = *wmap.entry(key).or_insert_with(|| {
            // Interpolate from the lower index so both faces on the edge get
            // bit-identical positions.
            let (lo, hi) = key;
            let t = d[lo as usize] / (d[lo as usize] - d[hi as usize]);
            self.pos
                .push(self.pos[lo as usize].lerp(self.pos[hi as usize], t));
            (self.pos.len() - 1) as u32
        });
        let rkey = edge(a, b);
        *rmap.entry(rkey).or_insert_with(|| {
            let (lo, hi) = rkey;
            let (dl, dh) = (
                d[self.rw[lo as usize] as usize],
                d[self.rw[hi as usize] as usize],
            );
            let t = dl / (dl - dh);
            let n = self.normal[lo as usize]
                .lerp(self.normal[hi as usize], t)
                .try_normalize()
                .unwrap_or(self.normal[lo as usize]);
            self.normal.push(n);
            if let Some(uv) = &mut self.uv {
                uv.push(uv[lo as usize].lerp(uv[hi as usize], t));
            }
            self.rw.push(w);
            (self.rw.len() - 1) as u32
        })
    }

    /// Majority smoothing of the unlocked labels over edge neighbours: a
    /// face whose neighbours agree on another part joins it. Cleans the
    /// ragged ownership boundary away from the cut planes.
    fn smooth(&mut self, rounds: usize) {
        let adjacency = self.adjacency();
        for _ in 0..rounds {
            let labels: Vec<Part> = self.faces.iter().map(|f| f.label).collect();
            for i in 0..self.faces.len() {
                if self.faces[i].locked {
                    continue;
                }
                let w = self.welded(&self.faces[i]);
                let mut votes: Vec<Part> = Vec::with_capacity(3);
                for k in 0..3 {
                    let e = edge(w[k], w[(k + 1) % 3]);
                    if let Some(other) = adjacency[&e].iter().find(|&&j| j != i) {
                        votes.push(labels[*other]);
                    }
                }
                for &candidate in &votes {
                    if candidate != labels[i]
                        && votes.iter().filter(|&&v| v == candidate).count() >= 2
                    {
                        self.faces[i].label = candidate;
                        break;
                    }
                }
            }
        }
    }

    /// One part's faces as a capped, recentred mesh with its attachments.
    fn extract(&self, part: Part, fit: &Fit) -> PartMesh {
        let mut faces: Vec<&Face> = self.faces.iter().filter(|f| f.label == part).collect();
        // A face hanging on by one edge (two or three of its edges open) would
        // be capped by its own mirror image, a zero-volume pocket whose edges
        // are used four times. It is left out instead; the part across its
        // open edges caps over the spot.
        for _ in 0..8 {
            let edges: HashSet<(u32, u32)> = faces
                .iter()
                .flat_map(|f| {
                    let w = self.welded(f);
                    (0..3).map(move |k| (w[k], w[(k + 1) % 3]))
                })
                .collect();
            let before = faces.len();
            faces.retain(|f| {
                let w = self.welded(f);
                (0..3)
                    .filter(|&k| !edges.contains(&(w[(k + 1) % 3], w[k])))
                    .count()
                    < 2
            });
            if faces.len() == before {
                break;
            }
        }
        let mut local: HashMap<u32, u32> = HashMap::new();
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut triangles = Vec::with_capacity(faces.len());
        let mut push =
            |r: u32, positions: &mut Vec<Vec3>, normals: &mut Vec<Vec3>, uvs: &mut Vec<Vec2>| {
                *local.entry(r).or_insert_with(|| {
                    positions.push(self.pos[self.rw[r as usize] as usize]);
                    normals.push(self.normal[r as usize]);
                    if let Some(uv) = &self.uv {
                        uvs.push(uv[r as usize]);
                    }
                    (positions.len() - 1) as u32
                })
            };
        // Welded vertex → one of its local render vertices, for the caps.
        let mut any_local: HashMap<u32, u32> = HashMap::new();
        let mut half: HashSet<(u32, u32)> = HashSet::new();
        let mut ordered = Vec::new();
        for f in &faces {
            let t = f.r.map(|r| push(r, &mut positions, &mut normals, &mut uvs));
            triangles.push(t);
            let w = self.welded(f);
            for k in 0..3 {
                any_local.entry(w[k]).or_insert(t[k]);
                let he = (w[k], w[(k + 1) % 3]);
                half.insert(he);
                ordered.push(he);
            }
        }

        let cut_faces = triangles.len();
        // Boundary loops, in face order for determinism.
        let boundary: Vec<(u32, u32)> = ordered
            .into_iter()
            .filter(|&(a, b)| !half.contains(&(b, a)))
            .collect();
        let mut next: HashMap<u32, Vec<u32>> = HashMap::new();
        for &(a, b) in &boundary {
            next.entry(a).or_default().push(b);
        }
        for &(start, _) in &boundary {
            while next.get(&start).is_some_and(|l| !l.is_empty()) {
                let mut ring = vec![start];
                let mut at = start;
                let closed = loop {
                    let Some(b) = next.get_mut(&at).and_then(|l| l.pop()) else {
                        break false;
                    };
                    if b == start {
                        break true;
                    }
                    ring.push(b);
                    at = b;
                    if ring.len() > boundary.len() {
                        break false;
                    }
                };
                if closed && ring.len() >= 3 {
                    let points: Vec<Vec3> = ring.iter().map(|&w| self.pos[w as usize]).collect();
                    let (mut tris, normal) = cap(&points);
                    // A diagonal that is already an edge of the part (a flat
                    // patch of a shell capped by its own outline) would be
                    // used four times; a fan round a new centre vertex has no
                    // diagonals at all.
                    let m = ring.len();
                    let clash = tris.iter().any(|t| {
                        (0..3).any(|k| {
                            let (a, b) = (t[k], t[(k + 1) % 3]);
                            let consecutive = (a + 1) % m == b || (b + 1) % m == a;
                            let (wa, wb) = (ring[a], ring[b]);
                            !consecutive && (half.contains(&(wa, wb)) || half.contains(&(wb, wa)))
                        })
                    });
                    let base = positions.len() as u32;
                    for &w in &ring {
                        positions.push(self.pos[w as usize]);
                        normals.push(-normal);
                        if self.uv.is_some() {
                            uvs.push(uvs[any_local[&w] as usize]);
                        }
                    }
                    if clash {
                        positions.push(points.iter().sum::<Vec3>() / m as f32);
                        normals.push(-normal);
                        if self.uv.is_some() {
                            let start = uvs.len() - m;
                            let mean = uvs[start..].iter().sum::<Vec2>() / m as f32;
                            uvs.push(mean);
                        }
                        tris = (0..m).map(|k| [k, (k + 1) % m, m]).collect();
                    }
                    // Reversed: the cap runs each boundary edge the other way.
                    for [a, b, c] in tris {
                        triangles.push([base + a as u32, base + c as u32, base + b as u32]);
                    }
                }
            }
        }

        let triangles_len = triangles.len();
        let (lo, hi) = bounds(positions.iter().copied());
        let centre = (lo + hi) / 2.;
        for p in positions.iter_mut() {
            *p -= centre;
        }
        let mut attachments = Vec::new();
        for joint in Joint::ALL {
            if joint.part() == part || joint.parent_part() == Some(part) {
                attachments.push((joint.attachment(), fit.joint(joint) - centre));
            }
        }
        attachments.sort_by_key(|(name, _)| name != &part.joint().attachment());
        PartMesh {
            part,
            centre,
            size: hi - lo,
            mesh: Mesh {
                positions,
                normals,
                uvs,
                triangles,
                up_hint: None,
                front_hint: None,
            },
            attachments,
            accessories: crate::accessory::accessories(part, fit, lo, hi),
            cap_triangles: triangles_len - cut_faces,
        }
    }
}

/// Closest distance from `p` to triangle `t` (Ericson's region test).
fn triangle_distance(p: Vec3, t: [Vec3; 3]) -> f32 {
    let [a, b, c] = t;
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0. && d2 <= 0. {
        return ap.length();
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0. && d4 <= d3 {
        return bp.length();
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0. && d1 >= 0. && d3 <= 0. {
        let v = d1 / (d1 - d3);
        return (p - (a + ab * v)).length();
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0. && d5 <= d6 {
        return cp.length();
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0. && d2 >= 0. && d6 <= 0. {
        let w = d2 / (d2 - d6);
        return (p - (a + ac * w)).length();
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0. && (d4 - d3) >= 0. && (d5 - d6) >= 0. {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (p - (b + (c - b) * w)).length();
    }
    let denom = 1. / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    (p - (a + ab * v + ac * w)).length()
}

/// Triangulates a boundary loop flat on its best-fit plane by ear clipping.
/// Returns triangles wound like the loop and the loop's unit normal (Newell),
/// which points to the side the loop winds anticlockwise around.
pub(crate) fn cap(points: &[Vec3]) -> (Vec<[usize; 3]>, Vec3) {
    let n = points.len();
    let mut normal = Vec3::ZERO;
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        normal += Vec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    let normal = normal.try_normalize().unwrap_or(Vec3::Y);
    let u = normal.any_orthonormal_vector();
    let v = normal.cross(u);
    let flat: Vec<Vec2> = points
        .iter()
        .map(|p| Vec2::new(p.dot(u), p.dot(v)))
        .collect();
    let cross = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a);
    let inside = |p: Vec2, a: Vec2, b: Vec2, c: Vec2| {
        cross(a, b, p) >= 0. && cross(b, c, p) >= 0. && cross(c, a, p) >= 0.
    };
    let mut ring: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n.saturating_sub(2));
    while ring.len() > 3 {
        let m = ring.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (ring[(k + m - 1) % m], ring[k], ring[(k + 1) % m]);
            let (a, b, c) = (flat[ia], flat[ib], flat[ic]);
            let area = cross(a, b, c);
            if area < 0. {
                continue;
            }
            let blocked = area > 0.
                && ring.iter().any(|&j| {
                    j != ia
                        && j != ib
                        && j != ic
                        && flat[j] != a
                        && flat[j] != b
                        && flat[j] != c
                        && inside(flat[j], a, b, c)
                });
            if !blocked {
                out.push([ia, ib, ic]);
                ring.remove(k);
                clipped = true;
                break;
            }
        }
        if !clipped {
            // Self-intersecting loop: a fan still covers every edge once.
            for k in 1..ring.len() - 1 {
                out.push([ring[0], ring[k], ring[k + 1]]);
            }
            return (out, normal);
        }
    }
    out.push([ring[0], ring[1], ring[2]]);
    (out, normal)
}
