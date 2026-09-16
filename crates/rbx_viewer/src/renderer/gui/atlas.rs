//! The images every GUI tree in a place samples, uploaded once and shared by
//! the screen overlay and every offscreen canvas.

use std::collections::HashMap;

use rbx_assets::AssetRef;

use super::super::rebuild::untried;
use super::super::texture;
use crate::assets::Image;
use crate::load::Answered;
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
    /// Every reference [`Atlas::extend`] ever tried, the failed ones
    /// included — `slot_of` cannot tell those from one never asked for, and
    /// a scene rebuild must not fetch a missing asset again on every edit.
    tried: HashMap<AssetRef, ()>,
}

impl Atlas {
    /// Uploads whichever of `references` the loader has already decoded, the
    /// flat-white fallback every background and border samples
    /// (`quads::WHITE`) first.
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        references: &[AssetRef],
        images: &Answered,
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
            tried: HashMap::new(),
        };
        atlas.push(device, queue, &white, quality);
        atlas.extend(device, queue, references, images, quality);
        atlas
    }

    /// Uploads whichever of `references` the loader has decoded and the atlas
    /// has not taken yet, keeping every slot already handed out: what a scene
    /// rebuild calls, so the same `ImageLabel` image is never uploaded twice.
    ///
    /// A reference `images` has no answer for is left untried, so the rebuild
    /// that follows its landing picks it up; one answered `None` is recorded
    /// as tried and never asked about again.
    pub(super) fn extend(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        references: &[AssetRef],
        images: &Answered,
        quality: &QualityProfile,
    ) {
        for reference in untried(&self.tried, references.iter().cloned()) {
            let Some(answer) = images.get(&reference) else {
                continue;
            };
            self.tried.insert(reference.clone(), ());
            let Some(image) = answer else {
                continue;
            };
            self.push(device, queue, image, quality);
            self.slot_of.insert(reference, self.groups.len() - 1);
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
