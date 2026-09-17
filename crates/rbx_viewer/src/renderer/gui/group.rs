//! `CanvasGroup` compositing. The docs apply `GroupColor3` and
//! `GroupTransparency` "to the rendered result" of the group's descendants,
//! flattened — so two overlapping children under a half-transparent group
//! blend with the screen once, as one picture, not twice. That needs the
//! subtree painted on its own first: [`flatten`] paints each tinted group's
//! run of elements into a texture of the group's pixel size, with the very
//! painter the overlay uses, and hands back the group as a single element
//! sampling that texture (see `quads::build`).
//!
//! A group at the default tint is left alone: it draws exactly like a
//! `Frame`, and costs nothing extra.

use super::atlas::Atlas;
use super::paint::Painter;
use super::text::Typesetter;
use crate::scene::{GuiElement, GuiGroup, GuiRect};

/// The textures one overlay's groups were flattened into, kept alive beside
/// the bind groups sampling them and thrown away with the overlay.
pub(super) struct Baked {
    sampler: wgpu::Sampler,
    textures: Vec<(wgpu::Texture, wgpu::BindGroup)>,
}

impl Baked {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        Baked {
            // Clamped, like a `SurfaceGui` canvas: the quad samples exactly
            // the texture's own rectangle, and a repeat could only bleed
            // the far edge in along the near one.
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("rbxview gui group"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            textures: Vec::new(),
        }
    }

    pub(super) fn clear(&mut self) {
        self.textures.clear();
    }

    /// Every texture a run can name: the atlas' slots first, then the groups
    /// in the order [`flatten`] baked them — which is the order it numbered
    /// their [`GuiGroup::texture`] in.
    pub(super) fn bindings(&self, atlas: &Atlas) -> Vec<wgpu::BindGroup> {
        let mut bindings = atlas.groups().to_vec();
        bindings.extend(self.textures.iter().map(|(_, group)| group.clone()));
        bindings
    }
}

/// Everything a bake needs, borrowed from the overlay for the duration.
pub(super) struct Bake<'a> {
    pub(super) device: &'a wgpu::Device,
    pub(super) queue: &'a wgpu::Queue,
    /// The overlay's own painter: its target format is what the group
    /// texture is created in, so the pipeline needs no second copy.
    pub(super) painter: &'a mut Painter,
    pub(super) atlas: &'a mut Atlas,
    pub(super) fonts: &'a mut Typesetter,
    pub(super) format: wgpu::TextureFormat,
    pub(super) baked: &'a mut Baked,
}

/// `elements` with every tinted group and its subtree replaced by the group
/// alone, its [`GuiGroup::texture`] naming the slot the subtree was painted
/// into; everything else passes through untouched.
pub(super) fn flatten(bake: &mut Bake, elements: Vec<GuiElement>) -> Vec<GuiElement> {
    let mut flat = Vec::with_capacity(elements.len());
    let mut index = 0;
    while index < elements.len() {
        let element = &elements[index];
        let Some(group) = element.group.filter(|group| !group.tint.is_default()) else {
            flat.push(element.clone());
            index += 1;
            continue;
        };
        let end = (index + 1 + group.descendants).min(elements.len());
        // Inner groups first: a nested tinted group is itself one flattened
        // picture inside its parent's.
        let subtree = flatten(bake, local(&elements[index..end]));
        let slot = bake.paint(&subtree, texture_size(bake.device, &element.rect));
        flat.push(GuiElement {
            group: Some(GuiGroup {
                texture: Some(slot),
                ..group
            }),
            ..element.clone()
        });
        index = end;
    }
    flat
}

/// A group's run of elements moved into the group's own frame: the group's
/// corner at the origin, and its rotation — which every descendant carries
/// as part of its own — taken back out, since the finished picture turns as
/// one when the group's quad does. The group itself loses its ancestors'
/// scissor (the quad is clipped by it instead) and its group-ness (or it
/// would be flattened again).
fn local(run: &[GuiElement]) -> Vec<GuiElement> {
    let group = &run[0];
    let angle = group.rotation;
    let pivot = [
        group.rect.x + group.rect.width * 0.5,
        group.rect.y + group.rect.height * 0.5,
    ];
    let origin = group.rect.turned(-angle, pivot);
    let shifted = |rect: &GuiRect| GuiRect {
        x: rect.x - origin.x,
        y: rect.y - origin.y,
        ..*rect
    };
    let mut local: Vec<GuiElement> = run
        .iter()
        .map(|element| GuiElement {
            rect: shifted(&element.rect.turned(-angle, pivot)),
            // A rotated chain leaves every clip `None` already (see
            // `scene::gui::layout::emit`), so a clip that is here is a plain
            // translation away from where it belongs.
            clip: element.clip.map(|clip| shifted(&clip)),
            rotation: element.rotation - angle,
            ..element.clone()
        })
        .collect();
    local[0].clip = None;
    local[0].group = None;
    local
}

/// Whole pixels, at least one each way, and no bigger than the device can
/// hold — the docs themselves warn a group "consumes extra texture memory".
fn texture_size(device: &wgpu::Device, rect: &GuiRect) -> (u32, u32) {
    let cap = device.limits().max_texture_dimension_2d;
    let side = |extent: f32| (extent.ceil().max(1.0) as u32).min(cap);
    (side(rect.width), side(rect.height))
}

/// The one place a group texture is created, so its format and usage are
/// switched in one place should the compositing colour space move.
fn offscreen(
    device: &wgpu::Device,
    size: (u32, u32),
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rbxview gui group"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

impl Bake<'_> {
    /// Paints `elements` into a fresh texture of `size` pixels, submitted on
    /// the spot so the overlay's own pass can sample it, and returns the slot
    /// the texture answers to.
    fn paint(&mut self, elements: &[GuiElement], size: (u32, u32)) -> usize {
        let texture = offscreen(self.device, size, self.format);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        self.painter.prepare(
            self.device,
            self.queue,
            elements,
            self.atlas.slot_of(),
            size,
            self.fonts,
        );
        self.atlas
            .sync_glyphs(self.device, self.queue, &mut self.fonts.atlas);
        let bindings = self.baked.bindings(self.atlas);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rbxview gui group"),
            });
        // Transparent, not a colour: whatever the subtree leaves uncovered
        // shows what is under the group.
        self.painter.draw(
            &mut encoder,
            &view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &bindings,
            size,
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rbxview gui group"),
            layout: &self.atlas.image_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.baked.sampler),
                },
            ],
        });
        self.baked.textures.push((texture, bind_group));
        bindings.len()
    }
}

#[cfg(test)]
mod tests;
