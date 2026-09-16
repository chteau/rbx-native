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

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct Vertex {
    position: [f32; 3],
}

impl Vertex {
    pub(super) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3],
        }
    }
}

/// The 12 edges (24 vertices) of one part's oriented bounding box: the unit
/// cube's corners carried through its model matrix, which already scales them
/// to the part's `Size`.
pub(super) fn edges(model: Mat4) -> [Vertex; 24] {
    let corners = CORNERS.map(|corner| model.transform_point3(corner));
    let mut vertices = [Vertex { position: [0.0; 3] }; 24];
    for (edge, (a, b)) in EDGES.iter().enumerate() {
        vertices[edge * 2] = Vertex {
            position: corners[*a].into(),
        };
        vertices[edge * 2 + 1] = Vertex {
            position: corners[*b].into(),
        };
    }
    vertices
}

/// Every named referent's edges, in order — a referent with no placement (not
/// a `BasePart`) contributes nothing, whether it names a `Folder`, a service,
/// or a `Model` with no aggregate box of its own yet.
pub(super) fn vertices_for(placements: &HashMap<Ref, Placement>, referents: &[Ref]) -> Vec<Vertex> {
    referents
        .iter()
        .filter_map(|referent| placements.get(referent))
        .flat_map(|placement| edges(placement.model))
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
    fn a_box_has_twelve_edges_and_twenty_four_vertices() {
        let vertices = edges(Mat4::IDENTITY);
        assert_eq!(vertices.len(), 24);
        // 12 edges, each contributing exactly one pair of endpoints.
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
        let vertices = vertices_for(&placements, &[Ref::new(1)]);
        assert!(vertices.is_empty());
    }

    #[test]
    fn a_part_referent_draws_its_box() {
        let mut placements = HashMap::new();
        placements.insert(Ref::new(1), placement(Mat4::IDENTITY));

        let vertices = vertices_for(&placements, &[Ref::new(1)]);
        assert_eq!(vertices.len(), 24);
    }

    #[test]
    fn naming_nothing_draws_nothing() {
        let placements = HashMap::new();
        let vertices = vertices_for(&placements, &[]);
        assert!(vertices.is_empty());
    }
}
