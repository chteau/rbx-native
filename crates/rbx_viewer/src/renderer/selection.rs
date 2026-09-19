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

// Drawn with the depth test off (`compare: Always`) by default: a selection
// box reads as a control the user is acting on, not scenery, so it is never
// occluded by the geometry it wraps — the whole box shows through, which is
// what Studio does and what makes a part selected behind another object still
// visible. Drawn last of the scene geometry (see `renderer::pass`), so drawing
// on top writes no depth anything later reads except the draggers, which have
// no depth test of their own either.
//
// [`Selection::set_occluded`] asks for the other behaviour: an ordinary depth
// test, so a part standing in front of the box hides that much of it. Off by
// default, because the box that shows through is the one Studio draws;
// `rbxstudio` carries the choice as a persisted setting.

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
        std::mem::take(&mut self.stale)
            .then(|| outline::box_edges(&self.placements, &self.selected))
    }
}

/// The selection outline's GPU state: a `LineList` pipeline sharing the
/// renderer's own camera bind group, and the tiny vertex buffer rebuilt from
/// [`Outline`] whenever that has something new to say.
pub(super) struct Selection {
    /// The box drawn over everything, and the same box depth-tested against
    /// the scene. Both built up front rather than one rebuilt whenever the
    /// setting flips: they differ in a single depth-compare, and the flip
    /// arrives as a menu click with no device in hand to build the other from.
    on_top: wgpu::RenderPipeline,
    occluded_by_scene: wgpu::RenderPipeline,
    /// Whether geometry standing in front of the selection hides its box.
    occluded: bool,
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
        let with_compare = |compare| {
            pipeline::surface(
                device,
                target,
                &Surface {
                    cull: None,
                    compare,
                    // Screen-space quads, two triangles an edge, not a
                    // `LineList`: the outline is expanded to a real pixel
                    // width in the vertex shader (see `selection.wgsl`).
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Surface::new(
                        "rbxview selection",
                        SHADER,
                        &[Some(frame_layout)],
                        &[Some(Vertex::layout())],
                    )
                },
            )
        };

        Selection {
            on_top: with_compare(wgpu::CompareFunction::Always),
            // Reversed-Z (see `camera::Camera::projection`) makes `Greater`
            // the ordinary test; `GreaterEqual` takes the tie as well, so an
            // edge drawn exactly on the surface of the part it wraps is not
            // swallowed by it — the same compare `renderer::hover` picks.
            occluded_by_scene: with_compare(wgpu::CompareFunction::GreaterEqual),
            occluded: false,
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

    /// Whether geometry in front of the selection hides its box — see this
    /// module's own note above. Nothing is re-uploaded and nothing is rebuilt:
    /// only which of the two pipelines [`Selection::draw`] binds changes.
    pub(super) fn set_occluded(&mut self, occluded: bool) {
        self.occluded = occluded;
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

    /// The box the Scale handles stand on — see `gizmo::scale_box`.
    pub(super) fn scale_box(&self) -> Option<Mat4> {
        gizmo::scale_box(
            self.outline
                .selected
                .iter()
                .flat_map(|entry| outline::models_of(&self.outline.placements, entry)),
        )
    }

    /// The centre of the world-axis-aligned box containing every part the
    /// selection covers — where one gizmo for a whole selection belongs. For a
    /// single part this is simply that part's own centre; for a `Model` it is
    /// the middle of the very box `outline::box_of` outlines it with.
    ///
    /// `rbxstudio` places the handles it hit-tests from the very same
    /// `gizmo::centre_of` (see `transform::Targets::centre`), so what the user
    /// can grab and what they can see cannot drift apart.
    pub(super) fn centre(&self) -> Option<Vec3> {
        gizmo::centre_of(
            self.outline
                .selected
                .iter()
                .flat_map(|entry| outline::models_of(&self.outline.placements, entry)),
        )
    }

    /// Draws the outline, if any, reusing whichever camera bind group the rest
    /// of the scene pass just bound at group 0.
    pub(super) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, frame: &'a wgpu::BindGroup) {
        let Some(vertices) = &self.vertices else {
            return;
        };

        pass.set_pipeline(if self.occluded {
            &self.occluded_by_scene
        } else {
            &self.on_top
        });
        pass.set_bind_group(0, frame, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..self.count, 0..1);
    }
}

#[cfg(test)]
#[path = "selection/tests.rs"]
mod tests;
