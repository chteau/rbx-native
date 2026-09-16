//! The second pass: `Decal` and `Texture` images projected back onto the very
//! surfaces they are pinned to, one instanced draw per (image, shape) pair.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use rbx_assets::AssetRef;
use rbx_dom::Ref;

use super::geometry::Meshes;
use super::mesh::Vertex;
use super::pipeline::{self, Surface, Target, DECAL_SHADER};
use super::slots::keyed::Keyed;
use super::slots::Roster;
use super::texture::{self, Pending};
use crate::assets::Image;
use crate::quality::QualityProfile;
use crate::scene::ShapeKind;
use crate::textures::{FaceInstance, Group};

/// A single opaque white texel, uploaded synchronously for every slot before
/// [`Textured::new`] returns, so a batch always has something valid to bind
/// even before its real image's turn in [`Textured::upload_pending`] comes
/// up. 1x1, with no mip chain beyond itself, so seeding every slot with one
/// costs nothing like the burst this module spreads out.
fn placeholder_image() -> Image {
    Image {
        width: 1,
        height: 1,
        pixels: vec![255, 255, 255, 255],
    }
}

/// Which of the previous scene's uploads each of `groups` can take over, by
/// slot: `Some(old)` where slot `old` held the very same asset's real image,
/// `None` where the group needs a placeholder and a deferred upload of its
/// own. `still_pending` names the old slots whose real image never got its
/// turn in [`Textured::upload_pending`] — what they hold is the placeholder,
/// not the asset, so they are not worth taking over.
fn reuse_plan(
    previous: &[AssetRef],
    still_pending: &[usize],
    groups: &[Group],
) -> Vec<Option<usize>> {
    let still_pending: HashSet<usize> = still_pending.iter().copied().collect();
    // `or_insert`: a reference never sits in two slots (see `Decor::assemble`),
    // but were it to, the first is the one a linear search would have found.
    let mut slot_of: HashMap<&AssetRef, usize> = HashMap::new();
    for (slot, reference) in previous.iter().enumerate() {
        slot_of.entry(reference).or_insert(slot);
    }
    groups
        .iter()
        .map(|group| {
            slot_of
                .get(&group.reference)
                .copied()
                .filter(|slot| !still_pending.contains(slot))
        })
        .collect()
}

const INSTANCES_LABEL: &str = "rbxview decal instances";

const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 10] = wgpu::vertex_attr_array![
    2 => Float32x4,
    3 => Float32x4,
    4 => Float32x4,
    5 => Float32x4,
    6 => Float32x3,
    7 => Float32x3,
    8 => Float32x3,
    9 => Float32x4,
    10 => Float32x4,
    11 => Uint32,
];

/// Depth bias to eliminate z-fighting between a decal and the surface it is
/// projected on, which are the same geometry drawn twice: the constant bias moves
/// the decal closer by a fixed depth-buffer step count, and the slope bias scales
/// with surface angle so small parts and huge baseplates behave alike.
///
/// Positive, not negative: reversed-Z (see camera.rs) makes closer mean a *bigger*
/// depth value, so pushing a decal toward the camera means adding to its depth
/// instead of subtracting from it.
const DEPTH_BIAS: wgpu::DepthBiasState = wgpu::DepthBiasState {
    constant: 16,
    slope_scale: 2.0,
    clamp: 0.0,
};

/// One face instance on the GPU: where the part stands, and how the image is
/// projected onto it — see `textured.wgsl` for how a fragment turns this into a
/// UV, and [`crate::textures::Projection`] for the projection itself.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct DecalRaw {
    model: [[f32; 4]; 4],
    face_normal: [f32; 3],
    u_axis: [f32; 3],
    v_axis: [f32; 3],
    uv_transform: [f32; 4],
    tint: [f32; 4],
    /// 1 on a wedge, whose slope belongs to the Front face however its normal
    /// classifies — the shader has no other way to tell one mesh from another.
    wedge: u32,
}

