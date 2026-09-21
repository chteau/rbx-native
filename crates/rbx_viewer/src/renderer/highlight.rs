//! `Highlight`: the silhouette outline and interior fill a script or a place
//! file asks for by parenting the instance to what it wants called attention
//! to.
//!
//! Two passes. The first re-draws the highlighted geometry, position only,
//! into a one-byte-per-pixel *mask* holding which highlight owns each pixel —
//! instanced against the same unit shapes and file-mesh buffers the colour
//! pass uses, so the silhouette is the object's own and not the box around
//! it. The second reads that mask back and paints: the interior wherever a
//! highlight owns the pixel, the outline wherever neighbouring pixels
//! disagree about who does.
//!
//! Deliberately not built on [`super::outline`], which the Explorer's
//! selection and hover cues share: that one traces the twelve edges of a
//! bounding box, which is the right shape for "what you have selected" and
//! the wrong one for an effect Roblox documents as a *silhouette* outline.
//!
//! Which parts each highlight covers is resolved off the DOM in
//! `scene::highlight`; nothing here reads an `Adornee`.

mod pipelines;

use std::collections::HashMap;

use rbx_assets::AssetRef;
use rbx_dom::Ref;

use super::cull::visible_runs;
use super::geometry::Meshes;
use super::pipeline::Target;
use super::post::Targets;
use super::shadow::casters::{self, MeshGeometry};
use super::slots::keyed::Keyed;
use super::slots::Roster;
use crate::scene::{
    DepthMode, Highlight, Part, PartId, Resolved, ResolvedInstance, ShapeKind, MAX_HIGHLIGHTS,
};

use pipelines::{Composite, Mask, MaskInstance, Paint};

/// The highlighted unit shapes, drawn against [`Meshes`]' shared buffers.
/// Keyed like `shaped::Shaped`'s own batches and for the same reason, but
/// filtered by "a highlight covers it" rather than by "it is drawn opaque":
/// the same part lands at a different slot in each.
///
/// The side data is the instance's depth mode, so one pass over a batch can
/// pick out the run of instances the pipeline currently bound is for.
type ShapeBatches = Keyed<ShapeKind, (), MaskInstance, DepthMode, PartId>;

/// The highlighted file meshes, one batch per mesh asset, against
/// position-only geometry of their own — [`casters::MeshGeometry`], which the
/// shadow pass builds from the same meshes for the same reason.
type MeshBatches = Keyed<AssetRef, MeshGeometry, MaskInstance, DepthMode>;

/// What a part needs to know about the highlight covering it: which one, as
/// the 1-based index the mask carries, and which side of the scene's depth it
/// is allowed to show on.
type Claim = (u32, DepthMode);

/// Every `Highlight` in the scene, as the two passes that draw them need it.
pub(super) struct Highlights {
    /// The highlight covering each referent. Empty in a place with no
    /// highlights at all, which is what makes the whole pass free there.
    claims: HashMap<Ref, Claim>,
    shapes: ShapeBatches,
    meshes: MeshBatches,
    /// One [`Paint`] per highlight, at the index the mask names.
    paints: wgpu::Buffer,
    /// The six pipelines, built the first time a scene actually holds a
    /// highlight and never before: most places hold none, and this repo
    /// already counts what opening one compiles (see
    /// `renderer::switch::Renderer::rebuild_pipelines`).
    gpu: Option<Gpu>,
    /// What the pipelines were built for, so [`Self::rebuild`] can build them
    /// later without being handed the target again.
    format: Target,
    /// The mask texture and the bind group reading it, reallocated whenever
    /// the frame changes size. `None` until the first frame draws one.
    target: Option<MaskTarget>,
    /// Which depth modes are in play, so a scene using one never pays for a
    /// second walk of the batches in the other.
    modes: Vec<DepthMode>,
}

struct Gpu {
    mask: Mask,
    composite: Composite,
}

struct MaskTarget {
    size: (u32, u32),
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

impl Highlights {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::BindGroupLayout,
        target: Target,
        scene: Source<'_>,
    ) -> Self {
        let mut pass = Highlights {
            claims: HashMap::new(),
            shapes: Keyed::new("rbxview highlight shapes"),
            meshes: Keyed::new("rbxview highlight meshes"),
            paints: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rbxview highlight paints"),
                size: pipelines::PAINTS_SIZE,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            gpu: None,
            format: target,
            target: None,
            modes: Vec::new(),
        };
        pass.rebuild(device, queue, frame, scene);
        pass
    }

    /// Follows a new sample count — see
    /// `renderer::switch::Renderer::rebuild_pipelines`. The pipelines are
    /// rebuilt only if this scene ever had any; the mask texture goes either
    /// way, since its sample count has to match the depth buffer it shares a
    /// pass with, and the next frame allocates one that does.
    pub(super) fn set_target(
        &mut self,
        device: &wgpu::Device,
        target: Target,
        frame: &wgpu::BindGroupLayout,
    ) {
        self.format = target;
        if self.gpu.is_some() {
            self.gpu = Some(Gpu::new(device, frame, target));
        }
        self.target = None;
    }

