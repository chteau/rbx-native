//! Outer cages for the cut body: Roblox's own template cages, bent onto the
//! fit, so layered clothing can wrap the rig.
//!
//! A body's `<Part>_OuterCage` has to keep the vertices and UVs of the
//! template cage Roblox ships for its reference body (creator-docs,
//! `avatar/character-bodies/specifications.md`, "Outer cages"): layered
//! clothing pairs its inner cage with the body's vertex for vertex. An
//! envelope of our own would import, but no garment would match it. So each
//! template part cage is moved, never remeshed: the template part is turned
//! onto the fitted bone and scaled to the cut part, every vertex blends the
//! maps of its nearest bones so seam copies land on one point, and the
//! result is pushed out of the body along the cage's own normals and
//! relaxed so it stays a loose, even shell.
//!
//! The template cages are not shipped here (they are Roblox's assets): the
//! caller downloads them and hands them in as [`CageTemplate`]s.

mod closest;
mod winding;

use glam::{Quat, Vec3};

use self::closest::Surface;
use self::winding::Solid;
use crate::mesh::Mesh;
use crate::rig::Part;
use crate::split::Body;

/// One body part's template outer cage, as the reference body carries it.
#[derive(Debug, Clone, PartialEq)]
pub struct CageTemplate {
    pub part: Part,
    /// The cage in the template part's frame (its `CageOrigin` already
    /// added), with its UVs. Normals may be empty.
    pub mesh: Mesh,
    /// The template part's `InitialSize`: the body part the cage was made
    /// around, centred on the part's origin.
    pub size: Vec3,
    /// The template part's `<Joint>RigAttachment`s, in its frame, by name:
    /// its own joint's and its children's, as [`crate::PartMesh::attachments`].
    pub attachments: Vec<(String, Vec3)>,
}

/// Why the cages could not be fitted.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CageError {
    #[error("no template cage for {0}")]
    MissingTemplate(Part),
    #[error("the template {part} has no {attachment}")]
    MissingAttachment { part: Part, attachment: String },
}

/// The outer cage of every part of `body`, in [`Part::ALL`] order, each
/// relative to its part's `centre` like [`crate::PartMesh::mesh`]: the
/// template's vertex order, UVs and triangles, with moved positions and
/// recomputed normals.
pub fn fit_cages(body: &Body, templates: &[CageTemplate]) -> Result<Vec<Mesh>, CageError> {
    fit_cages_report(body, templates).map(|(cages, _)| cages)
}