impl DecalRaw {
    fn new(face: &FaceInstance) -> Self {
        let projection = &face.projection;
        DecalRaw {
            model: face.model.to_cols_array_2d(),
            face_normal: projection.normal.to_array(),
            u_axis: projection.u.to_array(),
            v_axis: projection.v.to_array(),
            uv_transform: [
                projection.uv_scale[0],
                projection.uv_scale[1],
                projection.uv_offset[0],
                projection.uv_offset[1],
            ],
            tint: [face.tint[0], face.tint[1], face.tint[2], face.alpha],
            wedge: u32::from(face.kind == ShapeKind::Wedge),
        }
    }

    const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<DecalRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES,
        }
    }
}

/// One pass's batches, keyed by (image slot, shape) — the same key a
/// [`DecalRaw`] instance is grouped by in [`add_batches`] — with the
/// referent index [`Textured::sync`] moves a single edited instance through.
type Batches = Keyed<(usize, ShapeKind), (), DecalRaw, ()>;

/// Both textured passes and the GPU state they draw from.
pub(super) struct Textured {
    opaque_pipeline: wgpu::RenderPipeline,
    blended_pipeline: wgpu::RenderPipeline,
    /// The images themselves, kept beside the bind groups that view them so a
    /// quality level can re-view them without re-uploading (see
    /// [`Textured::set_quality`]).
    uploads: Vec<texture::Uploaded>,
    image_layout: wgpu::BindGroupLayout,
    images: Vec<wgpu::BindGroup>,
    /// Whether each image (by the same slot as `images`) carries any
    /// transparent pixels at all — [`Textured::sync`]'s own copy of the test
    /// [`Group`]'s own image answered once at load time, so a patched
    /// instance sorts into the same pass a full reload would put it in
    /// without needing the decoded image kept around just to ask again.
    image_alpha: Vec<bool>,
    /// Real images [`Textured::rebuild`] hasn't uploaded to their slot yet,
    /// each tagged with which one it belongs to — see
    /// [`Textured::upload_pending`].
    pending: Pending<(usize, Arc<Image>)>,
    /// Which asset each slot's image is, so a scene rebuild can take an
    /// upload over from the slot the previous scene held it in (see
    /// [`Textured::rebuild`]) rather than decode and upload it again.
    references: Vec<AssetRef>,
    opaque: Batches,
    blended: Batches,
}