    /// Takes on a freshly re-planned highlight list (see
    /// `scene::Scene::replan_effect`). Everything is rebuilt: a highlight's
    /// index is its position in that list, so one appearing or leaving
    /// renumbers the rest, and nothing survives that worth keeping.
    pub(super) fn replace(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::BindGroupLayout,
        scene: Source<'_>,
    ) {
        self.rebuild(device, queue, frame, scene);
    }

    /// Brings the mask in line with one edited unit-shape part — the same
    /// four outcomes `shaped::Shaped::sync` has. A part no highlight covers
    /// is never in these batches to begin with, so this is a no-op for all
    /// but a handful of the edits a drag reports.
    pub(super) fn sync_part(&mut self, device: &wgpu::Device, part: &Part) {
        let wanted = self
            .claims
            .get(&part.referent())
            .filter(|_| part.is_drawn())
            .map(|&(index, mode)| {
                (
                    part.kind,
                    MaskInstance::new(part.transform.to_cols_array_2d(), index),
                    mode,
                )
            });
        self.shapes.sync(device, part.id, wanted, |_| Some(()));
    }

    pub(super) fn remove_part(&mut self, id: PartId) {
        self.shapes.remove(id);
    }

    /// Uploads what [`Self::sync_part`] and [`Self::sync_mesh`] rewrote in
    /// place — a moved part's record is only marked, like every other
    /// pass's (see `slots::Slots::flush`), so a highlight or cue left
    /// unflushed stays drawn where its part used to stand.
    pub(super) fn flush(&mut self, queue: &wgpu::Queue) {
        self.shapes.flush(queue);
        self.meshes.flush(queue);
    }

    /// [`Self::sync_part`] for a resolved file mesh. `false` when the mesh is
    /// one this renderer never downloaded, which is the caller's cue to fall
    /// back to a full reload — the same answer `casters::sync_mesh` gives.
    pub(super) fn sync_mesh(
        &mut self,
        device: &wgpu::Device,
        resolved: &Resolved,
        instance: &ResolvedInstance,
    ) -> bool {
        let wanted = self.claims.get(&instance.referent).map(|&(index, mode)| {
            (
                instance.mesh.clone(),
                MaskInstance::new(instance.model.to_cols_array_2d(), index),
                mode,
            )
        });
        self.meshes.sync(device, instance.referent, wanted, |mesh| {
            casters::geometry(device, resolved, mesh)
        })
    }

    pub(super) fn remove_mesh(&mut self, referent: Ref) {
        self.meshes.remove(referent);
    }

    /// Whether this frame has anything to draw at all. A place with no
    /// `Highlight` in it — which is most of them — allocates no mask and
    /// opens neither pass.
    pub(super) fn is_empty(&self) -> bool {
        self.claims.is_empty()
    }

