//! Moving a built renderer to another graphics quality level.
//!
//! Nothing in a scene depends on the level: the meshes, the instance buffers and
//! the decoded images are all the same whatever it is. So a switch re-views and
//! rebinds rather than rebuilding — the texture cap becomes a `base_mip_level`
//! (see `texture::Uploaded`), anisotropy a fresh sampler, the shadow map one
//! reallocated depth texture, and the rest of the table lands in the uniforms the
//! next frame writes anyway.

use super::pipeline::{self, Shared, Target};
use super::{lighting, post, shadow, Renderer};
use crate::quality::QualityProfile;

impl Renderer {
    /// Draws every following frame at `quality`.
    ///
    /// Costs a handful of bind groups and, where the level changes the sample
    /// count, the surface pipelines; nothing is decoded, uploaded or re-batched.
    /// Setting the level it is already at still does the work, so callers that
    /// step levels should compare first.
    pub(crate) fn set_quality(&mut self, device: &wgpu::Device, quality: &QualityProfile) {
        self.quality = *quality;

        self.materials
            .set_quality(device, &self.material_layout, quality);
        self.textured.set_quality(device, quality);
        self.filemesh.set_quality(device, quality);
        if let Some(sky) = &mut self.sky {
            sky.set_quality(device, quality);
        }
        if let Some(bodies) = &mut self.bodies {
            bodies.set_quality(device, quality);
        }
        // Before the rebind below, which is what publishes the new views.
        self.env.set_quality(device, quality);
        self.shadows.set_quality(device, quality);
        self.cap_lights(device, quality);

        let target = Target {
            format: post::HDR_FORMAT,
            samples: self.post.set_quality(device, quality),
        };
        if target != self.target {
            self.target = target;
            self.rebuild_pipelines(device);
        }
        self.rebind_frames(device);
    }

    /// Rebuilds every pipeline that draws into the HDR target, which is the whole
    /// cost of crossing the level where multisampling starts.
    ///
    /// Both counts are not cached: a second set of nine pipelines doubles what
    /// opening a place compiles, to save a switch that happens at most once or
    /// twice a session.
    fn rebuild_pipelines(&mut self, device: &wgpu::Device) {
        let target = self.target;
        let (opaque, blended) =
            pipeline::shape_pipelines(device, target, &self.frame_layout, &self.material_layout);
        self.opaque = opaque;
        self.blended = blended;
        self.textured.set_target(device, target, &self.frame_layout);
        self.filemesh
            .set_target(device, target, (&self.frame_layout, &self.material_layout));
        if let Some(sky) = &mut self.sky {
            sky.set_target(device, target, &self.frame_layout);
        }
        if let Some(stars) = &mut self.stars {
            stars.set_target(device, target, &self.frame_layout);
        }
        if let Some(bodies) = &mut self.bodies {
            bodies.set_target(device, target, &self.frame_layout);
        }
        self.highlights
            .set_target(device, target, &self.frame_layout);
        self.cues.set_target(device, target, &self.frame_layout);
        self.adornments.set_target(device, target);
        self.preview = super::preview::Preview::new(device, target, &self.frame_layout);
        self.particles.set_target(device, target);
        self.beams.set_target(device, target);
        self.trails.set_target(device, target);
        self.gui.set_target(device, target);
    }

    /// Rewrites the local light buffer where the level allows a different number
    /// of them. A light the level does not allow is not uploaded at all, so it
    /// costs neither memory nor a loop step in the shader.
    fn cap_lights(&mut self, device: &wgpu::Device, quality: &QualityProfile) {
        let allowed = self.all_lights.len().min(quality.local_lights_max);
        if allowed == self.lights {
            return;
        }

        self.lights_buffer = lighting::local_lights_buffer(device, &self.all_lights[..allowed]);
        // Sized to match: `Renderer::draw` rewrites its contents every frame,
        // but the buffer itself has to exist at the new length before then.
        self.light_shadows_buffer = shadow::local::buffer(device, allowed);
        self.lights = allowed;
    }

    /// Rebuilds bind group 0 for every pass that has one: the probe's view, the
    /// shadow map and the light buffer have all just been replaced (or may
    /// have been, after a scene rebuild — see `renderer::rebuild`), and a bind
    /// group is immutable once created.
    pub(super) fn rebind_frames(&mut self, device: &wgpu::Device) {
        let shared = Shared {
            lighting: &self.lighting_buffer,
            lights: &self.lights_buffer,
            env: &self.env,
            shadow_map: self.shadows.view(),
            shadow_sampler: self.shadows.sampler(),
            local_shadow_map: self.shadows.local_view(),
            light_shadows: &self.light_shadows_buffer,
        };

        self.frame.rebind(device, &self.frame_layout, shared);
        if let Some(sky) = &mut self.sky {
            sky.camera.rebind(device, &self.frame_layout, shared);
        }
        if let Some(stars) = &mut self.stars {
            stars.camera.rebind(device, &self.frame_layout, shared);
        }
        if let Some(bodies) = &mut self.bodies {
            bodies.camera.rebind(device, &self.frame_layout, shared);
        }
    }
}
