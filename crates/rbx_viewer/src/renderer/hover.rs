//! The "about to click" cue: an outline around whatever `BasePart` is under
//! the cursor, distinct from — and drawn in addition to — the Explorer's own
//! selection outline (see [`super::selection::Selection`]).
//!
//! Mirrors `Selection` almost exactly, down to reusing its box-edge math (see
//! [`super::outline`]), but has no `anchor`/`centre`: the transform gizmo has
//! no business following whatever the cursor happens to be sitting over. It
//! tracks a list of parts, not one: hovering a `Model` outlines every part it
//! covers, the way a plain click selects the whole model.

use std::collections::HashMap;

use rbx_dom::Ref;
use wgpu::util::DeviceExt;

use crate::pick::Selected;
use crate::scene::Placement;

use super::outline::{box_edges, Vertex};
use super::pipeline::{self, Surface, Target};

const SHADER: &str = include_str!("hover.wgsl");

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
    /// What [`Hover::set`] last outlined — the instance(s) the hover covers,
    /// each drawn as one box (a model's aggregate bounds, a part's own), so a
    /// placement that moves under the cursor (see [`Hover::place`]) can redraw
    /// it.
    selected: Vec<Selected>,
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
                // Screen-space quads, like the selection outline (see
                // `hover.wgsl`), not a one-pixel `LineList`.
                topology: wgpu::PrimitiveTopology::TriangleList,
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
            selected: Vec::new(),
            vertices: None,
            count: 0,
        }
    }

    /// Rebuilds the outline around `selected`, replacing whatever was hovered
    /// before. An empty list clears it.
    pub(super) fn set(&mut self, device: &wgpu::Device, selected: Vec<Selected>) {
        self.selected = selected;
        let vertices = box_edges(&self.placements, &self.selected);
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
        if self.covers(referent) {
            self.set(device, self.selected.clone());
        }
    }

    /// Whether one part is inside anything the hover currently outlines — a
    /// hovered part itself, or one beneath a hovered model.
    fn covers(&self, referent: Ref) -> bool {
        self.selected
            .iter()
            .any(|entry| entry.parts().contains(&referent))
    }

    /// Forgets where one part was drawn — it stopped drawing as a box (a mesh
    /// took over) or is gone — and clears the hover outline if it was the one
    /// hovered: a box drawn around a part that no longer exists would
    /// otherwise linger until the cursor moves onto something else.
    pub(super) fn remove(&mut self, device: &wgpu::Device, referent: Ref) {
        // Only the placement is forgotten; the hovered entries stand, so the
        // box simply drops that part until it draws as a box again. A mesh
        // part keeps its placement now (see `renderer::patch`), so this is the
        // genuine gone/off-Workspace case rather than a mesh finishing loading.
        if self.placements.remove(&referent).is_some() && self.covers(referent) {
            self.set(device, self.selected.clone());
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

    /// The rest of the geometry (how a box's edges follow its model matrix,
    /// how a referent with no placement draws nothing) is already covered by
    /// `renderer::outline`'s own tests — this only checks that a hover of one
    /// part draws its box and a hover of two draws both.
    #[test]
    fn a_hovered_part_draws_its_box_and_a_model_draws_all_of_them() {
        let mut placements = HashMap::new();
        placements.insert(Ref::new(1), placement(Mat4::IDENTITY));
        placements.insert(Ref::new(2), placement(Mat4::IDENTITY));

        assert_eq!(
            box_edges(&placements, &[Selected::part(Ref::new(1))]).len(),
            72
        );
        assert_eq!(
            box_edges(
                &placements,
                &[Selected::part(Ref::new(1)), Selected::part(Ref::new(2))]
            )
            .len(),
            144
        );
        assert!(box_edges(&placements, &[]).is_empty());
    }
}
