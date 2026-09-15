//! The HDR frame and what turns it back into an image.
//!
//! Every scene pass draws into [`HDR_FORMAT`] with nothing clamped, which is
//! what lets a neon face or a blown-out sky carry a value above 1 as far as the
//! bloom. This module owns that target (and the depth buffer beside it, which
//! `DepthOfFieldEffect` reads back per pixel), builds the bloom from it, grades
//! the result with the place's `ColorCorrectionEffect` and resolves the lot —
//! `SunRaysEffect`'s god-rays included — into whatever the caller is presenting
//! to.

mod bloom;
mod chain;
mod pipelines;
mod samples;
mod targets;
mod uniform;

use glam::Vec2;

use crate::lighting::Effects;
use crate::quality::QualityProfile;
use pipelines::{blur_layout, fullscreen, sample_layout};
use samples::supported;
use targets::Sources;
pub(super) use targets::Targets;

/// Half floats, not 8-bit: the whole point of the offscreen target is the range
/// above 1 the threshold compares against.
pub(super) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

const SHADER: &str = include_str!("post.wgsl");

/// The one line of [`SHADER`] that cannot be written for both shapes of the
/// scene's depth buffer at once, and what it becomes above one sample.
///
/// WGSL types a multisampled depth texture as a different type entirely, so the
/// resolve is compiled twice from the same source with this substituted — far
/// less machinery than a second shader file kept in step with the first by hand,
/// and the resolve is the only entry point that reads depth at all.
const DEPTH_BINDING: &str = "var scene_depth: texture_depth_2d";
const MULTISAMPLED_DEPTH_BINDING: &str = "var scene_depth: texture_depth_multisampled_2d";

/// The bloom chain starts at half the frame, which is where its first level's
/// size comes from.
const FIRST_LEVEL_DIVISOR: u32 = 2;

/// Radius of the one blurred copy of the frame `DepthOfFieldEffect` mixes in, in
/// pixels at 1080p — the unit `BlurEffect.Size` and `BloomEffect.Size` are both
/// quoted in, and scaled to the frame by the same function.
///
/// A true depth of field varies its radius per pixel with the distance from the
/// focus plane, which costs every pixel a gather as wide as the worst case. One
/// fixed blur mixed in by distance is the cheap stand-in this pass uses instead,
/// so this is the radius the *most* out-of-focus pixel gets: wide enough to read
/// as out of focus rather than merely soft, without smearing a whole part into
/// its background.
const DOF_BLUR_SIZE: f32 = 32.0;

/// Additive, so an upsampled level brightens the one below it rather than
/// replacing it: the chain is a sum of blurs, each wider than the last.
const ADDITIVE: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent::REPLACE,
};

pub(super) struct Post {
    sample_layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    uniform_bind: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    threshold: wgpu::RenderPipeline,
    downsample: wgpu::RenderPipeline,
    upsample: wgpu::RenderPipeline,
    /// One per depth binding the scene's depth buffer can have: WGSL types a
    /// multisampled depth texture differently from a single-sampled one, and the
    /// resolve reads it for `DepthOfFieldEffect`. Which of the two runs follows
    /// `samples`, so a change of quality level never has to rebuild a pipeline.
    resolve: wgpu::RenderPipeline,
    resolve_multisampled: wgpu::RenderPipeline,
    blur_layout: wgpu::BindGroupLayout,
    blur_multisampled_layout: wgpu::BindGroupLayout,
    effects: Effects,
    /// What the quality level allows of the two: a skipped bloom leaves the
    /// resolve in place (it is the only pass that writes the final image) and
    /// simply stops the chain from being built or added in.
    bloom: bool,
    color_correction: bool,
    /// Samples every scene pass is drawn at, which is [`QualityProfile::msaa_samples`]
    /// clamped to what the adapter will multisample and resolve.
    samples: u32,
    targets: Option<Targets>,
}

