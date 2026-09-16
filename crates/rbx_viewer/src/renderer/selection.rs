//! The Explorer's selection, drawn as a thin outline around what it covers:
//! a selected part's own oriented bounding box, or — for a `Model`, a
//! `Folder`, or any other container with no placement of its own — one
//! world-axis-aligned box around every part beneath it.
//!
//! [`Scene::all_placements`] is keyed by `BasePart` referent and never has an
//! entry for a container, so the parts each selected instance stands for are
//! resolved against the DOM by the *editor* and arrive here already worked
//! out, as [`Selected`] — see `crate::pick::parts_of` for why both sides
//! resolve them through one function. A container holding no drawable
//! geometry at all still outlines nothing: there is genuinely nothing to
//! draw a box around.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};
use rbx_dom::Ref;
use wgpu::util::DeviceExt;

use crate::gizmo;
use crate::pick::Selected;
use crate::scene::Placement;

use super::pipeline::{self, Surface, Target};

const SHADER: &str = include_str!("selection.wgsl");

/// Half the unit cube's side, matching `renderer::mesh`'s own box extent: a
/// part's model matrix already folds its `Size` into the scale, so the same
/// [-0.5, 0.5] corners it instances land exactly on the part's surface.
const HALF: f32 = 0.5;

// wgpu rejects a depth bias on anything but triangle topology, so unlike the
// decal pass this outline cannot nudge itself toward the camera. It does not
// need to: an edge sits exactly on the same geometry the box's own opaque pass
// already wrote, so `GreaterEqual` below wins every on-surface tie outright,
// while a genuinely far edge is behind a nearer (bigger, reversed-Z) depth
// already in the buffer and loses to it exactly as it should.

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
struct Vertex {
    position: [f32; 3],
}

impl Vertex {
    const fn layout() -> wgpu::VertexBufferLayout<'static> {
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
fn edges(model: Mat4) -> [Vertex; 24] {
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

/// Every model matrix one selected instance covers, in the order
/// `crate::pick::parts_of` resolved them — a part the scene never built (one
/// outside `Workspace`, or a suppressed `MeshPart`) drops out here.
fn models_of<'a>(
    placements: &'a HashMap<Ref, Placement>,
    entry: &'a Selected,
) -> impl Iterator<Item = Mat4> + 'a {
    entry
        .parts()
        .iter()
        .filter_map(|referent| placements.get(referent))
        .map(|placement| placement.model)
}

/// The single box drawn around one selected instance, or `None` when it
/// covers no drawn geometry at all.
///
/// A part keeps its own oriented box, which hugs it however it is turned. A
/// container has no orientation to hug it with, so it gets the world-axis
/// -aligned box around everything beneath it — one box for the whole thing,
/// not one per part, because that extent is what Studio calls a model's
/// bounding box and what the Move gizmo already stands in the middle of (see
/// [`gizmo::bounds_of`], which [`Selection::centre`] takes its answer from
/// too).
fn box_of(placements: &HashMap<Ref, Placement>, entry: &Selected) -> Option<Mat4> {
    if entry.is_part() {
        return Some(placements.get(&entry.referent())?.model);
    }
    let (min, max) = gizmo::bounds_of(models_of(placements, entry))?;
    // The unit cube `edges` carries through this spans [-0.5, 0.5], so the
    // box's full extent is its scale, exactly as a part's `Size` is.
    Some(Mat4::from_translation((min + max) * 0.5) * Mat4::from_scale(max - min))
}

/// Every selected instance's edges, in selection order — one box each,
/// and nothing at all for a container with no drawable geometry under it.
fn vertices_for(placements: &HashMap<Ref, Placement>, selected: &[Selected]) -> Vec<Vertex> {
    selected
        .iter()
        .filter_map(|entry| box_of(placements, entry))
        .flat_map(edges)
        .collect()
}

/// Where the transform gizmo takes its frame of reference: the first part the
/// selection covers that actually has a placement — a container's own first
/// descendant part, so Scale and Rotate act on real geometry rather than on a
/// `Model` that has no `Size` or `CFrame` to write. A referent with nothing
/// drawn under it is skipped rather than silently hiding the gizmo. `None`
/// when the whole selection covers nothing drawn at all.
///
/// `rbxstudio` picks the same part the same way (`transform::Targets::read`
/// flattens a selection through the very `pick::parts_of` that built these
/// entries), so the handles it hit-tests stand where these are drawn.
fn anchor_of(placements: &HashMap<Ref, Placement>, selected: &[Selected]) -> Option<(Vec3, Mat3)> {
    let model = selected
        .iter()
        .flat_map(|entry| entry.parts())
        .find_map(|referent| placements.get(referent))?
        .model;
    // A part's model matrix folds its `Size` into the same columns its
    // rotation lives in, so the basis vectors come out scaled; the gizmo
    // normalizes them (see `gizmo::basis`).
    Some((model.w_axis.truncate(), Mat3::from_mat4(model)))
}

