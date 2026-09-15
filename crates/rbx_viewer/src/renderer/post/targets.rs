//! The attachments one frame size needs: the HDR target the scene resolves into,
//! its depth buffer, the multisampled pair in front of both where the quality
//! level asks for MSAA, and the bloom and full-frame blur chains hanging off
//! the result.

use super::super::pipeline::DEPTH_FORMAT;
use super::pipelines::attachment;
use super::{FIRST_LEVEL_DIVISOR, HDR_FORMAT};

/// What every bind group in here is built from; all three live on [`super::Post`]
/// for the whole run, while the views they point at are rebuilt per frame size.
#[derive(Clone, Copy)]
pub(super) struct Sources<'a> {
    pub(super) layout: &'a wgpu::BindGroupLayout,
    pub(super) sampler: &'a wgpu::Sampler,
    /// Whichever of [`super::Post`]'s two depth layouts matches the sample count
    /// these targets are being built at — the depth buffer below is multisampled
    /// with the scene, and WGSL types the two shapes differently.
    /// Whichever of [`super::Post`]'s two blur layouts matches the sample count
    /// these targets are being built at: it carries the scene's depth buffer
    /// alongside the blurred frame, and WGSL types a multisampled depth texture
    /// differently from a single-sampled one.
    pub(super) blur_layout: &'a wgpu::BindGroupLayout,
}

/// One level of a downsample chain (bloom's or the full-frame blur's): its own
/// view, and the bind group that samples it as a source.
pub(super) struct Level {
    pub(super) view: wgpu::TextureView,
    pub(super) source: wgpu::BindGroup,
}

pub(in crate::renderer) struct Targets {
    size: (u32, u32),
    samples: u32,
    depth: wgpu::TextureView,
    /// Single-sampled, and the only colour target the post chain ever samples: it
    /// is either what the scene pass draws into or what it resolves into.
    scene: wgpu::TextureView,
    /// Present only above one sample: the target the scene pass actually draws
    /// into, resolved into `scene` by the pass's own resolve attachment rather
    /// than by a pass of ours.
    multisampled: Option<wgpu::TextureView>,
    pub(super) scene_source: wgpu::BindGroup,
    /// The resolve's own third group: the blurred copy of the frame
    /// (`blur_chain`'s deepest level, or the sharp scene where there is no chain
    /// to build) next to `depth` bound for reading rather than for writing, which
    /// is what `DepthOfFieldEffect` reconstructs a per-pixel distance from.
    ///
    /// Safe to bind the depth buffer here because the scene pass, the only thing
    /// that writes it, has already ended by the time the resolve runs; the two
    /// share one group because a WebGPU device is only guaranteed four of them.
    pub(super) blur_depth: wgpu::BindGroup,
    pub(super) chain: Vec<Level>,
    /// The full-frame blur's own chain, sized independently of `chain` since a
    /// `BlurEffect.Size` and a `BloomEffect.Size` need not agree. Empty where the
    /// place has neither an enabled `BlurEffect` nor an enabled
    /// `DepthOfFieldEffect` — the two share this one blurred copy of the frame.
    pub(super) blur_chain: Vec<Level>,
}

impl Targets {
    pub(super) fn new(
        device: &wgpu::Device,
        sources: Sources<'_>,
        size: (u32, u32),
        levels: u32,
        blur_levels: u32,
        samples: u32,
    ) -> Self {
        let scene = attachment(device, "rbxview scene", size, HDR_FORMAT, 1);
        let depth = attachment(device, "rbxview depth", size, DEPTH_FORMAT, samples);
        let blur_chain = build_chain(device, sources, size, blur_levels, "rbxview blur");

        Targets {
            size,
            samples,
            // The deepest level is the blurriest, and the resolve's own sampler
            // stretches it back over the frame (see `Post::resolve`). Where there
            // is no chain the sharp scene stands in: neither effect that reads it
            // is enabled, and the group still needs a real view.
            blur_depth: bind_blur(
                device,
                sources,
                blur_chain.last().map_or(&scene, |level| &level.view),
                &depth,
            ),
            depth,
            multisampled: (samples > 1)
                .then(|| attachment(device, "rbxview scene (msaa)", size, HDR_FORMAT, samples)),
            scene_source: bind(device, sources, &scene),
            chain: build_chain(device, sources, size, levels, "rbxview bloom"),
            blur_chain,
            scene,
        }
    }

    /// Whether these targets are still the ones the frame wants.
    pub(super) fn fit(
        &self,
        size: (u32, u32),
        levels: u32,
        blur_levels: u32,
        samples: u32,
    ) -> bool {
        self.size == size
            && self.chain.len() == levels as usize
            && self.blur_chain.len() == blur_levels as usize
            && self.samples == samples
    }

    /// The scene pass's colour attachment and, above one sample, the target it
    /// resolves into.
    pub(in crate::renderer) fn color(&self) -> (&wgpu::TextureView, Option<&wgpu::TextureView>) {
        match &self.multisampled {
            Some(multisampled) => (multisampled, Some(&self.scene)),
            None => (&self.scene, None),
        }
    }

    pub(in crate::renderer) fn depth(&self) -> &wgpu::TextureView {
        &self.depth
    }
}

/// A downsample chain of `levels` deep, each level half the size of the one
/// before it, starting at half `size`. Shared by bloom's own chain and the
/// full-frame blur's, which differ only in how many levels they ask for.
fn build_chain(
    device: &wgpu::Device,
    sources: Sources<'_>,
    size: (u32, u32),
    levels: u32,
    label: &str,
) -> Vec<Level> {
    let mut chain = Vec::with_capacity(levels as usize);
    let mut level_size = size;
    for _ in 0..levels {
        level_size = (
            (level_size.0 / FIRST_LEVEL_DIVISOR).max(1),
            (level_size.1 / FIRST_LEVEL_DIVISOR).max(1),
        );
        let view = attachment(device, label, level_size, HDR_FORMAT, 1);
        chain.push(Level {
            source: bind(device, sources, &view),
            view,
        });
    }
    chain
}

fn bind_blur(
    device: &wgpu::Device,
    sources: Sources<'_>,
    blurred: &wgpu::TextureView,
    depth: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rbxview post blur and depth"),
        layout: sources.blur_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(blurred),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sources.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(depth),
            },
        ],
    })
}

fn bind(device: &wgpu::Device, sources: Sources<'_>, view: &wgpu::TextureView) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rbxview post source"),
        layout: sources.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sources.sampler),
            },
        ],
    })
}