impl Post {
    pub(super) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        effects: Effects,
        quality: &QualityProfile,
    ) -> Self {
        let sample_layout = sample_layout(device);
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rbxview post"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rbxview post"),
            size: std::mem::size_of::<uniform::PostRaw>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rbxview post"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });

        let one_source = [Some(&uniform_layout), Some(&sample_layout)];
        let blur = blur_layout(device, false);
        let blur_multisampled = blur_layout(device, true);
        // The uniform, the scene, the bloom and then the blurred frame beside the
        // scene's depth buffer, in the order `fs_resolve` declares them as groups
        // 0 through 3.
        let resolve_sources = |blur| {
            [
                Some(&uniform_layout),
                Some(&sample_layout),
                Some(&sample_layout),
                Some(blur),
            ]
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rbxview post"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        // The same shader with the depth binding retyped: WGSL has no
        // `textureLoad` that takes both, and a multisampled texture cannot be
        // declared as a plain `texture_depth_2d` at all.
        let multisampled_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rbxview post (msaa depth)"),
            source: wgpu::ShaderSource::Wgsl(
                SHADER
                    .replace(DEPTH_BINDING, MULTISAMPLED_DEPTH_BINDING)
                    .into(),
            ),
        });

        Post {
            threshold: fullscreen(
                device,
                &shader,
                "fs_threshold",
                &one_source,
                HDR_FORMAT,
                None,
            ),
            downsample: fullscreen(
                device,
                &shader,
                "fs_downsample",
                &one_source,
                HDR_FORMAT,
                None,
            ),
            upsample: fullscreen(
                device,
                &shader,
                "fs_upsample",
                &one_source,
                HDR_FORMAT,
                Some(ADDITIVE),
            ),
            resolve: fullscreen(
                device,
                &shader,
                "fs_resolve",
                &resolve_sources(&blur),
                format,
                None,
            ),
            resolve_multisampled: fullscreen(
                device,
                &multisampled_shader,
                "fs_resolve",
                &resolve_sources(&blur_multisampled),
                format,
                None,
            ),
            // ClampToEdge and linear: every pass here samples between texels,
            // and wrapping would pull the far side of the frame into the blur.
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("rbxview post"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            sample_layout,
            blur_layout: blur,
            blur_multisampled_layout: blur_multisampled,
            uniform,
            uniform_bind,
            effects,
            bloom: quality.bloom,
            color_correction: quality.color_correction,
            samples: supported(device, quality.msaa_samples),
            targets: None,
        }
    }

    /// The sample count every surface pipeline must be built with.
    pub(super) fn samples(&self) -> u32 {
        self.samples
    }

    /// Swaps in a freshly read `Lighting.Effects` — a `PostEffect` edit's fast
    /// path (see `Renderer::update_lighting`). Safe with no rebuild: every
    /// pipeline here is built once and reads `self.effects` fresh every
    /// `prepare`, so nothing pins the old value but this field.
    pub(super) fn set_effects(&mut self, effects: Effects) {
        self.effects = effects;
    }

    /// Follows the quality level: which post passes run, and how many samples the
    /// scene is drawn at. The HDR target and the bloom chain are sized by the
    /// frame rather than by the level, so a change of level reallocates them only
    /// where it changes the sample count — which the next [`Post::prepare`] does.
    ///
    /// Answers the sample count the surface pipelines now have to match.
    pub(super) fn set_quality(&mut self, device: &wgpu::Device, quality: &QualityProfile) -> u32 {
        self.bloom = quality.bloom;
        self.color_correction = quality.color_correction;
        self.samples = supported(device, quality.msaa_samples);
        self.samples
    }

    /// Rebuilds the targets if the frame changed size, and writes the uniform
    /// the resolve reads. `None` where the frame has no pixels at all.
    ///
    /// `sun_screen` is this frame's answer from
    /// [`super::sun::sun_screen_position`] — `None` silences `SunRaysEffect`
    /// outright regardless of what the place's own `Intensity` says, which is
    /// how a place with the effect enabled still draws nothing on a frame
    /// where the moon is out or the sun is off-screen.
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        sun_screen: Option<Vec2>,
    ) -> Option<&Targets> {
        if size.0 == 0 || size.1 == 0 {
            return None;
        }

        let bloom_blur = bloom::blur(self.effects.bloom.size, size.1);
        let blur_levels = self.blur_levels(size.1);
        if self
            .targets
            .as_ref()
            .is_none_or(|targets| !targets.fit(size, bloom_blur.levels, blur_levels, self.samples))
        {
            self.targets = Some(Targets::new(
                device,
                self.sources(),
                size,
                bloom_blur.levels,
                blur_levels,
                self.samples,
            ));
        }

        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&self.raw(bloom_blur.tent, sun_screen)),
        );

        self.targets.as_ref()
    }

    /// The current frame's targets, once [`Post::prepare`] has built them.
    pub(super) fn targets(&self) -> Option<&Targets> {
        self.targets.as_ref()
    }

    /// How deep the blurred copy of the frame has to be downsampled, 0 building
    /// no chain at all.
    ///
    /// `BlurEffect` reuses the exact same resolution-aware chain sizing as
    /// `BloomEffect`, just on the whole frame instead of the thresholded part of
    /// it, and `DepthOfFieldEffect` mixes in that same copy at its own fixed
    /// radius — so where both are enabled the deeper of the two wins and the
    /// blur reads wider than its `Size` asked for. That costs nothing visible: a
    /// `BlurEffect` already replaces the frame outright, which leaves the depth
    /// of field mixing a blurred frame into itself.
    fn blur_levels(&self, height: u32) -> u32 {
        let blur = self
            .effects
            .blur
            .map_or(0, |blur| bloom::blur(blur.size, height).levels);
        let depth_of_field = self
            .effects
            .depth_of_field
            .map_or(0, |_| bloom::blur(DOF_BLUR_SIZE, height).levels);

        blur.max(depth_of_field)
    }

    /// The bloom intensity this level allows, 0 being "no glow at all".
    ///
    /// Roblox's own docs for `BloomEffect.Size`: "a value of 0 will disable
    /// the bleed (but not the color adjustment)" — the bleed is exactly this
    /// additive spread, so `Size <= 0` zeroes it regardless of `Intensity`.
    /// The "color adjustment" the doc leaves a residual for is not specified
    /// anywhere beyond that one clause, so it is not reproduced here.
    fn intensity(&self) -> f32 {
        if self.bloom && self.effects.bloom.size > 0.0 {
            self.effects.bloom.intensity
        } else {
            0.0
        }
    }

    fn sources(&self) -> Sources<'_> {
        Sources {
            layout: &self.sample_layout,
            sampler: &self.sampler,
            blur_layout: if self.samples > 1 {
                &self.blur_multisampled_layout
            } else {
                &self.blur_layout
            },
        }
    }
}
