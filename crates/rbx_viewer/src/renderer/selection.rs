//! The Explorer's selection, drawn as a thin outline around what it covers:
//! a selected part's own oriented bounding box, or — for a `Model`, a
//! `Folder`, or any other container with no placement of its own — one
//! world-axis-aligned box around every part beneath it.
//!
//! [`Scene::all_placements`] is keyed by `BasePart` referent and never has an
//! entry for a container, so the parts each selected instance stands for are
//! resolved against the DOM by the *editor* and arrive here already worked
//! out, as [`Selected`] — see `crate::pick::selection` for why both sides
//! resolve them through one function. A container holding no drawable
//! geometry at all still outlines nothing: there is genuinely nothing to
//! draw a box around.
//!
//! The box-edge math itself lives in [`super::outline`], shared with
//! [`super::hover::Hover`]: only the GPU state below (which referents are
//! tracked, the pipeline's colour) is specific to the selection.

use std::collections::{HashMap, HashSet};

use glam::{Mat4, Vec3};
use rbx_dom::Ref;
use wgpu::util::DeviceExt;

use crate::gizmo;
use crate::pick::Selected;
use crate::scene::Placement;

use super::outline::{self, Vertex};
use super::pipeline::{self, Surface, Target};

const SHADER: &str = include_str!("selection.wgsl");

// wgpu rejects a depth bias on anything but triangle topology, so unlike the
// decal pass this outline cannot nudge itself toward the camera. It does not
// need to: an edge sits exactly on the same geometry the box's own opaque pass
// already wrote, so `GreaterEqual` below wins every on-surface tie outright,
// while a genuinely far edge is behind a nearer (bigger, reversed-Z) depth
// already in the buffer and loses to it exactly as it should.

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
        .flat_map(outline::edges)
        .collect()
}

/// Where the transform gizmo takes its frame of reference: the first part the
/// selection covers that actually has a placement — a container's own first
/// descendant part, so Scale and Rotate act on real geometry rather than on a
/// `Model` that has no `Size` or `CFrame` to write. A referent with nothing
/// drawn under it is skipped rather than silently hiding the gizmo. `None`
/// when the whole selection covers nothing drawn at all.
///
/// The whole matrix rather than a centre and a rotation: a part's model matrix
/// folds its `Size` into the same columns its rotation lives in, and the Scale
/// tool's handles need those lengths to find the part's own faces (the gizmo
/// normalizes them where it wants directions instead — see `gizmo::basis`).
///
/// `rbxstudio` picks the same part the same way (`transform::Targets::read`
/// flattens the very `pick::selection` entries these are, dedup and order
/// alike), so the handles it hit-tests stand where these are drawn.
fn anchor_of(placements: &HashMap<Ref, Placement>, selected: &[Selected]) -> Option<Mat4> {
    Some(
        selected
            .iter()
            .flat_map(|entry| entry.parts())
            .find_map(|referent| placements.get(referent))?
            .model,
    )
}

/// Everything the outline is worked out from, with no GPU in it: what is
/// selected, where every part stands, and whether the vertices last uploaded
/// still match.
///
/// Apart from [`Selection`] because a group drag leans on the bookkeeping
/// here — one aggregate rebuild per frame, however many of a model's parts
/// moved in it — and that is worth being able to test without a device.
#[derive(Default)]
struct Outline {
    /// Every part's placement — including one whose box a resolved mesh
    /// replaced, which is still selectable and still has the box Studio
    /// outlines (see [`Scene::all_placements`]) — read once from the scene at
    /// construction and kept in step by [`Outline::place`] afterwards, so
    /// there is no reason to walk the scene again on every selection change.
    placements: HashMap<Ref, Placement>,
    /// What [`Selection::set`] last outlined, each entry already resolved to
    /// the parts it covers, so a placement that moves under the outline (see
    /// [`Outline::place`]) can redraw it without a DOM to walk.
    selected: Vec<Selected>,
    /// Every part the selection covers, flattened. A set rather than a walk of
    /// `selected`: [`Outline::place`] runs this test once per moved part, and
    /// re-scanning an N-part model's entry each time would make dragging it
    /// quadratic in the membership test alone.
    covered: HashSet<Ref>,
    /// Whether a placement under the outline has moved since the vertices were
    /// last worked out.
    stale: bool,
}

impl Outline {
    fn set(&mut self, selected: &[Selected]) {
        self.selected = selected.to_vec();
        self.covered = selected
            .iter()
            .flat_map(|entry| entry.parts())
            .copied()
            .collect();
        self.stale = true;
    }

    /// Records where one part is drawn now, and notes the outline as owing a
    /// rebuild when that part is one it covers.
    fn place(&mut self, referent: Ref, placement: Placement) {
        self.placements.insert(referent, placement);
        self.stale |= self.covered.contains(&referent);
    }

    /// Forgets where one part was drawn — it stopped drawing as a box (a mesh
    /// took over, so a full build would list no placement for it either) or
    /// is gone — and notes the outline as owing a rebuild if that part was one
    /// it covers: an outline around nothing is exactly what a deleted part
    /// leaves behind.
    fn remove(&mut self, referent: Ref) {
        if self.placements.remove(&referent).is_some() {
            self.stale |= self.covered.contains(&referent);
        }
    }

