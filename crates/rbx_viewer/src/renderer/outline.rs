//! The oriented-box outline math [`super::selection::Selection`] and
//! [`super::hover::Hover`] both draw from: a `BasePart`'s unit cube, carried
//! through its model matrix into 12 edges / 24 `LineList` vertices. Factored
//! out so the two outlines — Studio-blue for the selection, a dimmer amber for
//! the hover cue — can never quietly drift apart on what "a box around a
//! part" actually means; only their GPU state (pipeline, colour, which
//! referents they track) differs.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use rbx_dom::Ref;

use crate::gizmo;
use crate::pick::Selected;
use crate::scene::Placement;

/// Half the unit cube's side, matching `renderer::mesh`'s own box extent: a
/// part's model matrix already folds its `Size` into the scale, so the same
/// [-0.5, 0.5] corners it instances land exactly on the part's surface.
const HALF: f32 = 0.5;

const CORNERS: [Vec3; 8] = [
    Vec3::new(-HALF, -HALF, -HALF),
    Vec3::new(HALF, -HALF, -HALF),
    Vec3::new(HALF, HALF, -HALF),
    Vec3::new(-HALF, HALF, -HALF),
    Vec3::new(-HALF, -HALF, HALF),
    Vec3::new(HALF, -HALF, HALF),
    Vec3::new(HALF, HALF, HALF),
    Vec3::new(-HALF, HALF, HALF),
];

/// The cube's 12 edges as corner index pairs: one ring on each end, then the
/// four edges joining them.
const EDGES: [(usize, usize); 12] = [
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 0),
    (4, 5),
    (5, 6),
    (6, 7),
    (7, 4),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

/// One corner of an edge's screen-space quad: this endpoint, the edge's other
/// endpoint (so the vertex shader can find the on-screen direction of the
/// line), and which side of the line to push out to — see `outline.wgsl`,
/// concatenated into both `selection.wgsl` and `hover.wgsl`. A `LineList`
/// would draw at one hairline pixel with no width control at all (wgpu's line
/// width is fixed at 1 and WebGPU has none); expanding each edge into two
/// triangles in screen space is what gives the outline a real, camera-
/// independent thickness, the way Studio's own selection box has.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct Vertex {
    position: [f32; 3],
    other: [f32; 3],
    side: f32,
}

const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32];

impl Vertex {
    pub(super) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBUTES,
        }
    }
}

/// The six vertices — two triangles — of one edge's screen-space quad, from
/// endpoint `a` to endpoint `b`. Each carries its own end, the far end, and a
/// side; `outline.wgsl` turns the pair into a ribbon of constant pixel width.
/// A corner on `a` names `b` as its `other` and vice versa, so the shader
/// reads the same on-screen line direction from both ends.
fn quad(a: Vec3, b: Vec3) -> [Vertex; 6] {
    let av: [f32; 3] = a.into();
    let bv: [f32; 3] = b.into();
    let al = Vertex {
        position: av,
        other: bv,
        side: 1.0,
    };
    let ar = Vertex {
        position: av,
        other: bv,
        side: -1.0,
    };
    let bl = Vertex {
        position: bv,
        other: av,
        side: -1.0,
    };
    let br = Vertex {
        position: bv,
        other: av,
        side: 1.0,
    };
    [al, ar, bl, ar, br, bl]
}

/// Every edge of the box `model` carries, as screen-space quads — 12 edges,
/// six vertices each.
pub(super) fn edges(model: Mat4) -> [Vertex; 72] {
    let corners = CORNERS.map(|corner| model.transform_point3(corner));
    let mut vertices = [Vertex {
        position: [0.0; 3],
        other: [0.0; 3],
        side: 0.0,
    }; 72];
    for (edge, (a, b)) in EDGES.iter().enumerate() {
        let quad = quad(corners[*a], corners[*b]);
        vertices[edge * 6..edge * 6 + 6].copy_from_slice(&quad);
    }
    vertices
}