impl Textured {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: Target,
        view_projection: &wgpu::BindGroupLayout,
        groups: &[Group],
        quality: &QualityProfile,
    ) -> Self {
        let image_layout = texture::layout(device);
        let layouts = [Some(view_projection), Some(&image_layout)];
        let mut textured = Textured {
            opaque_pipeline: create(device, target, &layouts, false),
            blended_pipeline: create(device, target, &layouts, true),
            uploads: Vec::new(),
            image_layout,
            images: Vec::new(),
            image_alpha: Vec::new(),
            pending: Pending::new([]),
            references: Vec::new(),
            opaque: Keyed::new(INSTANCES_LABEL),
            blended: Keyed::new(INSTANCES_LABEL),
        };
        textured.rebuild(device, queue, groups, quality);
        textured
    }

    /// Replaces every batch with `groups`' face instances, keeping the
    /// pipelines and, wherever a group's asset was already uploaded for the
    /// previous scene, that upload — slot by slot, so `images[i]` is still
    /// `groups[i]`'s image afterwards, whatever slot it sat in before.
    ///
    /// A group whose asset is new (or whose upload was still queued, and so
    /// only ever held the placeholder) gets a cheap placeholder up front
    /// rather than its real image: the real ones are decoded, full-size
    /// textures that can number in the dozens for one place, and uploading
    /// all of them here is exactly the load-time burst this module exists to
    /// spread across frames instead (see [`Textured::upload_pending`]).
    pub(super) fn rebuild(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        groups: &[Group],
        quality: &QualityProfile,
    ) {
        // Repeat, because a Texture's whole point is tiling past its own edges.
        let sampler = texture::sampler(device, wgpu::AddressMode::Repeat, quality.anisotropy);
        let previous = std::mem::take(&mut self.references);
        let still_pending: Vec<usize> = self
            .pending
            .take(usize::MAX)
            .into_iter()
            .map(|(slot, _)| slot)
            .collect();
        let plan = reuse_plan(&previous, &still_pending, groups);
        let mut held: Vec<Option<(texture::Uploaded, wgpu::BindGroup, bool)>> =
            std::mem::take(&mut self.uploads)
                .into_iter()
                .zip(std::mem::take(&mut self.images))
                .zip(std::mem::take(&mut self.image_alpha))
                .map(|((upload, image), alpha)| Some((upload, image, alpha)))
                .collect();

        let placeholder = placeholder_image();
        let mut pending = Vec::new();
        let mut opaque = Keyed::new(INSTANCES_LABEL);
        let mut blended = Keyed::new(INSTANCES_LABEL);
        for (slot, (group, reuse)) in groups.iter().zip(plan).enumerate() {
            match reuse.and_then(|old| held[old].take()) {
                Some((upload, image, alpha)) => {
                    self.uploads.push(upload);
                    self.images.push(image);
                    self.image_alpha.push(alpha);
                }
                None => {
                    let upload = texture::Uploaded::color(device, queue, &placeholder);
                    self.images.push(upload.bind(
                        device,
                        &self.image_layout,
                        &sampler,
                        quality.texture_max_size,
                    ));
                    self.image_alpha.push(group.image.has_alpha());
                    self.uploads.push(upload);
                    pending.push((slot, group.image.clone()));
                }
            }
            self.references.push(group.reference.clone());
            add_batches(device, &mut opaque, slot, &group.opaque);
            add_batches(device, &mut blended, slot, &group.blended);
        }

        self.pending = Pending::new(pending);
        self.opaque = opaque;
        self.blended = blended;
    }

    /// Uploads up to `budget` of the real images [`Textured::rebuild`] deferred,
    /// replacing that slot's placeholder bind group with the real one —
    /// called once per drawn frame (see `Renderer::draw`) with a bounded
    /// budget so a place with many textures spreads their GPU upload cost
    /// across the frames after load instead of paying for all of them before
    /// the first one. Pass [`usize::MAX`] to finish every remaining upload
    /// at once, for a caller with no next frame to spread the rest across
    /// (the single-shot `--screenshot` path — see `Renderer::finish_loading`).
    pub(super) fn upload_pending(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        quality: &QualityProfile,
        budget: usize,
    ) {
        if self.pending.is_empty() {
            return;
        }
        let sampler = texture::sampler(device, wgpu::AddressMode::Repeat, quality.anisotropy);
        for (slot, image) in self.pending.take(budget) {
            let upload = texture::Uploaded::color(device, queue, &image);
            self.images[slot] = upload.bind(
                device,
                &self.image_layout,
                &sampler,
                quality.texture_max_size,
            );
            self.uploads[slot] = upload;
        }
    }

    /// Brings both passes in line with one `Decal`/`Texture` instance whose
    /// part was just patched (see `Scene::patch_part`): rewritten in place
    /// if its (image, shape) batch is unchanged, otherwise moved to the one
    /// it belongs in now. Its image slot is looked up from whichever batch
    /// already holds it rather than resolved from `face`'s asset again,
    /// since a `CFrame`/`Size`/`Shape` edit never changes which `Texture` an
    /// instance points at — so `false` here means this renderer never drew
    /// this referent in the first place (its image never downloaded), not
    /// that the edit itself failed; there is nothing to catch up on.
    pub(super) fn sync(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        face: &FaceInstance,
    ) -> bool {
        let Some(image) = self
            .opaque
            .key_of(face.referent)
            .or_else(|| self.blended.key_of(face.referent))
            .map(|&(image, _)| image)
        else {
            return false;
        };

        let key = (image, face.kind);
        let raw = DecalRaw::new(face);
        if face.alpha >= 1.0 && !self.image_alpha[image] {
            self.blended.remove(queue, face.referent);
            self.opaque
                .sync(device, queue, face.referent, Some((key, raw, ())), |_| {
                    Some(())
                })
        } else {
            self.opaque.remove(queue, face.referent);
            self.blended
                .sync(device, queue, face.referent, Some((key, raw, ())), |_| {
                    Some(())
                })
        }
    }

    /// Re-views every decal at the new texture cap and anisotropy: one sampler
    /// and one bind group per image, with nothing decoded or uploaded again.
    pub(super) fn set_quality(&mut self, device: &wgpu::Device, quality: &QualityProfile) {
        let sampler = texture::sampler(device, wgpu::AddressMode::Repeat, quality.anisotropy);
        self.images = self
            .uploads
            .iter()
            .map(|upload| {
                upload.bind(
                    device,
                    &self.image_layout,
                    &sampler,
                    quality.texture_max_size,
                )
            })
            .collect();
    }

    /// Rebuilds both pipelines for a new sample count, which is the one quality
    /// knob baked into them.
    pub(super) fn set_target(
        &mut self,
        device: &wgpu::Device,
        target: Target,
        view_projection: &wgpu::BindGroupLayout,
    ) {
        let layouts = [Some(view_projection), Some(&self.image_layout)];
        self.opaque_pipeline = create(device, target, &layouts, false);
        self.blended_pipeline = create(device, target, &layouts, true);
    }

    pub(super) fn is_empty(&self) -> bool {
        !has_instances(&self.opaque) && !has_instances(&self.blended)
    }

    /// Draws the opaque face instances; call before anything translucent in the
    /// frame.
    pub(super) fn draw_opaque(&self, pass: &mut wgpu::RenderPass<'_>, meshes: &Meshes) {
        self.draw(pass, meshes, &self.opaque_pipeline, &self.opaque);
    }

    /// Draws the translucent face instances with the depth buffer read-only, so
    /// they blend over whatever is already there in whichever order they come.
    pub(super) fn draw_blended(&self, pass: &mut wgpu::RenderPass<'_>, meshes: &Meshes) {
        self.draw(pass, meshes, &self.blended_pipeline, &self.blended);
    }

    fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        meshes: &Meshes,
        pipeline: &wgpu::RenderPipeline,
        batches: &Batches,
    ) {
        if !has_instances(batches) {
            return;
        }

        pass.set_pipeline(pipeline);
        for group in batches.groups() {
            let count = group.slots.count();
            if count == 0 {
                continue;
            }
            let (image, kind) = group.key;
            let Some(mesh) = meshes.get(kind) else {
                continue;
            };
            pass.set_bind_group(1, &self.images[image], &[]);
            pass.set_vertex_buffer(1, group.slots.buffer().slice(..));
            mesh.draw(pass, count);
        }
    }
}

