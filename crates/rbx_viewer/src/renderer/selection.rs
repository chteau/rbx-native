//! The Explorer's selection, drawn as a thin outline around the selected
//! part's oriented bounding box.
//!
//! A `Folder`, a service, or any other non-`BasePart` instance has no
//! placement to outline — [`Scene::placements`] never has an entry for one —
//! so selecting it simply draws nothing (a `Model`'s aggregate bounds are a
//! TODO: nothing here derives one yet).
//!
//! The box-edge math itself lives in [`super::outline`], shared with
//! [`super::hover::Hover`]: only the GPU state below (which referents are
//! tracked, the pipeline's colour) is specific to the selection.

use std::collections::HashMap;

use glam::{Mat3, Vec3};
use rbx_dom::Ref;
use wgpu::util::DeviceExt;

use crate::gizmo;
use crate::scene::Placement;

use super::outline::{vertices_for, Vertex};
use super::pipeline::{self, Surface, Target};

const SHADER: &str = include_str!("selection.wgsl");

// wgpu rejects a depth bias on anything but triangle topology, so unlike the
// decal pass this outline cannot nudge itself toward the camera. It does not
// need to: an edge sits exactly on the same geometry the box's own opaque pass
// already wrote, so `GreaterEqual` below wins every on-surface tie outright,
// while a genuinely far edge is behind a nearer (bigger, reversed-Z) depth
// already in the buffer and loses to it exactly as it should.

/// Where the transform gizmo takes its frame of reference: the first referent
/// (in selection order) that actually has a placement, so a `Model` or a
/// `Folder` selected ahead of a real part is skipped rather than silently
/// hiding the gizmo. `None` when nothing selected has a placement at all — an
/// all-`Folder` selection, or none.
fn anchor_of(placements: &HashMap<Ref, Placement>, referents: &[Ref]) -> Option<(Vec3, Mat3)> {
    let model = referents
        .iter()
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
    /// Every drawable part's placement, read once from the scene at
    /// construction and kept in step by [`Selection::place`] afterwards, so
    /// there is no reason to walk the scene again on every selection change.
    placements: HashMap<Ref, Placement>,
    /// What [`Selection::set`] last outlined, so a placement that moves
    /// under the outline (see [`Selection::place`]) can redraw it.
    referents: Vec<Ref>,
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
            referents: Vec::new(),
            vertices: None,
            count: 0,
        }
    }

    /// Rebuilds the outline around whatever `referents` names now, replacing
    /// whatever the previous selection drew.
    pub(super) fn set(&mut self, device: &wgpu::Device, referents: &[Ref]) {
        self.referents = referents.to_vec();
        let vertices = vertices_for(&self.placements, referents);
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
    /// resized or reshaped it — and redraws the outline if that part is in
    /// it: the edited instance is nearly always the selected one.
    pub(super) fn place(&mut self, device: &wgpu::Device, referent: Ref, placement: Placement) {
        self.placements.insert(referent, placement);
        if self.referents.contains(&referent) {
            let referents = std::mem::take(&mut self.referents);
            self.set(device, &referents);
        }
    }

    /// The first outlined part's centre and the rotation its own local axes
    /// point along (see `renderer::gizmo`) — the part Scale and Rotate
    /// transform, and whose frame the local-orientation toggle takes. `None`
    /// when nothing with a placement is selected.
    ///
    /// Where the Move gizmo is *drawn* is [`Selection::centre`] instead: it
    /// drags the whole selection as a group, so it belongs at the middle of
    /// it rather than hanging off whichever part happens to be first.
    pub(super) fn anchor(&self) -> Option<(Vec3, Mat3)> {
        anchor_of(&self.placements, &self.referents)
    }

    /// The centre of the world-axis-aligned box containing every outlined
    /// part — where one gizmo for a whole selection belongs. For a single
    /// part this is simply that part's own centre.
    ///
    /// `rbxstudio` places the handles it hit-tests from the very same
    /// `gizmo::centre_of` (see `transform::Targets::centre`), so what the user
    /// can grab and what they can see cannot drift apart.
    pub(super) fn centre(&self) -> Option<Vec3> {
        gizmo::centre_of(
            self.referents
                .iter()
                .filter_map(|referent| self.placements.get(referent))
                .map(|placement| placement.model),
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
mod tests {
    use glam::Mat4;

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
    fn nothing_selected_anchors_nothing() {
        let placements = HashMap::new();
        assert_eq!(anchor_of(&placements, &[]), None);
    }

    #[test]
    fn a_single_parts_anchor_is_its_own_centre_and_rotation() {
        let model = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let mut placements = HashMap::new();
        placements.insert(Ref::new(1), placement(model));

        let (origin, rotation) = anchor_of(&placements, &[Ref::new(1)]).unwrap();
        assert_eq!(origin, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(rotation, Mat3::from_mat4(model));
    }

    /// The whole point of an anchor at all: several parts selected together
    /// still get exactly one gizmo, at the first one in selection order.
    #[test]
    fn several_parts_anchor_at_the_first_one_in_selection_order() {
        let mut placements = HashMap::new();
        placements.insert(Ref::new(1), placement(Mat4::from_translation(Vec3::X)));
        placements.insert(Ref::new(2), placement(Mat4::from_translation(Vec3::Y)));
        placements.insert(Ref::new(3), placement(Mat4::from_translation(Vec3::Z)));

        let (origin, _) = anchor_of(&placements, &[Ref::new(2), Ref::new(1), Ref::new(3)]).unwrap();
        assert_eq!(origin, Vec3::Y);
    }

    /// A `Model`/`Folder` selected ahead of a real part (no placement of its
    /// own) must not hide the gizmo — the search skips it for the next
    /// referent that actually has one.
    #[test]
    fn a_referent_with_no_placement_is_skipped_rather_than_hiding_the_gizmo() {
        let mut placements = HashMap::new();
        placements.insert(Ref::new(2), placement(Mat4::from_translation(Vec3::X)));

        let (origin, _) = anchor_of(&placements, &[Ref::new(1), Ref::new(2)]).unwrap();
        assert_eq!(origin, Vec3::X);
    }

    #[test]
    fn a_selection_with_no_placement_at_all_anchors_nothing() {
        let placements = HashMap::new();
        assert_eq!(anchor_of(&placements, &[Ref::new(1), Ref::new(2)]), None);
    }
}