/// [`fit_cages`], plus how many triangles of each part (in [`Part::ALL`]
/// order) the shrink-wrap turned over relative to the coarse fit.
pub fn fit_cages_report(
    body: &Body,
    templates: &[CageTemplate],
) -> Result<(Vec<Mesh>, Vec<usize>), CageError> {
    let tpl: Vec<&CageTemplate> = Part::ALL
        .into_iter()
        .map(|p| {
            templates
                .iter()
                .find(|t| t.part == p)
                .ok_or(CageError::MissingTemplate(p))
        })
        .collect::<Result<_, _>>()?;

    let mut centres: [Option<Vec3>; 15] = [None; 15];
    for p in Part::ALL {
        template_centre(p, &tpl, &mut centres)?;
    }

    let mut maps = Vec::with_capacity(15);
    let mut bones = Vec::with_capacity(15);
    for p in Part::ALL {
        let t = tpl[p.index()];
        let pm = body.part(p);
        let rot = rotation(p, &tpl, body)?;
        let axes = [Vec3::X, Vec3::Y, Vec3::Z].map(|u| rot * u);
        let mut scale = [1.0f32; 3];
        let mut mid = [0.0f32; 3];
        for (k, axis) in axes.iter().enumerate() {
            let (lo, hi) = pm
                .mesh
                .positions
                .iter()
                .map(|q| axis.dot(*q))
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), d| {
                    (l.min(d), h.max(d))
                });
            if lo.is_finite() && hi.is_finite() {
                mid[k] = (lo + hi) * 0.5;
                if t.size[k] > f32::EPSILON {
                    scale[k] = ((hi - lo) / t.size[k]).clamp(0.05, 20.0);
                }
            }
        }
        let centre_t = centres[p.index()].unwrap_or(Vec3::ZERO);
        let joint = centre_t + template_attachment(t, &p.joint().attachment())?;
        let end = match Part::ALL.into_iter().find(|c| c.parent() == Some(p)) {
            Some(c) => centre_t + template_attachment(t, &c.joint().attachment())?,
            None => joint + 2.0 * (centre_t - joint),
        };
        bones.push((joint, end));
        maps.push(PartMap {
            centre_t,
            centre_o: pm.centre,
            axes,
            scale,
            mid,
        });
    }

    let mut template_world = Vec::new();
    let mut owner = Vec::new();
    let mut offsets = Vec::with_capacity(15);
    for p in Part::ALL {
        offsets.push(template_world.len());
        for v in &tpl[p.index()].mesh.positions {
            template_world.push(centres[p.index()].unwrap_or(Vec3::ZERO) + *v);
            owner.push(p.index());
        }
    }
    let welded = crate::mesh::weld(&template_world, 1e-3);
    let n = welded.positions.len();
    // A group holding vertices of two parts is a seam: its copies must get
    // the very same coarse point, so no part may be favoured there.
    let mut group_part: Vec<Option<usize>> = vec![None; n];
    let mut seam = vec![false; n];
    for (i, &g) in welded.of.iter().enumerate() {
        match group_part[g as usize] {
            None => group_part[g as usize] = Some(owner[i]),
            Some(q) if q != owner[i] => seam[g as usize] = true,
            _ => {}
        }
    }

    // Each part's map alone disagrees with its neighbour's at the seams (a
    // full belly against a slim template hip), so every vertex blends the
    // maps of the bones nearest it, the way skinning does.
    let mut points = vec![Vec3::ZERO; n];
    let mut counts = vec![0u32; n];
    for (i, &g) in welded.of.iter().enumerate() {
        let w = template_world[i];
        let own = (!seam[g as usize]).then_some(owner[i]);
        points[g as usize] += blend(w, own, &maps, &bones);
        counts[g as usize] += 1;
    }
    for (pt, &c) in points.iter_mut().zip(&counts) {
        *pt /= c.max(1) as f32;
    }

    let mut cage = Cage {
        tris: Vec::with_capacity(15),
        members: Vec::with_capacity(15),
        neighbours: vec![Vec::new(); n],
    };
    for p in Part::ALL {
        let t = tpl[p.index()];
        let off = offsets[p.index()];
        let tris: Vec<[u32; 3]> = t
            .mesh
            .triangles
            .iter()
            .map(|tri| tri.map(|i| welded.of[off + i as usize]))
            .collect();
        for g in &tris {
            for (a, b) in [(g[0], g[1]), (g[1], g[2]), (g[2], g[0])] {
                if a != b {
                    cage.neighbours[a as usize].push(b);
                    cage.neighbours[b as usize].push(a);
                }
            }
        }
        let mut members: Vec<u32> = welded.of[off..off + t.mesh.positions.len()].to_vec();
        members.sort_unstable();
        members.dedup();
        cage.tris.push(tris);
        cage.members.push(members);
    }
    for list in &mut cage.neighbours {
        list.sort_unstable();
        list.dedup();
    }

    let coarse = points.clone();
    shrink_wrap(body, &mut points, &coarse, &cage);

    let part_mesh = |p: Part, from: &[Vec3]| -> Mesh {
        let t = tpl[p.index()];
        let off = offsets[p.index()];
        let centre = body.part(p).centre;
        let positions: Vec<Vec3> = (0..t.mesh.positions.len())
            .map(|i| from[welded.of[off + i] as usize] - centre)
            .collect();
        let normals = crate::mesh::smooth_normals(&positions, &t.mesh.triangles);
        Mesh {
            positions,
            normals,
            uvs: t.mesh.uvs.clone(),
            triangles: t.mesh.triangles.clone(),
            ..Mesh::default()
        }
    };
    let mut cages = Vec::with_capacity(15);
    let mut folds = Vec::with_capacity(15);
    for p in Part::ALL {
        let fitted = part_mesh(p, &points);
        folds.push(folded(&part_mesh(p, &coarse), &fitted));
        cages.push(fitted);
    }
    Ok((cages, folds))
}

