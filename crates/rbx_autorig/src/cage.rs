//! Outer cages for the cut body: Roblox's own template cages, bent onto the
//! fit, so layered clothing can wrap the rig.
//!
//! A body's `<Part>_OuterCage` has to keep the vertices and UVs of the
//! template cage Roblox ships for its reference body (creator-docs,
//! `avatar/character-bodies/specifications.md`, "Outer cages"): layered
//! clothing pairs its inner cage with the body's vertex for vertex. An
//! envelope of our own would import, but no garment would match it. So each
//! template part cage is moved, never remeshed: the template part is turned
//! onto the fitted bone and scaled to the cut part, the seams shared between
//! parts are welded again, and the result is pushed out of the body and
//! smoothed so it stays a loose, even shell.
//!
//! The template cages are not shipped here (they are Roblox's assets): the
//! caller downloads them and hands them in as [`CageTemplate`]s.

mod closest;

use glam::{Quat, Vec3};

use self::closest::Surface;
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

    // Coarse map of every render vertex, in template world and our world.
    let mut template_world = Vec::new();
    let mut ours_world = Vec::new();
    let mut offsets = Vec::with_capacity(15);
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
        offsets.push(template_world.len());
        for v in &t.mesh.positions {
            template_world.push(centre_t + *v);
            let local: Vec3 = (0..3).map(|k| axes[k] * (v[k] * scale[k] + mid[k])).sum();
            ours_world.push(pm.centre + local);
        }
    }

    let welded = crate::mesh::weld(&template_world, 1e-3);
    let n = welded.positions.len();
    let mut points = vec![Vec3::ZERO; n];
    let mut counts = vec![0u32; n];
    for (i, &g) in welded.of.iter().enumerate() {
        points[g as usize] += ours_world[i];
        counts[g as usize] += 1;
    }
    for (pt, &c) in points.iter_mut().zip(&counts) {
        *pt /= c.max(1) as f32;
    }

    let mut neighbours: Vec<Vec<u32>> = vec![Vec::new(); n];
    for p in Part::ALL {
        let off = offsets[p.index()];
        for tri in &tpl[p.index()].mesh.triangles {
            let g = tri.map(|i| welded.of[off + i as usize]);
            for (a, b) in [(g[0], g[1]), (g[1], g[2]), (g[2], g[0])] {
                if a != b {
                    neighbours[a as usize].push(b);
                    neighbours[b as usize].push(a);
                }
            }
        }
    }
    for list in &mut neighbours {
        list.sort_unstable();
        list.dedup();
    }

    shrink_wrap(body, &mut points, &neighbours);

    Ok(Part::ALL
        .into_iter()
        .map(|p| {
            let t = tpl[p.index()];
            let off = offsets[p.index()];
            let centre = body.part(p).centre;
            let positions: Vec<Vec3> = (0..t.mesh.positions.len())
                .map(|i| points[welded.of[off + i] as usize] - centre)
                .collect();
            let normals = crate::mesh::smooth_normals(&positions, &t.mesh.triangles);
            Mesh {
                positions,
                normals,
                uvs: t.mesh.uvs.clone(),
                triangles: t.mesh.triangles.clone(),
                ..Mesh::default()
            }
        })
        .collect())
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

/// Pulls the coarse cage onto the body as an even shell `margin` outside it,
/// smoothing the moves so neighbouring points travel together.
fn shrink_wrap(body: &Body, points: &mut [Vec3], neighbours: &[Vec<u32>]) {
    let surface = Surface::new(
        body.parts
            .iter()
            .map(|pm| {
                let keep = pm.mesh.triangles.len().saturating_sub(pm.cap_triangles);
                pm.mesh.triangles[..keep]
                    .iter()
                    .map(|t| t.map(|i| pm.centre + pm.mesh.positions[i as usize]))
                    .collect()
            })
            .collect(),
    );
    let (lo, hi) = crate::mesh::bounds(
        body.parts
            .iter()
            .flat_map(|pm| pm.mesh.positions.iter().map(move |v| pm.centre + *v)),
    );
    let height = if hi.y > lo.y { hi.y - lo.y } else { 5.2 };
    let margin = 0.02 * height / 5.2;

    for _ in 0..3 {
        let mut disp: Vec<Vec3> = points
            .iter()
            .map(|&q| {
                let Some((s, n)) = surface.closest(q) else {
                    return Vec3::ZERO;
                };
                let target = s + n * margin;
                if (q - s).dot(n) < margin {
                    target - q
                } else if q.distance(s) > 4.0 * margin {
                    (target - q) * 0.5
                } else {
                    Vec3::ZERO
                }
            })
            .collect();
        for _ in 0..4 {
            disp = disp
                .iter()
                .zip(neighbours)
                .map(|(&d, list)| {
                    if list.is_empty() {
                        return d;
                    }
                    let mean = list.iter().map(|&j| disp[j as usize]).sum::<Vec3>()
                        / list.len() as f32;
                    d * 0.5 + mean * 0.5
                })
                .collect();
        }
        for (q, d) in points.iter_mut().zip(&disp) {
            *q += *d;
        }
    }
    // Smoothing can drag a point back under the skin; this last unsmoothed
    // push guarantees every point ends outside. In a crease (an armpit, the
    // crotch) one push can land the point under a neighbouring face, so it
    // repeats until no point needs moving.
    for q in points.iter_mut() {
        for _ in 0..16 {
            let Some((s, n)) = surface.closest(*q) else {
                break;
            };
            if (*q - s).dot(n) >= margin * 0.999 {
                break;
            }
            *q = s + n * margin;
        }
    }
}
