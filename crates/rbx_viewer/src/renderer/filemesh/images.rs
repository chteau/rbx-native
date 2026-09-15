//! The `TextureID` images file meshes are skinned with, deduplicated as batches
//! ask for them.
//!
//! Kept apart from the batches so the images outlive any one quality level: a cap
//! change re-views them (see [`Images::rebind`]) rather than downloading or
//! uploading anything again.

use std::collections::HashMap;

use rbx_assets::AssetRef;

use super::super::texture::Uploaded;
use super::pipelines::Skin;
use super::GroupKey;
use crate::scene::Resolved;

/// Everything binding one image needs besides the image: they never vary within a
/// scene, only with the quality level.
#[derive(Clone, Copy)]
pub(super) struct Binding<'a> {
    pub(super) layout: &'a wgpu::BindGroupLayout,
    pub(super) sampler: &'a wgpu::Sampler,
    pub(super) max_size: u32,
}

#[derive(Default)]
pub(super) struct Images {
    slots: HashMap<AssetRef, usize>,
    uploads: Vec<Uploaded>,
    pub(super) bind_groups: Vec<wgpu::BindGroup>,
}

impl Images {
    /// The skin one group key resolves to, or `None` when its texture never
    /// downloaded and the batch has to be dropped.
    pub(super) fn slot(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        binding: Binding<'_>,
        resolved: &Resolved,
        key: &GroupKey,
    ) -> Option<Skin> {
        if let Some(index) = key.appearance {
            return Some(Skin::Appearance(index));
        }
        let Some(reference) = &key.texture else {
            return Some(Skin::Plain);
        };

        let image = resolved.images.get(reference)?;
        let slot = *self.slots.entry(reference.clone()).or_insert_with(|| {
            let upload = Uploaded::color(device, queue, image);
            self.bind_groups.push(upload.bind(
                device,
                binding.layout,
                binding.sampler,
                binding.max_size,
            ));
            self.uploads.push(upload);
            self.bind_groups.len() - 1
        });
        Some(Skin::Image(slot))
    }

    /// Re-views every image at the new cap, behind a sampler rebuilt at the new
    /// anisotropy.
    pub(super) fn rebind(&mut self, device: &wgpu::Device, binding: Binding<'_>) {
        self.bind_groups = self
            .uploads
            .iter()
            .map(|upload| upload.bind(device, binding.layout, binding.sampler, binding.max_size))
            .collect();
    }
}