/// One part's coarse map from the template's shared space to ours.
struct PartMap {
    centre_t: Vec3,
    centre_o: Vec3,
    axes: [Vec3; 3],
    scale: [f32; 3],
    mid: [f32; 3],
}

impl PartMap {
    fn apply(&self, w: Vec3) -> Vec3 {
        let v = w - self.centre_t;
        self.centre_o
            + (0..3)
                .map(|k| self.axes[k] * (v[k] * self.scale[k] + self.mid[k]))
                .sum::<Vec3>()
    }
}

fn segment_distance(p: Vec3, (a, b): (Vec3, Vec3)) -> f32 {
    let ab = b - a;
    let len = ab.length_squared();
    let t = if len > f32::EPSILON {
        ((p - a).dot(ab) / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

/// `w` mapped by the three nearest bones' maps, weighted by inverse fourth
/// power distance; `own`'s weight is doubled so a vertex mostly follows
/// its own part away from the seams.
fn blend(w: Vec3, own: Option<usize>, maps: &[PartMap], bones: &[(Vec3, Vec3)]) -> Vec3 {
    let mut weights: Vec<(usize, f32)> = bones
        .iter()
        .enumerate()
        .map(|(i, &b)| (i, 1.0 / (segment_distance(w, b).powi(4) + 1e-6)))
        .collect();
    weights.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
    weights.truncate(3);
    for (i, wt) in &mut weights {
        if Some(*i) == own {
            *wt *= 2.0;
        }
    }
    let total: f32 = weights.iter().map(|(_, wt)| wt).sum();
    weights
        .iter()
        .map(|&(i, wt)| maps[i].apply(w) * (wt / total))
        .sum()
}

/// Triangles of `fitted` facing against the same triangle of `coarse`: the
/// fit turned them over. Both meshes share one topology.
pub(crate) fn folded(coarse: &Mesh, fitted: &Mesh) -> usize {
    let normal = |m: &Mesh, t: &[u32; 3]| {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        (b - a).cross(c - a)
    };
    coarse
        .triangles
        .iter()
        .zip(&fitted.triangles)
        .filter(|(tc, tf)| normal(coarse, tc).dot(normal(fitted, tf)) < 0.0)
        .count()
}

/// The welded cage's topology: each part's triangles and points by welded
/// index, and every point's neighbours across all parts.
struct Cage {
    tris: Vec<Vec<[u32; 3]>>,
    members: Vec<Vec<u32>>,
    neighbours: Vec<Vec<u32>>,
}

impl Cage {
    /// Outward vertex normals. Each part's cage is an open shell whose
    /// winding the template does not promise, so each part is oriented on
    /// its own against its centroid; a seam point averages its parts.
    fn normals(&self, points: &[Vec3]) -> Vec<Vec3> {
        let mut acc = vec![Vec3::ZERO; points.len()];
        for (tris, members) in self.tris.iter().zip(&self.members) {
            if members.is_empty() {
                continue;
            }
            let normals = crate::mesh::smooth_normals(points, tris);
            let centroid = members.iter().map(|&i| points[i as usize]).sum::<Vec3>()
                / members.len() as f32;
            let facing: f32 = members
                .iter()
                .map(|&i| normals[i as usize].dot(points[i as usize] - centroid))
                .sum();
            let sign = if facing < 0.0 { -1.0 } else { 1.0 };
            for &i in members {
                acc[i as usize] += normals[i as usize] * sign;
            }
        }
        acc.into_iter()
            .map(|n| n.try_normalize().unwrap_or(Vec3::Y))
            .collect()
    }

    fn neighbour_mean(&self, i: usize, field: &[Vec3]) -> Option<Vec3> {
        let list = &self.neighbours[i];
        (!list.is_empty())
            .then(|| list.iter().map(|&j| field[j as usize]).sum::<Vec3>() / list.len() as f32)
    }
}

fn template_attachment(t: &CageTemplate, name: &str) -> Result<Vec3, CageError> {
    t.attachments
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| *v)
        .ok_or_else(|| CageError::MissingAttachment {
            part: t.part,
            attachment: name.to_string(),
        })
}

fn our_attachment(body: &Body, p: Part, name: &str) -> Vec3 {
    body.part(p)
        .attachments
        .iter()
        .find(|(n, _)| n == name)
        .map_or(Vec3::ZERO, |(_, v)| *v)
}

/// Template part centres chained through the rig attachments from the
/// LowerTorso, so the parts sit together the way the reference body does.
fn template_centre(
    p: Part,
    tpl: &[&CageTemplate],
    centres: &mut [Option<Vec3>; 15],
) -> Result<Vec3, CageError> {
    if let Some(c) = centres[p.index()] {
        return Ok(c);
    }
    let c = match p.parent() {
        None => Vec3::ZERO,
        Some(q) => {
            let name = p.joint().attachment();
            let qc = template_centre(q, tpl, centres)?;
            qc + template_attachment(tpl[q.index()], &name)?
                - template_attachment(tpl[p.index()], &name)?
        }
    };
    centres[p.index()] = Some(c);
    Ok(c)
}

/// Turns the template bone onto ours; parts with no child to aim at follow
/// their parent's turn.
fn rotation(p: Part, tpl: &[&CageTemplate], body: &Body) -> Result<Quat, CageError> {
    let Some(child) = Part::ALL.into_iter().find(|c| c.parent() == Some(p)) else {
        return match p.parent() {
            Some(q) => rotation(q, tpl, body),
            None => Ok(Quat::IDENTITY),
        };
    };
    let own = p.joint().attachment();
    let next = child.joint().attachment();
    let t = tpl[p.index()];
    let a_t = template_attachment(t, &next)? - template_attachment(t, &own)?;
    let a_o = our_attachment(body, p, &next) - our_attachment(body, p, &own);
    Ok(match (a_t.try_normalize(), a_o.try_normalize()) {
        (Some(from), Some(to)) => Quat::from_rotation_arc(from, to),
        _ => Quat::IDENTITY,
    })
}

/// Signed distance to the body: negative inside any part, judged
/// by winding number so edges and creases stay right. A cage that sits
/// inside any part would show that part through clothing, so no piece may
/// be exempt.
struct Field {
    surface: Surface,
    solid: Solid,
}

impl Field {
    fn new(body: &Body) -> Self {
        let world = |pm: &crate::PartMesh, t: &[u32; 3]| {
            t.map(|i| pm.centre + pm.mesh.positions[i as usize])
        };
        let solid = Solid::new(
            body.parts
                .iter()
                .map(|pm| pm.mesh.triangles.iter().map(|t| world(pm, t)).collect())
                .collect(),
        );
        // Caps sit inside the body, and so does a shell buried in another (a
        // neck reaching up into the head): measured against either, a point
        // deep under the skin would seem a hair from getting out and never be
        // pushed far enough. A face is outside when a point just off it is.
        let surface = body
            .parts
            .iter()
            .map(|pm| {
                let keep = pm.mesh.triangles.len().saturating_sub(pm.cap_triangles);
                pm.mesh.triangles[..keep]
                    .iter()
                    .map(|t| world(pm, t))
                    .filter(|t| {
                        (t[1] - t[0]).cross(t[2] - t[0]).try_normalize().is_some_and(|n| {
                            !solid.inside((t[0] + t[1] + t[2]) / 3.0 + n * 1e-3)
                        })
                    })
                    .collect()
            })
            .collect();
        Self {
            surface: Surface::new(surface),
            solid,
        }
    }

    fn probe(&self, q: Vec3) -> Option<Probe> {
        let (d, at, normal) = self.surface.closest(q)?;
        let inside = self.solid.inside(q);
        let away = if inside { at - q } else { q - at };
        // On the surface itself the way out is the face's own normal.
        let out = if d > 1e-5 { away / d } else { normal };
        Some(Probe {
            sd: if inside { -d } else { d },
            at,
            out,
        })
    }

    fn signed(&self, q: Vec3) -> Option<f32> {
        self.probe(q).map(|p| p.sd)
    }

    /// The move that takes `q` to `margin` outside: along the cage normal
    /// `n` when that gets it at least halfway there, else straight out
    /// through its closest exposed surface point. Deep under the skin a
    /// normal can run along a limb instead of out of it (a joint's buried
    /// rounded end), and following it would drag the point down the limb.
    fn push(&self, q: Vec3, n: Vec3, p: &Probe, margin: f32) -> Vec3 {
        let along = n * (margin - p.sd);
        let enough = p.sd + 0.5 * (margin - p.sd);
        if self.signed(q + along).is_some_and(|s| s >= enough) {
            along
        } else {
            p.at + p.out * margin - q
        }
    }

    /// One iteration's raw move for `q`: out if nearer than `margin`, in by
    /// half the excess over twice the margin if further than three margins
    /// (never past `margin`), else none.
    fn step(&self, q: Vec3, n: Vec3, margin: f32) -> Vec3 {
        let Some(p) = self.probe(q) else {
            return Vec3::ZERO;
        };
        if p.sd < margin {
            self.push(q, n, &p, margin)
        } else if p.sd > 3.0 * margin {
            let d = -n * (0.5 * (p.sd - 2.0 * margin));
            match self.signed(q + d) {
                Some(s) if s >= margin && s < p.sd => d,
                _ => Vec3::ZERO,
            }
        } else {
            Vec3::ZERO
        }
    }

    /// `q`, or if it is not clear of the body, the nearest spot that is,
    /// found by reaching ever further out through the closest surface point
    /// or along either way of the normal. Far enough, every direction
    /// leaves the body, so this always ends outside.
    fn way_out(&self, q: Vec3, n: Vec3, margin: f32) -> Vec3 {
        let clear = margin * 0.25;
        let Some(p) = self.probe(q).filter(|p| p.sd < clear) else {
            return q;
        };
        for k in 0..14 {
            let reach = margin * (1u32 << k) as f32;
            for c in [p.at + p.out * reach, q + n * reach, q - n * reach] {
                if self.signed(c).is_some_and(|s| s >= clear) {
                    return c;
                }
            }
        }
        q
    }
}

/// Where a point stands against the body.
struct Probe {
    /// Signed distance: negative inside.
    sd: f32,
    /// The closest exposed surface point.
    at: Vec3,
    /// The unit direction from `at` away from the body.
    out: Vec3,
}

/// Wraps the coarse cage onto the body as an even shell `margin` outside
/// it. Points travel along the cage's own normals rather than to their
/// closest body point: closest-point targets gather many points into the
/// same fold (a full belly, a low crotch) and tangle the cage.
fn shrink_wrap(body: &Body, points: &mut [Vec3], coarse: &[Vec3], cage: &Cage) {
    let field = Field::new(body);
    // A point on no triangle has no normal to travel along.
    let loose: Vec<bool> = cage.neighbours.iter().map(Vec::is_empty).collect();
    // Each point's offset from its neighbours' mean in the coarse cage: the
    // template's own spacing, which relaxation restores.
    let detail: Vec<Vec3> = (0..coarse.len())
        .map(|i| {
            cage.neighbour_mean(i, coarse)
                .map_or(Vec3::ZERO, |mean| coarse[i] - mean)
        })
        .collect();
    let (lo, hi) = crate::mesh::bounds(
        body.parts
            .iter()
            .flat_map(|pm| pm.mesh.positions.iter().map(move |v| pm.centre + *v)),
    );
    let height = if hi.y > lo.y { hi.y - lo.y } else { 5.2 };
    let margin = 0.02 * height / 5.2;
    // Points the field never had to move already sit where the template
    // put them; relaxing them too would drag a well-placed shell along
    // after every pushed neighbour and fold it.
    let mut active = vec![false; points.len()];

    for _ in 0..10 {
        let normals = cage.normals(points);
        let mut step: Vec<Vec3> = (0..points.len())
            .map(|i| {
                if loose[i] {
                    Vec3::ZERO
                } else {
                    field.step(points[i], normals[i], margin)
                }
            })
            .collect();
        let moving: Vec<bool> = step.iter().map(|d| *d != Vec3::ZERO).collect();
        for (a, &m) in active.iter_mut().zip(&moving) {
            *a |= m;
        }
        for _ in 0..3 {
            step = (0..step.len())
                .map(|i| match cage.neighbour_mean(i, &step) {
                    Some(mean) => step[i] * 0.5 + mean * 0.5,
                    None => step[i],
                })
                .collect();
        }
        // Only points that needed a move take one: a point already sitting
        // well in the shell would otherwise be dragged after its pushed
        // neighbours every pass and wander off its template place.
        for ((q, d), &m) in points.iter_mut().zip(&step).zip(&moving) {
            if m {
                *q += *d;
            }
        }
        // Moving only along normals bunches triangles where the body
        // narrows; sliding points within the surface evens them out
        // without undoing the push. They slide toward the template's own
        // spacing rather than the bare mean, which would flatten its finer
        // regions and drift every point off its template place.
        let normals = cage.normals(points);
        for _ in 0..2 {
            let moved: Vec<Vec3> = (0..points.len())
                .map(|i| {
                    let q = points[i];
                    if !active[i] {
                        return q;
                    }
                    let Some(mean) = cage.neighbour_mean(i, points) else {
                        return q;
                    };
                    let d = (mean + detail[i] - q) * 0.3;
                    q + d - normals[i] * d.dot(normals[i])
                })
                .collect();
            points.copy_from_slice(&moved);
        }
    }

    // Untangling can pull a point back under the skin and pushing it out
    // can fold a triangle again, so the two alternate.
    for _ in 0..ROUNDS {
        untangle(points, coarse, cage, &detail);
        push_out(&field, points, cage, &loose, margin);
    }
}

/// Untangle-then-push rounds at the end of [`shrink_wrap`].
const ROUNDS: usize = 4;

/// Smoothing and relaxing can leave a point under the skin; this
/// unsmoothed push brings every point back out. In a crease one push can
/// land under a neighbouring face, so it repeats.
fn push_out(field: &Field, points: &mut [Vec3], cage: &Cage, loose: &[bool], margin: f32) {
    let normals = cage.normals(points);
    for ((q, &n), &skip) in points.iter_mut().zip(&normals).zip(loose) {
        if skip {
            continue;
        }
        for _ in 0..8 {
            match field.probe(*q) {
                Some(p) if p.sd < margin * 0.5 => *q += field.push(*q, n, &p, margin),
                _ => break,
            }
        }
        *q = field.way_out(*q, n, margin);
    }
}

/// Relaxes the neighbourhood of every folded triangle back toward the
/// template's own spacing. Where the body creases (an armpit pressed to the
/// chest, a low crotch) pushing points out along their normals can turn a
/// triangle over; the fold shows through any garment wrapped on it, and
/// only its neighbourhood needs smoothing, not the whole shell.
fn untangle(points: &mut [Vec3], coarse: &[Vec3], cage: &Cage, detail: &[Vec3]) {
    let normal = |field: &[Vec3], t: &[u32; 3]| {
        let [a, b, c] = t.map(|i| field[i as usize]);
        (b - a).cross(c - a)
    };
    for _ in 0..UNTANGLE_PASSES {
        let mut marked = vec![false; points.len()];
        let mut any = false;
        for t in cage.tris.iter().flatten() {
            if normal(points, t).dot(normal(coarse, t)) < 0.0 {
                any = true;
                for &i in t {
                    marked[i as usize] = true;
                    for &j in &cage.neighbours[i as usize] {
                        marked[j as usize] = true;
                    }
                }
            }
        }
        if !any {
            break;
        }
        let moved: Vec<Vec3> = (0..points.len())
            .map(|i| match cage.neighbour_mean(i, points) {
                Some(mean) if marked[i] => points[i] + (mean + detail[i] - points[i]) * 0.5,
                _ => points[i],
            })
            .collect();
        points.copy_from_slice(&moved);
    }
}

/// How many relaxation passes [`untangle`] may take before giving up on a
/// fold.
const UNTANGLE_PASSES: usize = 30;
