//! The one colour pass of a frame, and the order everything in it has to be
//! drawn in.
//!
//! Split out of [`super::Renderer::draw`], which is left with what surrounds the
//! pass: the per-frame uniforms, the shadow map and the resolve.

use super::cull::MainCull;
use super::pipeline;
use super::post::Targets;
use super::{Renderer, CLEAR_COLOR};

impl Renderer {
    pub(super) fn scene_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        cull: &MainCull<'_>,
    ) {
        // Above one sample, `view` is the multisampled target and `resolve` the
        // single-sampled one the post chain reads: the hardware resolves between
        // them as the pass ends, so nothing downstream knows MSAA happened.
        let (view, resolve) = targets.color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rbxview scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: targets.depth(),
                // 0.0, not 1.0: reversed-Z (see camera.rs) puts infinity at depth 0
                // and the near plane at depth 1, so "nothing drawn yet" is the
                // smallest depth value, not the largest.
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

        // Sky first (it writes no depth), then the stars and the sun and moon
        // on top of it, then the opaque surfaces, then everything
        // translucent over the top.
        if let Some(sky) = &self.sky {
            sky.draw(&mut pass);
        }
        // Before the discs: a star behind the moon has no business showing
        // through it, and the discs are the brighter of the two.
        if let Some(stars) = &self.stars {
            stars.draw(&mut pass);
        }
        // Before the geometry, not after: the discs sit on the far plane, so
        // anything drawn later simply overwrites them where it stands.
        if let Some(bodies) = &self.bodies {
            bodies.draw(&mut pass);
        }

        let decals = self.quality.decals && !self.textured.is_empty();
        let bindings = pipeline::Bindings {
            frame: &self.frame.bind_group,
            materials: &self.materials.bind_group,
        };
        pass.set_pipeline(&self.opaque);
        pass.set_bind_group(0, bindings.frame, &[]);
        pass.set_bind_group(1, bindings.materials, &[]);
        self.shaped.draw(&mut pass, &self.meshes, cull);
        // Real mesh geometry, opaque like everything above: its own
        // pipelines (untextured, textured), rebinding the frame itself.
        self.filemesh.draw_opaque(&mut pass, bindings);

        if decals {
            pass.set_bind_group(0, &self.frame.bind_group, &[]);
            self.textured.draw_opaque(&mut pass, &self.meshes);
        }

        if !self.translucent.is_empty() {
            pass.set_pipeline(&self.blended);
            pass.set_bind_group(0, bindings.frame, &[]);
            pass.set_bind_group(1, bindings.materials, &[]);
            self.translucent.draw(&mut pass, &self.meshes);
        }
        self.filemesh.draw_blended(&mut pass, bindings);
        if decals {
            pass.set_bind_group(0, &self.frame.bind_group, &[]);
            self.textured.draw_blended(&mut pass, &self.meshes);
        }

        // Last, so the outline never gets drawn over by geometry it should
        // sit on top of.
        self.selection.draw(&mut pass, &self.frame.bind_group);
    }
}
