//! The images every GUI tree in a place samples, uploaded once and shared by
//! the screen overlay and every offscreen canvas.

use std::collections::HashMap;
use std::sync::Arc;

use rbx_assets::AssetRef;

use super::super::texture;
use crate::assets::Image;
use crate::quality::QualityProfile;

pub(super) struct Atlas {
    pub(super) image_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    groups: Vec<wgpu::BindGroup>,
    /// Kept alive beside the bind groups that view them — identical role to
    /// `renderer::trail::Slot`.
    #[allow(dead_code)]
    uploads: Vec<texture::Uploaded>,
    /// Only holds the images that actually decoded: an `ImageLabel` whose
    /// asset is missing draws nothing, the way Roblox itself leaves it blank.
    slot_of: HashMap<AssetRef, usize>,
}

impl Atlas {
    /// An atlas holding only the flat-white fallback every background and
    /// border samples (`quads::WHITE`); [`Atlas::extend`] adds the images.
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        quality: &QualityProfile,
    ) -> Self {
        let image_layout = texture::layout(device);
        // `ScaleType.Tile` is the whole reason for `Repeat`: a stretched image
        // never reaches a UV outside 0..1 anyway.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rbxview gui sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: quality.anisotropy.max(1),
            ..Default::default()
        });

        let white = Image {
            width: 1,
            height: 1,
            pixels: vec![255, 255, 255, 255],
        };
        let mut atlas = Atlas {
            image_layout,
            sampler,
            groups: Vec::new(),
            uploads: Vec::new(),
            slot_of: HashMap::new(),
        };
        atlas.push(device, queue, &white, quality);
        atlas
    }

    /// Uploads whichever of `references` the atlas does not hold yet, out of
    /// `images` (what the loader decoded — see `Decor::gui`; a reference it
    /// lacks never downloaded and is skipped), keeping every slot already
    /// handed out: what a scene rebuild calls, so the same `ImageLabel`
    /// images are not uploaded twice. Slots are handed out in `references`'
    /// order rather than the map's, so the same file draws the same
    /// texture-to-slot mapping twice in a row.
    pub(super) fn extend(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        references: &[AssetRef],
        images: &HashMap<AssetRef, Arc<Image>>,
        quality: &QualityProfile,
    ) {
        for reference in references {
            if self.slot_of.contains_key(reference) {
                continue;
            }
            let Some(image) = images.get(reference) else {
                continue;
            };
            self.push(device, queue, image, quality);
            self.slot_of
                .insert(reference.clone(), self.groups.len() - 1);
        }
    }

    fn push(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &Image,
        quality: &QualityProfile,
    ) {
        let uploaded = texture::Uploaded::color(device, queue, image);
        self.groups.push(uploaded.bind(
            device,
            &self.image_layout,
            &self.sampler,
            quality.texture_max_size,
        ));
        self.uploads.push(uploaded);
    }

    pub(super) fn groups(&self) -> &[wgpu::BindGroup] {
        &self.groups
    }

    pub(super) fn slot_of(&self) -> &HashMap<AssetRef, usize> {
        &self.slot_of
    }
}
