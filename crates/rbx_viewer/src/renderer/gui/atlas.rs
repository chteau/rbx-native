//! The images every GUI tree in a place samples, uploaded once and shared by
//! the screen overlay and every offscreen canvas.

use std::collections::HashMap;

use rbx_assets::AssetRef;

use super::super::texture;
use crate::assets::{self, Image};
use crate::quality::QualityProfile;

pub(super) struct Atlas {
    pub(super) image_layout: wgpu::BindGroupLayout,
    groups: Vec<wgpu::BindGroup>,
    /// Kept alive beside the bind groups that view them — identical role to
    /// `renderer::trail::Slot`.
    #[allow(dead_code)]
    uploads: Vec<texture::Uploaded>,
    /// Only holds the images that actually downloaded: an `ImageLabel` whose
    /// asset is missing draws nothing, the way Roblox itself leaves it blank.
    slot_of: HashMap<AssetRef, usize>,
}

impl Atlas {
    /// Downloads and uploads `references`, the flat-white fallback every
    /// background and border samples (`quads::WHITE`) first.
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        references: &[AssetRef],
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
            groups: Vec::new(),
            uploads: Vec::new(),
            slot_of: HashMap::new(),
        };
        atlas.push(device, queue, &sampler, &white, quality);

        // Live-effect asset warnings aren't wired to the Output dock yet — see
        // `assets::load`'s doc comment; only scene-load-time warnings are.
        let (images, _warnings) = assets::load(references);
        for reference in references {
            let Some(image) = images.get(reference) else {
                continue;
            };
            atlas.push(device, queue, &sampler, image, quality);
            atlas
                .slot_of
                .insert(reference.clone(), atlas.groups.len() - 1);
        }
        atlas
    }

    fn push(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sampler: &wgpu::Sampler,
        image: &Image,
        quality: &QualityProfile,
    ) {
        let uploaded = texture::Uploaded::color(device, queue, image);
        self.groups.push(uploaded.bind(
            device,
            &self.image_layout,
            sampler,
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