/// Every model matrix one selected/hovered instance covers, in the order
/// `crate::pick::parts_of` resolved them — a part the scene never built (one
/// outside `Workspace`) drops out here.
pub(super) fn models_of<'a>(
    placements: &'a HashMap<Ref, Placement>,
    entry: &'a Selected,
) -> impl Iterator<Item = Mat4> + 'a {
    entry
        .parts()
        .iter()
        .filter_map(|referent| placements.get(referent))
        .map(|placement| placement.model)
}

/// The single box drawn around one instance, or `None` when it covers no
/// drawn geometry at all.
///
/// A part keeps its own oriented box, which hugs it however it is turned. A
/// container has no orientation to hug it with, so it gets the world-axis
/// -aligned box around everything beneath it — one box for the whole thing,
/// not one per part, because that extent is what Studio calls a model's
/// bounding box and what the Move gizmo already stands in the middle of (see
/// `gizmo::bounds_of`). Shared by the selection and hover outlines so both
/// draw a model as one box rather than a mess of per-part ones.
pub(super) fn box_of(placements: &HashMap<Ref, Placement>, entry: &Selected) -> Option<Mat4> {
    if entry.is_part() {
        return Some(placements.get(&entry.referent())?.model);
    }
    let (min, max) = gizmo::bounds_of(models_of(placements, entry))?;
    // The unit cube `edges` carries through spans [-0.5, 0.5], so the box's
    // full extent is its scale, exactly as a part's `Size` is.
    Some(Mat4::from_translation((min + max) * 0.5) * Mat4::from_scale(max - min))
}

/// Every instance's edges, in order — one box each, and nothing at all for a
/// container with no drawable geometry under it.
pub(super) fn box_edges(
    placements: &HashMap<Ref, Placement>,
    selected: &[Selected],
) -> Vec<Vertex> {
    selected
        .iter()
        .filter_map(|entry| box_of(placements, entry))
        .flat_map(edges)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::ShapeKind;

    fn placement(model: Mat4) -> Placement {
        Placement {
            kind: ShapeKind::Box,
            model,
            size: Vec3::ONE,
        }
    }

    #[test]
    fn a_box_has_twelve_edges_of_six_vertices_each() {
        let vertices = edges(Mat4::IDENTITY);
        // 12 edges, each a screen-space quad of two triangles.
        assert_eq!(vertices.len(), 72);
        assert_eq!(EDGES.len(), 12);
    }

    #[test]
    fn the_corners_follow_the_model_matrix() {
        let model = Mat4::from_translation(Vec3::new(66.0, 6.5, -81.0))
            * Mat4::from_scale(Vec3::new(10.0, 13.0, 2.0));
        let vertices = edges(model);

        // Every vertex is a cube corner carried through `model`: half the part's
        // size away from its centre on every axis.
        for vertex in vertices {
            let local = Vec3::from(vertex.position) - Vec3::new(66.0, 6.5, -81.0);
            assert!((local.x.abs() - 5.0).abs() < 1e-4);
            assert!((local.y.abs() - 6.5).abs() < 1e-4);
            assert!((local.z.abs() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn a_referent_with_no_placement_draws_nothing() {
        // Stands for a `Folder`, a service, or a `Model`: none of them are a
        // `BasePart`, so `Scene::placements` never has an entry for one.
        let placements = HashMap::new();
        let vertices = box_edges(&placements, &[Selected::part(Ref::new(1))]);
        assert!(vertices.is_empty());
    }

    #[test]
    fn a_part_referent_draws_its_box() {
        let mut placements = HashMap::new();
        placements.insert(Ref::new(1), placement(Mat4::IDENTITY));

        let vertices = box_edges(&placements, &[Selected::part(Ref::new(1))]);
        assert_eq!(vertices.len(), 72);
    }

    #[test]
    fn naming_nothing_draws_nothing() {
        let placements = HashMap::new();
        let vertices = box_edges(&placements, &[]);
        assert!(vertices.is_empty());
    }
}
