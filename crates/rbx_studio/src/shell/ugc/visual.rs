//! Visualizing UGC Validation, Studio's beta of that name: a failed check
//! drawn on the model where it failed, the way Studio's UGC Validation
//! window draws one when its row is clicked.
//!
//! Measured in Studio 0.748 with the beta on: a check that comes with
//! per-vertex data draws the edges of every triangle touching a flagged
//! vertex, `#7F2020` for the edges that meet one and `#404040` for the rest,
//! two pixels wide and hidden behind the model, and a `#FF4040` dot on each
//! flagged vertex, drawn over everything. (On the Traffic Cone, 1082802,
//! the red runs up the side from every dot on the open base ring.) A cage
//! check draws the whole cage that way, and so do degenerate triangles and
//! the mesh outside its cage, over the whole render mesh. Checks without
//! per-vertex data (bounds, attachments, schema, properties, HSR, textures)
//! draw nothing, and neither does hovering.
//!
//! Of the checks Studio draws, this validator has five: the open edges of
//! `Watertight` (Studio's `Measure_Mesh_Watertight` and
//! `Measure_Mesh_Manifold`), the vertices of `CageUvs` (its
//! `Measure_Cage_UV`), the corners of `DegenerateTriangles`
//! (`Measure_Degen_Triangles`), the render-mesh vertices `MeshOutsideCage`
//! finds outside the outer cage (`Measure_Mesh_Outside_OuterCage`), and the
//! outer-cage vertices `CageDistance` fails (`Measure_Cage_Mesh_Distance`).

use std::collections::{BTreeSet, HashSet};

use glam::{Mat4, Vec3};
use gpui_kit::rgb;
use rbx_dom::{Ref, WeakDom};
use rbx_ugc::{CFrame, CageKind, Finding, Geometry, Rule};
use rbx_viewer::Segment;

use super::{collect, Collected};
use crate::tokens::linear;

pub(crate) const BETA_ID: &str = "ugc-validation-visual";

const EDGE: u32 = 0x404040;
const EDGE_ERROR: u32 = 0x7f2020;
const MARKER: u32 = 0xff4040;
const LINE: f32 = 2.0;
const DOT: f32 = 4.0;

/// Where one part's or cage's vertices sit: `local` on top of `node`'s
/// `CFrame` as the place has it now, so a drawing follows the item.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Place {
    pub(crate) name: String,
    pub(crate) node: Ref,
    pub(crate) local: Mat4,
}

/// A Roblox `CFrame` (position, then rotation row by row) as a matrix.
pub(in crate::shell::ugc) fn cframe_matrix(c: &CFrame) -> Mat4 {
    Mat4::from_cols_array(&[
        c[3], c[6], c[9], 0.0, c[4], c[7], c[10], 0.0, c[5], c[8], c[11], 0.0, c[0], c[1], c[2],
        1.0,
    ])
}

/// Whether a finding of `rule` has somewhere on the model to draw.
pub(super) fn drawable(rule: Rule) -> bool {
    matches!(
        rule,
        Rule::Watertight
            | Rule::CageUvs
            | Rule::DegenerateTriangles
            | Rule::MeshOutsideCage
            | Rule::CageDistance
    )
}

/// Where `finding`'s vertices sit in the place now; none when its part is
/// gone or it was not read from the place.
pub(super) fn frame(dom: &WeakDom, collected: &Collected, finding: &Finding) -> Option<Mat4> {
    let place = collected
        .places
        .iter()
        .find(|p| p.name == finding.subject)?;
    let part = dom.get(place.node)?;
    Some(cframe_matrix(&collect::cframe_of(part.properties())) * place.local)
}

/// The lines and dots that show `finding` on the item in the place; none
/// when it has nothing to draw or its part is gone.
#[cfg(test)]
fn segments(dom: &WeakDom, collected: &Collected, finding: &Finding) -> Vec<Segment> {
    frame(dom, collected, finding).map_or_else(Vec::new, |f| drawing(collected, finding, f))
}