    /// The outline's vertices when something has moved under it since they
    /// were last taken, and `None` when nothing has.
    ///
    /// Taken once per frame rather than once per placement: a drag of a model
    /// moves each of its parts in turn, and rebuilding the whole aggregate box
    /// for every one of them is quadratic in the number of parts, where one
    /// rebuild at the end of the step is linear.
    fn take_vertices(&mut self) -> Option<Vec<Vertex>> {
        std::mem::take(&mut self.stale).then(|| vertices_for(&self.placements, &self.selected))
    }
}

/// The selection outline's GPU state: a `LineList` pipeline sharing the
/// renderer's own camera bind group, and the tiny vertex buffer rebuilt from
/// [`Outline`] whenever that has something new to say.
pub(super) struct Selection {
    pipeline: wgpu::RenderPipeline,
    outline: Outline,
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
                // Screen-space quads, two triangles an edge, not a `LineList`:
                // the outline is expanded to a real pixel width in the vertex
                // shader (see `selection.wgsl`).
                topology: wgpu::PrimitiveTopology::TriangleList,
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
            outline: Outline {
                placements,
                ..Outline::default()
            },
            vertices: None,
            count: 0,
        }
    }

    /// Replaces every placement with a rebuilt scene's, keeping the pipeline
    /// and the selection itself — redrawn straight away around wherever its
    /// parts stand in the new scene, or around nothing if they are gone.
    pub(super) fn rebuild(&mut self, device: &wgpu::Device, placements: HashMap<Ref, Placement>) {
        self.outline.placements = placements;
        let selected = std::mem::take(&mut self.outline.selected);
        self.set(device, &selected);
    }

    /// Rebuilds the outline around whatever `selected` names now, replacing
    /// whatever the previous selection drew.
    pub(super) fn set(&mut self, device: &wgpu::Device, selected: &[Selected]) {
        self.outline.set(selected);
        self.flush(device);
    }

    /// Records where one part is drawn now — a Properties-panel edit moved,
    /// resized or reshaped it, or a drag is walking a whole model's parts
    /// through their new placements one at a time.
    ///
    /// Bookkeeping only: the box itself is rebuilt by [`Selection::flush`]
    /// before the next frame, which is what keeps a drag of an N-part model
    /// from paying for N aggregate rebuilds inside a single step of it.
    pub(super) fn place(&mut self, referent: Ref, placement: Placement) {
        self.outline.place(referent, placement);
    }

    /// Uploads the outline again if anything has moved under it since the last
    /// frame — the edited instance is nearly always the selected one, and
    /// during a drag of a whole `Model` it is one of its descendants rather
    /// than the selected instance itself, which is exactly the case that would
    /// otherwise leave the box behind where the model used to stand.
    pub(super) fn flush(&mut self, device: &wgpu::Device) {
        let Some(vertices) = self.outline.take_vertices() else {
            return;
        };
        self.count = vertices.len() as u32;
        self.vertices = (!vertices.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview selection"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
    }

    /// Forgets where one part was drawn — it stopped drawing as a box (a
    /// mesh took over, so a full build would list no placement for it
    /// either) or is gone — and notes the outline as owing a rebuild if that
    /// part was one it covers, the same deferred way [`Selection::place`]
    /// does: an outline around nothing is exactly what a deleted part
    /// leaves behind.
    pub(super) fn remove(&mut self, referent: Ref) {
        self.outline.remove(referent);
    }

    /// The first outlined part's placement (see `renderer::gizmo`) — the part
    /// Scale and Rotate transform, whose faces Scale's balls stand on, and
    /// whose frame the local-orientation toggle takes. `None` when the
    /// selection covers nothing drawn.
    ///
    /// Where the Move gizmo is *drawn* is [`Selection::centre`] instead: it
    /// drags the whole selection as a group, so it belongs at the middle of
    /// it rather than hanging off whichever part happens to be first.
    pub(super) fn anchor(&self) -> Option<Mat4> {
        anchor_of(&self.outline.placements, &self.outline.selected)
    }

    /// The centre of the world-axis-aligned box containing every part the
    /// selection covers — where one gizmo for a whole selection belongs. For a
    /// single part this is simply that part's own centre; for a `Model` it is
    /// the middle of the very box [`box_of`] outlines it with.
    ///
    /// `rbxstudio` places the handles it hit-tests from the very same
    /// `gizmo::centre_of` (see `transform::Targets::centre`), so what the user
    /// can grab and what they can see cannot drift apart.
    /// The box the Scale handles stand on — see `gizmo::scale_box`.
    pub(super) fn scale_box(&self) -> Option<Mat4> {
        gizmo::scale_box(
            self.outline
                .selected
                .iter()
                .flat_map(|entry| models_of(&self.outline.placements, entry)),
        )
    }

    pub(super) fn centre(&self) -> Option<Vec3> {
        gizmo::centre_of(
            self.outline
                .selected
                .iter()
                .flat_map(|entry| models_of(&self.outline.placements, entry)),
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