/// The selection outline's GPU state: a `LineList` pipeline sharing the
/// renderer's own camera bind group, and the tiny vertex buffer rebuilt each
/// time the selection changes.
pub(super) struct Selection {
    pipeline: wgpu::RenderPipeline,
    /// Every part's placement — including one whose box a resolved mesh
    /// replaced, which is still selectable and still has the box Studio
    /// outlines (see [`Scene::all_placements`]) — read once from the scene at
    /// construction and kept in step by [`Selection::place`] afterwards, so
    /// there is no reason to walk the scene again on every selection change.
    placements: HashMap<Ref, Placement>,
    /// What [`Selection::set`] last outlined, each entry already resolved to
    /// the parts it covers, so a placement that moves under the outline (see
    /// [`Selection::place`]) can redraw it without a DOM to walk.
    selected: Vec<Selected>,
    vertices: Option<wgpu::Buffer>,
    count: u32,
}

impl Selection {
    pub(super) fn new(
        device: &wgpu::Device,
        target: Target,
        frame_layout: &wgpu::BindGroupLayout,
        placements: HashMap<Ref, Placement>,
    ) -> Self {
        let pipeline = pipeline::surface(
            device,
            target,
            &Surface {
                cull: None,
                compare: wgpu::CompareFunction::GreaterEqual,
                topology: wgpu::PrimitiveTopology::LineList,
                ..Surface::new(
                    "rbxview selection",
                    SHADER,
                    &[Some(frame_layout)],
                    &[Some(Vertex::layout())],
                )
            },
        );

        Selection {
            pipeline,
            placements,
            selected: Vec::new(),
            vertices: None,
            count: 0,
        }
    }

    /// Rebuilds the outline around whatever `selected` names now, replacing
    /// whatever the previous selection drew.
    pub(super) fn set(&mut self, device: &wgpu::Device, selected: &[Selected]) {
        self.selected = selected.to_vec();
        let vertices = vertices_for(&self.placements, selected);
        self.count = vertices.len() as u32;
        self.vertices = (!vertices.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview selection"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
    }

    /// Records where one part is drawn now — a Properties-panel edit moved,
    /// resized or reshaped it — and redraws the outline if that part is one
    /// the selection covers: the edited instance is nearly always the
    /// selected one, and during a drag of a whole `Model` it is one of its
    /// descendants rather than the selected instance itself, which is exactly
    /// the case that would otherwise leave the box behind where the model
    /// used to stand.
    pub(super) fn place(&mut self, device: &wgpu::Device, referent: Ref, placement: Placement) {
        self.placements.insert(referent, placement);
        if self
            .selected
            .iter()
            .any(|entry| entry.parts().contains(&referent))
        {
            let selected = std::mem::take(&mut self.selected);
            self.set(device, &selected);
        }
    }

    /// The first outlined part's centre and the rotation its own local axes
    /// point along (see `renderer::gizmo`) — the part Scale and Rotate
    /// transform, and whose frame the local-orientation toggle takes. `None`
    /// when the selection covers nothing drawn.
    ///
    /// Where the Move gizmo is *drawn* is [`Selection::centre`] instead: it
    /// drags the whole selection as a group, so it belongs at the middle of
    /// it rather than hanging off whichever part happens to be first.
    pub(super) fn anchor(&self) -> Option<(Vec3, Mat3)> {
        anchor_of(&self.placements, &self.selected)
    }

    /// The centre of the world-axis-aligned box containing every part the
    /// selection covers — where one gizmo for a whole selection belongs. For a
    /// single part this is simply that part's own centre; for a `Model` it is
    /// the middle of the very box [`box_of`] outlines it with.
    ///
    /// `rbxstudio` places the handles it hit-tests from the very same
    /// `gizmo::centre_of` (see `transform::Targets::centre`), so what the user
    /// can grab and what they can see cannot drift apart.
    pub(super) fn centre(&self) -> Option<Vec3> {
        gizmo::centre_of(
            self.selected
                .iter()
                .flat_map(|entry| models_of(&self.placements, entry)),
        )
    }

    /// Draws the outline, if any, reusing whichever camera bind group the rest
    /// of the scene pass just bound at group 0.
    pub(super) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, frame: &'a wgpu::BindGroup) {
        let Some(vertices) = &self.vertices else {
            return;
        };

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..self.count, 0..1);
    }
}

#[cfg(test)]
#[path = "selection/tests.rs"]
mod tests;