    /// Both passes, in order: the mask, then the paint over the scene.
    ///
    /// Runs after every other scene pass (see `Renderer::draw`) because a
    /// highlight is documented as calling attention to an object rather than
    /// as being part of it — and before the resolve, so it is tone mapped
    /// with the rest of the frame, the way this renderer's other in-scene
    /// cues already are.
    pub(super) fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        frame: &wgpu::BindGroup,
        meshes: &Meshes,
        size: (u32, u32),
    ) {
        if self.is_empty() {
            return;
        }
        self.fit(device, size, targets.samples());
        let (Some(gpu), Some(target)) = (&self.gpu, &self.target) else {
            return;
        };

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rbxview highlight mask"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Zero is "no highlight here", so a cleared mask is an
                        // empty one — see `highlight.wgsl`.
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: targets.depth(),
                    // Loaded and stored unchanged: `Occluded` tests against
                    // what the scene pass wrote, and the passes after this one
                    // still read the same values.
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_bind_group(0, frame, &[]);
            for mode in self.modes.clone() {
                self.draw_mask(&mut pass, gpu, meshes, mode);
            }
        }

        let (view, resolve) = targets.color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rbxview highlight"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&gpu.composite.pipeline);
        pass.set_bind_group(0, &target.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// One depth mode's worth of mask draws: every batch, but only the runs
    /// of instances that mode's pipeline is for — the same
    /// [`visible_runs`] split the main and shadow passes already cull with,
    /// asked a different question.
    fn draw_mask<'p>(
        &'p self,
        pass: &mut wgpu::RenderPass<'p>,
        gpu: &'p Gpu,
        meshes: &Meshes,
        mode: DepthMode,
    ) {
        pass.set_pipeline(gpu.mask.shapes(mode));
        for batch in self.shapes.groups() {
            let Some(mesh) = meshes.get(batch.key) else {
                continue;
            };
            let runs = visible_runs(batch.slots.count(), |index| batch.slots.side(index) == mode);
            if runs.is_empty() {
                continue;
            }
            pass.set_vertex_buffer(1, batch.slots.buffer().slice(..));
            for run in runs {
                mesh.draw_range(pass, run);
            }
        }

        pass.set_pipeline(gpu.mask.meshes(mode));
        for batch in self.meshes.groups() {
            let runs = visible_runs(batch.slots.count(), |index| batch.slots.side(index) == mode);
            if runs.is_empty() {
                continue;
            }
            let geometry = &batch.extra;
            pass.set_vertex_buffer(0, geometry.vertices.slice(..));
            pass.set_vertex_buffer(1, batch.slots.buffer().slice(..));
            pass.set_index_buffer(geometry.indices.slice(..), wgpu::IndexFormat::Uint32);
            for run in runs {
                pass.draw_indexed(0..geometry.index_count, 0, run);
            }
        }
    }

    fn rebuild(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::BindGroupLayout,
        scene: Source<'_>,
    ) {
        let highlights = &scene.highlights[..scene.highlights.len().min(MAX_HIGHLIGHTS)];
        if !highlights.is_empty() && self.gpu.is_none() {
            self.gpu = Some(Gpu::new(device, frame, self.format));
        }

        self.claims = highlights
            .iter()
            .enumerate()
            .flat_map(|(position, highlight)| {
                let claim = (position as u32 + 1, highlight.depth_mode);
                highlight
                    .parts
                    .iter()
                    .map(move |&referent| (referent, claim))
            })
            .collect();
        self.modes = [DepthMode::AlwaysOnTop, DepthMode::Occluded]
            .into_iter()
            .filter(|&mode| highlights.iter().any(|one| one.depth_mode == mode))
            .collect();

        let paints: Vec<Paint> = highlights.iter().map(Paint::of).collect();
        if !paints.is_empty() {
            queue.write_buffer(&self.paints, 0, bytemuck::cast_slice(&paints));
        }

        self.shapes = shape_batches(device, &self.claims, scene.parts);
        self.meshes = mesh_batches(device, &self.claims, scene.resolved);
    }

    /// The mask target, allocated at `size` and kept until the frame changes
    /// shape — a window resize would otherwise cost one texture per frame of
    /// the drag.
    fn fit(&mut self, device: &wgpu::Device, size: (u32, u32), samples: u32) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        if self
            .target
            .as_ref()
            .is_some_and(|target| target.size == size)
        {
            return;
        }

        let view = pipelines::mask_texture(device, size, samples);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rbxview highlight composite"),
            layout: &gpu.composite.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.paints.as_entire_binding(),
                },
            ],
        });
        self.target = Some(MaskTarget {
            size,
            view,
            bind_group,
        });
    }
}

impl Gpu {
    fn new(device: &wgpu::Device, frame: &wgpu::BindGroupLayout, target: Target) -> Self {
        Gpu {
            mask: Mask::new(device, frame, target),
            composite: Composite::new(device, target),
        }
    }
}

/// The three parts of a [`Scene`](crate::scene::Scene) this pass is built
/// from. Together because they are always read from the same scene at the
/// same moment, and separately from `Scene` itself because `renderer::rebuild`
/// hands them over one field at a time.
#[derive(Clone, Copy)]
pub(super) struct Source<'a> {
    pub(super) highlights: &'a [Highlight],
    pub(super) parts: &'a [Part],
    pub(super) resolved: &'a Resolved,
}

fn shape_batches(
    device: &wgpu::Device,
    claims: &HashMap<Ref, Claim>,
    parts: &[Part],
) -> ShapeBatches {
    let mut batches = Keyed::new("rbxview highlight shapes");
    if claims.is_empty() {
        return batches;
    }
    for kind in super::shaped::kinds(parts) {
        let roster = Roster::from_iter(
            parts
                .iter()
                .filter(|part| part.kind == kind && part.is_drawn())
                .filter_map(|part| {
                    let &(index, mode) = claims.get(&part.referent())?;
                    Some((
                        part.id,
                        MaskInstance::new(part.transform.to_cols_array_2d(), index),
                        mode,
                    ))
                }),
        );
        if roster.len() > 0 {
            batches.add_group(device, kind, (), roster);
        }
    }
    batches
}

fn mesh_batches(
    device: &wgpu::Device,
    claims: &HashMap<Ref, Claim>,
    resolved: &Resolved,
) -> MeshBatches {
    let mut batches = Keyed::new("rbxview highlight meshes");
    let mut order: Vec<AssetRef> = Vec::new();
    for instance in &resolved.instances {
        if claims.contains_key(&instance.referent) && !order.contains(&instance.mesh) {
            order.push(instance.mesh.clone());
        }
    }
    for reference in order {
        let Some(geometry) = casters::geometry(device, resolved, &reference) else {
            continue;
        };
        let roster = Roster::from_iter(
            resolved
                .instances
                .iter()
                .filter(|instance| instance.mesh == reference)
                .filter_map(|instance| {
                    let &(index, mode) = claims.get(&instance.referent)?;
                    Some((
                        instance.referent,
                        MaskInstance::new(instance.model.to_cols_array_2d(), index),
                        mode,
                    ))
                }),
        );
        batches.add_group(device, reference, geometry, roster);
    }
    batches
}
