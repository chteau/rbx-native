//! `ViewportFrame` presentation: the frame's own parts drawn through the
//! ordinary shape pipelines into a texture of the frame's pixel size, which
//! the GUI then paints exactly like an `ImageLabel`'s image — so
//! `ImageColor3`/`ImageTransparency`, `UICorner`, `Rotation` and
//! `ClipsDescendants` all apply to it for free (see [`Viewports::bake_all`]).
//!
//! The lighting is the frame's own — one directional lamp plus an ambient
//! term — and nothing its docs deny it: no shadows, no sky or environment
//! (`EnvironmentSpecularScale`/`DiffuseScale` "act as if set to 0"), no fog,
//! no local lights, no post-processing. The surface shaders still declare
//! bind group 0 in full, so this pass carries stand-ins for the shadow maps,
//! the probe and the light buffers it never actually reads.
//!
//! Baked whenever the frame's tree is laid out, not per frame: nothing in a
//! `ViewportFrame` moves in this viewer, and its pixel size is only known
//! once its box is.

mod batch;
mod lighting;

use std::collections::HashMap;

use glam::Vec2;
use rbx_assets::AssetRef;
use wgpu::util::DeviceExt;

use super::atlas::Atlas;
use crate::quality::{QualityLevel, QualityProfile};
use crate::renderer::geometry::Meshes;
use crate::renderer::lighting::LightingRaw;
use crate::renderer::pipeline::{self, Frame, Shared, Target, DEPTH_FORMAT};
use crate::renderer::shadow::{Fit, Lamp};
use crate::scene::{GuiElement, GuiImageScale, GuiViewCamera, GuiViewport, Painted, Part};
use batch::{batch, pixels, target};
use lighting::{lighting_of, StandIns, NO_SKY};

/// Baked in the display's own encoding rather than HDR: the frame is an image
/// the GUI samples, and the GUI pass decodes sRGB on read.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const TARGET: Target = Target {
    format: FORMAT,
    samples: 1,
};
pub(super) struct Viewports {
    opaque: wgpu::RenderPipeline,
    blended: wgpu::RenderPipeline,
    /// Bind group 0: this pass's own matrix and lighting buffers over the
    /// stand-ins below.
    frame: Frame,
    lighting: wgpu::Buffer,
    meshes: Meshes,
    /// Clamped and linear, like a canvas': the texture is sampled over
    /// exactly its own rectangle.
    sampler: wgpu::Sampler,
    /// What `LightingRaw` reads of the level: an unlimited render distance
    /// and no probe sampling, whatever the display is drawn at.
    quality: QualityProfile,
    /// Kept alive beside the bind groups that view them — the stand-ins the
    /// frame group is built over, and every baked texture by its key.
    #[allow(dead_code)]
    stand_ins: StandIns,
    textures: HashMap<AssetRef, wgpu::Texture>,
}

