//! The images every GUI tree in a place samples, uploaded once and shared by
//! the screen overlay and every offscreen canvas.

use std::collections::HashMap;

use rbx_assets::AssetRef;

use super::super::rebuild::untried;
use super::super::texture;
use crate::assets::Image;
use crate::load::Answered;
use crate::quality::QualityProfile;

/// Where one image's bind groups live, and the pixel size `ScaleType.Fit`/
/// `Crop` need to letterbox or crop — the plan and layout stages never see an
/// actual texture, so this is the first point anything does.
#[derive(Debug, Clone, Copy)]
pub(super) struct Slot {
    /// Bilinear sampling, the default.
    pub(super) linear: usize,
    /// `ResampleMode.Pixelated`.
    pub(super) nearest: usize,
    pub(super) size: [f32; 2],
}

pub(super) struct Atlas {
    pub(super) image_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// A second sampler rather than a second `Atlas`: every image still needs
    /// only one upload, just two bind groups over the same texture view.
    nearest_sampler: wgpu::Sampler,
    groups: Vec<wgpu::BindGroup>,
    /// Kept alive beside the bind groups that view them — identical role to
    /// `renderer::trail::Slot`.
    #[allow(dead_code)]
    uploads: Vec<texture::Uploaded>,
    /// Only holds the images that actually decoded: an `ImageLabel` whose
    /// asset is missing draws nothing, the way Roblox itself leaves it blank.
    slot_of: HashMap<AssetRef, Slot>,
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
        // `ResampleMode.Pixelated`: nearest sampling has no notion of "along
        // the surface" to anisotropically filter, so this is always clamped
        // to 1 regardless of the quality profile.
        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rbxview gui sampler (pixelated)"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            anisotropy_clamp: 1,
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
            nearest_sampler,
            groups: Vec::new(),
            uploads: Vec::new(),
            slot_of: HashMap::new(),
            tried: HashMap::new(),
        };
        // The white texel is only ever sampled by an untextured quad
        // (`quads::WHITE`), addressed by that fixed index directly rather
        // than through `slot_of` — one bind group is all it ever needs.
        let uploaded = texture::Uploaded::color(device, queue, &white);
        atlas.groups.push(uploaded.bind(
            device,
            &atlas.image_layout,
            &atlas.sampler,
            quality.texture_max_size,
        ));
        atlas.uploads.push(uploaded);
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
            let slot = self.push(device, queue, image, quality);
            self.slot_of.insert(reference, slot);
        }
    }

    /// Uploads `image` once and binds it twice, linear then nearest — the two
    /// bind groups end up at adjacent indices, but `Slot` is what every
    /// caller actually addresses them by, so that is not load-bearing.
    fn push(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &Image,
        quality: &QualityProfile,
    ) -> Slot {
        let uploaded = texture::Uploaded::color(device, queue, image);
        let linear = self.groups.len();
        self.groups.push(uploaded.bind(
            device,
            &self.image_layout,
            &self.sampler,
            quality.texture_max_size,
        ));
        let nearest = self.groups.len();
        self.groups.push(uploaded.bind(
            device,
            &self.image_layout,
            &self.nearest_sampler,
            quality.texture_max_size,
        ));
        self.uploads.push(uploaded);
        Slot {
            linear,
            nearest,
            size: [image.width as f32, image.height as f32],
        }
    }

    pub(super) fn groups(&self) -> &[wgpu::BindGroup] {
        &self.groups
    }

    pub(super) fn slot_of(&self) -> &HashMap<AssetRef, Slot> {
        &self.slot_of
    }
}
