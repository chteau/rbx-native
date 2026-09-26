//! The opaque scene's depth, readable by the blended surfaces drawn over it:
//! what a `ForceField` measures how close it is to cutting through
//! something against (see `material.wgsl`'s `force_field_intersection`).
//!
//! Group 3 of the blended surface pipelines only. The pass those draw in
//! holds the depth buffer as a *read-only* attachment (see
//! `Renderer::translucent_pass`), which is what lets the same texture be
//! bound here in the same pass. Every other surface pipeline is built with
//! [`NONE`] in its place, so its shader still links against `scene_depth`
//! and simply never finds anything there.

/// A stand-in with nothing behind it: depth 0 is the cleared far plane under
/// reversed-Z, so everything measures as infinitely far from the scene.
const NONE: &str = "fn scene_depth(texel: vec2<i32>) -> f32 { return 0.0; }\n";

const SINGLE: &str = "@group(3) @binding(0) var scene_depth_texture: texture_depth_2d;
fn scene_depth(texel: vec2<i32>) -> f32 { return textureLoad(scene_depth_texture, texel, 0); }
";

// Sample 0 of the pixel rather than a resolve: a glow about a stud wide
// has nothing to gain from the other samples, and a depth buffer has no
// meaningful average anyway.
const MULTISAMPLED: &str =
    "@group(3) @binding(0) var scene_depth_texture: texture_depth_multisampled_2d;
fn scene_depth(texel: vec2<i32>) -> f32 { return textureLoad(scene_depth_texture, texel, 0); }
";

/// `shader` with the `scene_depth` it calls defined in front of it. In front
/// is enough: WGSL resolves module-scope names regardless of order.
pub(super) fn with_source(shader: &str, reads: bool, samples: u32) -> String {
    let source = match (reads, samples > 1) {
        (false, _) => NONE,
        (true, false) => SINGLE,
        (true, true) => MULTISAMPLED,
    };
    format!("{source}{shader}")
}

/// The layout [`bind`] builds against and the blended pipelines are built
/// with — structurally equal wherever it is created, which is all WebGPU asks
/// of a bind group and the pipeline it is used with.
pub(super) fn layout(device: &wgpu::Device, samples: u32) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rbxview scene depth"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: samples > 1,
            },
            count: None,
        }],
    })
}

pub(super) fn bind(
    device: &wgpu::Device,
    depth: &wgpu::TextureView,
    samples: u32,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rbxview scene depth"),
        layout: &layout(device, samples),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(depth),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_reading_pipeline_declares_the_binding_and_its_shape_follows_msaa() {
        assert!(!with_source("", false, 4).contains("@group(3)"));
        assert!(with_source("", true, 1).contains("texture_depth_2d"));
        assert!(with_source("", true, 4).contains("texture_depth_multisampled_2d"));
        // The shader itself follows, untouched.
        assert!(with_source("fn fs_main() {}", true, 1).ends_with("fn fs_main() {}"));
    }
}