/// A batch an edit emptied is kept rather than dropped (see [`Keyed`]'s own
/// doc comment), so whether a pass has anything to draw at all needs a real
/// scan rather than `groups().is_empty()`.
fn has_instances(batches: &Batches) -> bool {
    batches.groups().iter().any(|group| group.slots.count() > 0)
}

/// Splits the face instances sharing one image into one batch per shape kind,
/// since each kind is a different mesh to instance.
fn add_batches(device: &wgpu::Device, batches: &mut Batches, image: usize, faces: &[FaceInstance]) {
    let mut grouped: Vec<(ShapeKind, Vec<(Ref, DecalRaw)>)> = Vec::new();
    for face in faces {
        let entry = (face.referent, DecalRaw::new(face));
        match grouped.iter_mut().find(|(kind, _)| *kind == face.kind) {
            Some((_, instances)) => instances.push(entry),
            None => grouped.push((face.kind, vec![entry])),
        }
    }

    for (kind, instances) in grouped {
        let roster = Roster::from_iter(
            instances
                .into_iter()
                .map(|(referent, raw)| (referent, raw, ())),
        );
        batches.add_group(device, (image, kind), (), roster);
    }
}

fn create(
    device: &wgpu::Device,
    target: Target,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
    blended: bool,
) -> wgpu::RenderPipeline {
    let buffers = [Some(Vertex::layout()), Some(DecalRaw::layout())];

    pipeline::surface(
        device,
        target,
        &Surface {
            translucent: blended,
            bias: DEPTH_BIAS,
            compare: wgpu::CompareFunction::GreaterEqual,
            ..Surface::new("rbxview decals", DECAL_SHADER, bind_group_layouts, &buffers)
        },
    )
}

#[cfg(test)]
#[path = "textured/tests.rs"]
mod tests;
