//! The two colour passes of a frame, and the order everything in them has to
//! be drawn in.
//!
//! Split out of [`super::Renderer::draw`], which is left with what surrounds
//! them: the per-frame uniforms, the shadow map and the resolve.
//!
//! Two rather than one because a `Glass` surface reads the scene behind
//! itself out of a copy (see `renderer::post::Targets::capture_refraction`),
//! and a texture cannot be both a colour attachment and a bound resource in
//! the same pass. The opaque half ends, the copy is taken, and everything
//! that blends over it — the translucent geometry, the editor's own cues —
//! goes in the second.

use super::cull::MainCull;
use super::pipeline;
use super::post::Targets;
use super::{Renderer, CLEAR_COLOR};

impl Renderer {
    /// The opaque half: the sky and everything that writes depth.
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
    }

    /// Everything that blends over the opaque half, in the same order it was
    /// drawn in when the two were one pass: the translucent geometry, then
    /// the adornments the place asks for, then the editor's own cues.
    pub(super) fn overlay_pass(&self, encoder: &mut wgpu::CommandEncoder, targets: &Targets) {
        let (view, resolve) = targets.color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rbxview scene overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: resolve,
                // Loaded, never cleared: the opaque pass just drew into this
                // very attachment.
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: targets.depth(),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        let decals = self.quality.decals && !self.textured.is_empty();
        let bindings = pipeline::Bindings {
            frame: &self.frame.bind_group,
            materials: &self.materials.bind_group,
        };
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

        // Place content, so before the editor's own cues below: an
        // adornment is something the file asks for, where a selection
        // outline is something this editor draws about it.
        self.adornments.draw(&mut pass, &self.frame.bind_group);

        // Last, so the outlines never get drawn over by geometry they should
        // sit on top of. Hover after selection: `Shell` never sends a hover
        // for an already-selected referent, so the two never contest the
        // same box, but drawing hover second is still the more sensible
        // order if that ever changes — the "about to click" cue reading as
        // the top layer rather than being hidden under the selection box.
        self.selection.draw(&mut pass, &self.frame.bind_group);
        self.hover.draw(&mut pass, &self.frame.bind_group);
        // Over the outlines, under the draggers: a preview says where the
        // selection would land, so it belongs beside its outline rather
        // than over the handles being dragged.
        self.preview.draw(&mut pass, &self.frame.bind_group);
        // After both outlines, and with no depth test of its own: a dragger
        // is a control rather than scenery, and one buried inside the part it
        // moves would be impossible to grab.
        self.draggers.draw(&mut pass, &self.frame.bind_group);
    }
}
