//! The "about to click" cue: an outline around whatever `BasePart` is under
//! the cursor, distinct from — and drawn in addition to — the Explorer's own
//! selection outline (see [`super::selection::Selection`]).
//!
//! Mirrors `Selection` almost exactly, down to reusing its box-edge math (see
//! [`super::outline`]), but tracks at most one referent rather than a whole
//! selection, and has no `anchor`/`centre`: the transform gizmo has no
//! business following whatever the cursor happens to be sitting over.

use std::collections::HashMap;

use rbx_dom::Ref;
use wgpu::util::DeviceExt;

use crate::scene::Placement;

use super::outline::{vertices_for, Vertex};
use super::pipeline::{self, Surface, Target};

const SHADER: &str = include_str!("hover.wgsl");

/// `Hover` only ever tracks at most one referent, where `vertices_for`
/// (shared with `Selection`) takes a slice — the one hover-specific bit of
/// pure logic in this file, and worth its own test rather than folding it
/// silently into [`Hover::set`].
fn slice_of(referent: &Option<Ref>) -> &[Ref] {
    referent.as_ref().map_or(&[], std::slice::from_ref)
}

/// The hover outline's GPU state: a `LineList` pipeline of its own — same
/// depth test as the selection outline, but alpha-blended in a distinct
/// colour (see `hover.wgsl`) so the two never look like the same effect.
pub(super) struct Hover {
    pipeline: wgpu::RenderPipeline,
    /// Every drawable part's placement, read once from the scene at
    /// construction and kept in step by [`Hover::place`] afterwards — a
    /// second copy of what [`super::selection::Selection`] already tracks,
    /// traded for keeping the two outlines' GPU state independent rather than
    /// threading a shared placements map through both.
    placements: HashMap<Ref, Placement>,
    /// What [`Hover::set`] last outlined, so a placement that moves under the
    /// cursor (see [`Hover::place`]) can redraw it.
    referent: Option<Ref>,
    vertices: Option<wgpu::Buffer>,
    count: u32,
}

impl Hover {
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
                // Blended rather than replacing, unlike the selection outline:
                // a translucent line reads as a dimmer cue even where it
                // happens to sit right beside the selection's own edge.
                translucent: true,
                ..Surface::new(
                    "rbxview hover",
                    SHADER,
                    &[Some(frame_layout)],
                    &[Some(Vertex::layout())],
                )
            },
        );

        Hover {
            pipeline,
            placements,
            referent: None,
            vertices: None,
            count: 0,
        }
    }

    /// Rebuilds the outline around `referent`, replacing whatever was hovered
    /// before. `None` clears it.
    pub(super) fn set(&mut self, device: &wgpu::Device, referent: Option<Ref>) {
        self.referent = referent;
        let vertices = vertices_for(&self.placements, slice_of(&referent));
        self.count = vertices.len() as u32;
        self.vertices = (!vertices.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview hover"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
    }

    /// Records where one part is drawn now — a Properties-panel edit moved,
    /// resized or reshaped it — and redraws the outline if that part is the
    /// one currently hovered.
    pub(super) fn place(&mut self, device: &wgpu::Device, referent: Ref, placement: Placement) {
        self.placements.insert(referent, placement);
        if self.referent == Some(referent) {
            self.set(device, self.referent);
        }
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
    use super::*;
    use glam::{Mat4, Vec3};

    use crate::scene::ShapeKind;

    fn placement(model: Mat4) -> Placement {
        Placement {
            kind: ShapeKind::Box,
            model,
            size: Vec3::ONE,
        }
    }

    #[test]
    fn nothing_hovered_is_an_empty_slice() {
        assert!(slice_of(&None).is_empty());
    }

    #[test]
    fn something_hovered_is_a_one_element_slice() {
        let referent = Some(Ref::new(1));
        assert_eq!(slice_of(&referent), &[Ref::new(1)]);
    }

    /// The rest of the geometry (how a box's edges follow its model matrix,
    /// how a referent with no placement draws nothing) is already covered by
    /// `renderer::outline`'s own tests — this only checks that `slice_of`
    /// actually feeds `vertices_for` a hovered part correctly.
    #[test]
    fn a_hovered_part_draws_its_box() {
        let mut placements = HashMap::new();
        placements.insert(Ref::new(1), placement(Mat4::IDENTITY));
        let referent = Some(Ref::new(1));

        let vertices = vertices_for(&placements, slice_of(&referent));
        assert_eq!(vertices.len(), 24);
    }
}
