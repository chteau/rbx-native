//! The images every GUI tree in a place samples, uploaded once and shared by
//! the screen overlay and every offscreen canvas.

use std::collections::HashMap;

use rbx_assets::AssetRef;

use super::super::rebuild::untried;
use super::super::texture;
use super::text::GlyphAtlas;
use crate::assets::Image;
use crate::load::Answered;
use crate::quality::QualityProfile;

/// The slot text quads sample: the glyph atlas, uploaded from
/// [`Atlas::sync_glyphs`] rather than decoded from an asset.
pub(super) const GLYPHS: usize = 1;

/// Coverage, not colour: a glyph's alpha must not be sRGB-decoded on the way
/// to the shader, and its white RGB is 1.0 in either encoding.
const GLYPH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub(super) struct Atlas {
    pub(super) image_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    groups: Vec<wgpu::BindGroup>,
    /// The glyph texture behind slot [`GLYPHS`] and its side, replaced when
    /// the CPU atlas grows past it.
    glyphs: Option<(u32, wgpu::Texture)>,
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
            glyphs: None,
            slot_of: HashMap::new(),
            tried: HashMap::new(),
        };
        atlas.push(device, queue, &white, quality);
        // Slot `GLYPHS` is taken now so the image slots after it never move;
        // the first `sync_glyphs` replaces it with the real atlas.
        atlas.push(device, queue, &white, quality);
        atlas.extend(device, queue, references, images, quality);
        atlas
    }

    /// Uploads the glyph atlas into slot [`GLYPHS`] if it changed since the
    /// last call — after every quad build, before the draw that samples it.
    pub(super) fn sync_glyphs(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        glyphs: &mut GlyphAtlas,
    ) {
        let Some((side, pixels)) = glyphs.take_dirty() else {
            return;
        };
        if self.glyphs.as_ref().map(|(held, _)| *held) != Some(side) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("rbxview gui glyphs"),
                size: wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: GLYPH_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.groups[GLYPHS] = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rbxview gui glyphs"),
                layout: &self.image_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.glyphs = Some((side, texture));
        }
        let Some((_, texture)) = &self.glyphs else {
            return;
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(side * 4),
                rows_per_image: Some(side),
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
        );
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

#[cfg(test)]
mod tests {
    use super::super::text::Typesetter;
    use super::*;
    use crate::quality::QualityLevel;
    use crate::scene::{GuiAlign, GuiText, GuiTextSpan};

    // The glyph slot has to be there before any image slot, and be replaced
    // in place — not appended — once real glyphs land, or every `ImageLabel`
    // would sample the wrong texture.
    #[test]
    fn the_glyph_slot_is_reserved_up_front_and_filled_in_place() {
        let Some((device, queue)) = crate::gpu::for_tests() else {
            return;
        };
        let quality = QualityLevel::Automatic.profile();
        let mut atlas = Atlas::new(&device, &queue, &[], &Answered::new(), &quality);
        assert_eq!(atlas.groups().len(), 2);
        assert!(atlas.glyphs.is_none());

        let mut fonts = Typesetter::new();
        atlas.sync_glyphs(&device, &queue, &mut fonts.atlas);
        assert!(
            atlas.glyphs.is_none(),
            "nothing rasterised, nothing uploaded"
        );

        let text = GuiText {
            spans: vec![GuiTextSpan::plain("Ag")],
            color: [1.0; 3],
            alpha: 1.0,
            size: 24.0,
            scaled: false,
            wrapped: false,
            x_align: GuiAlign::Start,
            y_align: GuiAlign::Start,
            face: crate::fonts::Face::default(),
            line_height: 1.0,
            stroke: None,
            truncate: false,
            max_graphemes: None,
            automatic: [false, false],
            size_bounds: None,
        };
        let buffer = fonts.shape(&text, 24.0, None, None);
        let keys: Vec<_> = buffer
            .layout_runs()
            .flat_map(|run| {
                run.glyphs
                    .iter()
                    .map(|glyph| glyph.physical((0.0, 0.0), 1.0).cache_key)
            })
            .collect();
        if keys.is_empty() {
            return;
        }
        for key in keys {
            fonts.glyph(key);
        }

        atlas.sync_glyphs(&device, &queue, &mut fonts.atlas);
        let side = fonts.atlas.side();
        assert_eq!(atlas.glyphs.as_ref().map(|(held, _)| *held), Some(side));
        assert_eq!(atlas.groups().len(), 2, "replaced in place");
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("the upload completes");
    }
}