impl Viewports {
    /// `material_layout` is the one the renderer's material arrays are bound
    /// with: a frame's parts sample those very arrays at bind group 1.
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material_layout: &wgpu::BindGroupLayout,
        quality: &QualityProfile,
    ) -> Self {
        let stand_ins = StandIns::new(device, queue, quality);
        let layout = pipeline::frame_layout(device);
        let lighting = pipeline::lighting_buffer(device);
        let shadow_view = stand_ins
            .shadow
            .create_view(&wgpu::TextureViewDescriptor::default());
        let local_shadow_view = stand_ins
            .local_shadow
            .create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
        let point_shadow_view = stand_ins
            .point_shadow
            .create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
        let frame = Frame::new(
            device,
            &layout,
            Shared {
                lighting: &lighting,
                lights: &stand_ins.lights,
                env: &stand_ins.env,
                shadow_map: &shadow_view,
                shadow_sampler: &stand_ins.shadow_sampler,
                local_shadow_map: &local_shadow_view,
                light_shadows: &stand_ins.light_shadows,
                point_shadow_map: &point_shadow_view,
                point_faces: &stand_ins.point_faces,
            },
        );
        let (opaque, blended) = pipeline::shape_pipelines(device, TARGET, &layout, material_layout);

        let mut quality = QualityLevel::default().profile();
        quality.render_distance = f32::INFINITY;
        quality.env_reflections = false;

        Viewports {
            opaque,
            blended,
            frame,
            lighting,
            meshes: Meshes::new(device, []),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("rbxview viewport frame"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            quality,
            stand_ins,
            textures: HashMap::new(),
        }
    }

    /// Bakes every `ViewportFrame` among `elements` that has a camera, parts
    /// and a box to draw into, and hands each its texture as `image`, so the
    /// quad build that follows paints it like any other image.
    ///
    /// `namespace` keeps one layout's keys apart from another's in the shared
    /// atlas — the screen overlay's from each canvas' — while a re-layout of
    /// the same tree (a resize) replaces its textures in place.
    pub(super) fn bake_all(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        materials: &wgpu::BindGroup,
        atlas: &mut Atlas,
        namespace: &str,
        elements: &mut [GuiElement],
    ) {
        let mut baked = 0;
        for element in elements.iter_mut() {
            let Some(viewport) = &element.viewport else {
                continue;
            };
            // No camera, nothing to look through: the docs default
            // `CurrentCamera` to nil and describe the frame as rendering
            // *through* one, so only the background is left.
            let Some(camera) = viewport.camera else {
                continue;
            };
            let parts: Vec<&Part> = viewport
                .parts
                .iter()
                .filter(|part| part.is_drawn())
                .collect();
            if viewport.alpha <= 0.0 || parts.is_empty() {
                continue;
            }
            let Some(size) = pixels(&element.rect) else {
                continue;
            };
            let (tint, alpha) = (viewport.tint, viewport.alpha);

            let texture = self.render(device, queue, materials, viewport, camera, &parts, size);
            let key = AssetRef::Thumb(format!("viewport-frame/{namespace}/{baked}"));
            baked += 1;
            atlas.adopt(device, key.clone(), &texture, &self.sampler);
            self.textures.insert(key.clone(), texture);
            element.image = Some(Painted {
                asset: key,
                tint,
                alpha,
                repeat: [1.0, 1.0],
                scale: GuiImageScale::Stretch,
                rect_offset: [0.0, 0.0],
                rect_size: [0.0, 0.0],
                pixelated: false,
            });
        }
    }

    /// Draws `parts` through `camera` into a fresh texture of `size` pixels,
    /// cleared to transparent so the frame's own background shows wherever
    /// nothing lands. Submitted on the spot: the frame's uniforms are shared
    /// between bakes, so each one has to be on the GPU before the next writes
    /// them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        materials: &wgpu::BindGroup,
        viewport: &GuiViewport,
        camera: GuiViewCamera,
        parts: &[&Part],
        size: (u32, u32),
    ) -> wgpu::Texture {
        let aspect = size.0 as f32 / size.1 as f32;
        let eye = camera.eye();
        self.frame.write(
            queue,
            &camera.view_projection(aspect),
            Vec2::new(size.0 as f32, size.1 as f32),
        );
        queue.write_buffer(
            &self.lighting,
            0,
            bytemuck::bytes_of(&LightingRaw::new(
                &lighting_of(viewport),
                eye,
                &NO_SKY,
                (Lamp::None, &Fit::unfitted()),
                0,
                &self.quality,
            )),
        );

        let (instances, runs) = batch(parts, eye);
        for run in &runs {
            self.meshes.ensure(device, run.kind);
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rbxview viewport frame instances"),
            contents: bytemuck::cast_slice(&instances),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let texture = target(
            device,
            size,
            FORMAT,
            // `COPY_SRC` so a bake can be read back off the GPU and checked.
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = target(
            device,
            size,
            DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rbxview viewport frame"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rbxview viewport frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    // Reversed-Z, like the main pass: nothing drawn yet is
                    // the smallest depth, not the largest.
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.opaque);
            pass.set_bind_group(0, &self.frame.bind_group, &[]);
            pass.set_bind_group(1, materials, &[]);
            pass.set_vertex_buffer(1, buffer.slice(..));
            let mut blending = false;
            for run in &runs {
                // The runs are ordered opaque first, so one switch is all it
                // takes, and the blended pipeline's own depth state then
                // keeps a translucent part from hiding what sits behind it.
                if run.blended && !blending {
                    pass.set_pipeline(&self.blended);
                    blending = true;
                }
                if let Some(mesh) = self.meshes.get(run.kind) {
                    mesh.draw_range(&mut pass, run.instances.clone());
                }
            }
        }
        queue.submit(std::iter::once(encoder.finish()));
        texture
    }
}

#[cfg(test)]
mod tests;
