//! Boolean geometry for legacy unions: evaluates a union's operation tree
//! with the shared [`rbx_csg`] kernel. Entry point: [`evaluate`].

#[cfg(test)]
mod diagnostics;

use glam::Vec3;
#[cfg(test)]
use rbx_csg::{bsp, repair};
pub(super) use rbx_csg::{Failure, Solid};
use rbx_dom::Variant;

use super::tree::Node;
use crate::scene::ShapeKind;
use crate::shapes::{self, MeshData};
#[cfg(test)]
pub(super) use diagnostics::{connected_components, leak_report};
#[cfg(test)]
use repair::{clean, is_watertight, weld_t_junctions};

/// Same idea as the kernel's polygon cap for the operation tree itself:
/// nobody hand-builds a union this wide, so anything past it is a malformed
/// asset rather than a rock.
const MAX_LEAVES: usize = 512;

/// The unit mesh a leaf's shape draws as, the same generators the renderer
/// instances (`renderer::geometry`), so a carved sphere matches a drawn one.
pub(crate) fn unit_mesh(kind: ShapeKind) -> MeshData {
    match kind {
        ShapeKind::Box => shapes::block(),
        ShapeKind::Ball => shapes::sphere(),
        ShapeKind::CylinderX => shapes::cylinder_x(),
        ShapeKind::CylinderY => shapes::cylinder_y(),
        ShapeKind::Wedge => shapes::wedge(),
        ShapeKind::CornerWedge => shapes::corner_wedge(),
        ShapeKind::Truss {
            axis,
            segments,
            style,
        } => shapes::oriented(shapes::truss(segments, style), axis),
    }
}

/// Evaluates one operation tree in the union's own frame: at every node the
/// additive children are unioned, then each negated child is carved out.
///
/// Leaf meshes are generated once per shape kind and reused across the tree.
/// `bake` is the union's `InitialSize`, the extent Studio's own bake of this
/// tree has: a disconnected piece outside it is debris (see
/// `repair::discard_disconnected_debris`). `None` keeps every piece.
pub(super) fn evaluate(root: &Node, bake: Option<Vec3>) -> Result<Solid, Failure> {
    if root.leaf_count() > MAX_LEAVES {
        return Err(Failure::TooComplex);
    }
    let mut cache = Vec::new();
    let solid = evaluate_node(root, &mut cache)?.finish(bake.map(Vec3::as_dvec3))?;
    let volume = solid.volume();
    // A union can only ever shrink from subtracting; it can never exceed the
    // sum of its additive leaves' own volumes (inclusion-exclusion), so that
    // sum is a cheap ceiling without a second full boolean.
    let ceiling = additive_volume_bound(root);
    if volume <= 0.0 || volume > ceiling * (1.0 + VOLUME_SLACK) + EPSILON_VOLUME {
        return Err(Failure::Inverted);
    }
    Ok(solid)
}

/// Small enough that no legitimate result's own floating-point volume noise
/// trips the [`Failure::Inverted`] ceiling check above.
const EPSILON_VOLUME: f64 = 1e-6;
/// Relative to the ceiling: welding moves vertices by up to a weld cell, so a
/// result that only touches (none of whose leaves overlap) can land a few
/// parts in ten million over the exact sum on a large union of many parts.
const VOLUME_SLACK: f64 = 1e-4;

/// See [`evaluate`]'s ceiling check: the volume every additive leaf would
/// have on its own, ignoring negation and overlap.
fn additive_volume_bound(node: &Node) -> f64 {
    match node {
        Node::Leaf(leaf) if !leaf.negate => {
            // The whole placement, not just the leaf's size: a nested
            // operation resized since its bake scales the leaves under it.
            let mesh = unit_mesh(leaf.geometry.kind);
            Solid::from_mesh(&mesh, leaf.model()).volume().abs()
        }
        Node::Leaf(_) => 0.0,
        Node::Operation { children, .. } => children
            .iter()
            .filter(|child| !child.is_negate())
            .map(additive_volume_bound)
            .sum(),
    }
}

fn evaluate_node(node: &Node, cache: &mut Vec<(ShapeKind, MeshData)>) -> Result<Solid, Failure> {
    match node {
        Node::Leaf(leaf) => {
            let kind = leaf.geometry.kind;
            let slot = match cache.iter().position(|(known, _)| *known == kind) {
                Some(slot) => slot,
                None => {
                    cache.push((kind, unit_mesh(kind)));
                    cache.len() - 1
                }
            };
            // Every leaf, negated or not, paints the faces it leaves behind:
            // a hole's walls show the negated part's colour in Studio.
            let color = match leaf.properties.get("Color3uint8") {
                Some(&Variant::Color3uint8 { r, g, b }) => [r, g, b],
                _ => super::super::FALLBACK_COLOR,
            };
            Ok(Solid::from_mesh(&cache[slot].1, leaf.model()).painted(color))
        }
        Node::Operation { children, .. } => {
            let mut positive: Option<Solid> = None;
            let mut negatives = Vec::new();
            for child in children {
                let solid = evaluate_node(child, cache)?;
                if solid.is_empty() {
                    continue;
                }
                if child.is_negate() {
                    negatives.push(solid);
                } else {
                    positive = Some(match positive {
                        Some(so_far) => so_far.union(solid)?,
                        None => solid,
                    });
                }
            }
            let Some(mut result) = positive else {
                return Ok(Solid::default());
            };
            for negative in negatives {
                result = result.subtract(negative)?;
            }
            Ok(result)
        }
    }
}

#[cfg(test)]
#[path = "csg/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "csg/noise_tests.rs"]
mod noise_tests;