/// The lines and dots that show `finding` with its vertices at `to_world`.
pub(super) fn drawing(collected: &Collected, finding: &Finding, to_world: Mat4) -> Vec<Segment> {
    let item = &collected.item;
    let part = || {
        item.parts
            .iter()
            .find(|p| p.name == finding.subject)
            .and_then(|p| p.geometry.as_ref())
    };
    let welded_ids = |geometry: &Geometry, vertices: Vec<usize>| -> HashSet<u32> {
        let welded = rbx_ugc::weld(&geometry.positions);
        vertices
            .into_iter()
            .filter_map(|i| welded.get(i).copied())
            .collect()
    };
    match finding.rule {
        Rule::Watertight => {
            let Some(geometry) = part() else {
                return Vec::new();
            };
            let flagged: HashSet<u32> = rbx_ugc::open_edges(geometry)
                .into_iter()
                .flatten()
                .collect();
            draw(geometry, &flagged, false, to_world)
        }
        Rule::DegenerateTriangles => {
            let Some(geometry) = part() else {
                return Vec::new();
            };
            let corners = rbx_ugc::degenerate_triangles(geometry)
                .into_iter()
                .flat_map(|t| geometry.triangles[t].map(|i| i as usize))
                .collect();
            draw(geometry, &welded_ids(geometry, corners), true, to_world)
        }
        Rule::MeshOutsideCage => {
            let Some(geometry) = part() else {
                return Vec::new();
            };
            let outside = rbx_ugc::mesh_outside_cage(item);
            draw(geometry, &welded_ids(geometry, outside), true, to_world)
        }
        Rule::CageDistance => {
            let Some(cage) = item.cages.iter().find(|c| c.name == finding.subject) else {
                return Vec::new();
            };
            let failed = rbx_ugc::cage_distance_failures(&rbx_ugc::cage_mesh_distances(item));
            draw(
                &cage.geometry,
                &welded_ids(&cage.geometry, failed),
                true,
                to_world,
            )
        }
        Rule::CageUvs => {
            let Some(cage) = item.cages.iter().find(|c| c.name == finding.subject) else {
                return Vec::new();
            };
            let inner = item
                .cages
                .iter()
                .find(|c| c.kind == CageKind::Inner && cage.kind == CageKind::Outer)
                .map(|c| &c.geometry);
            let moved = cage
                .template
                .iter()
                .chain(inner)
                .flat_map(|base| rbx_ugc::cage_uv_mismatches(&cage.geometry, base))
                .collect();
            draw(
                &cage.geometry,
                &welded_ids(&cage.geometry, moved),
                true,
                to_world,
            )
        }
        _ => Vec::new(),
    }
}

/// Studio's drawing of flagged vertices (welded ids, see `rbx_ugc::weld`):
/// the edges of each triangle touching one, or of every triangle with
/// `whole`, and a dot on each.
fn draw(geometry: &Geometry, flagged: &HashSet<u32>, whole: bool, to_world: Mat4) -> Vec<Segment> {
    let welded = rbx_ugc::weld(&geometry.positions);
    let mut at = vec![Vec3::ZERO; welded.iter().max().map_or(0, |&m| m as usize + 1)];
    for (i, &id) in welded.iter().enumerate().rev() {
        at[id as usize] = to_world.transform_point3(Vec3::from(geometry.positions[i]));
    }
    let mut edges = BTreeSet::new();
    for tri in rbx_ugc::welded_triangles(geometry, &welded) {
        if whole || tri.iter().any(|id| flagged.contains(id)) {
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                edges.insert((a.min(b), a.max(b)));
            }
        }
    }
    let (edge, error, marker) = (
        linear(rgb(EDGE)),
        linear(rgb(EDGE_ERROR)),
        linear(rgb(MARKER)),
    );
    let mut out: Vec<Segment> = edges
        .into_iter()
        .map(|(a, b)| Segment {
            from: at[a as usize],
            to: at[b as usize],
            color: if flagged.contains(&a) || flagged.contains(&b) {
                error
            } else {
                edge
            },
            on_top: false,
            width: LINE,
        })
        .collect();
    let mut dots: Vec<u32> = flagged.iter().copied().collect();
    dots.sort_unstable();
    out.extend(dots.into_iter().filter_map(|id| {
        let p = *at.get(id as usize)?;
        Some(Segment {
            from: p,
            to: p,
            color: marker,
            on_top: true,
            width: DOT,
        })
    }));
    out
}

#[cfg(test)]
#[path = "visual/tests.rs"]
mod tests;
