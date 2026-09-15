//! The three ways a file mesh can be skinned, and the pipeline set that draws
//! them. Each blend mode owns one such set, so a translucent mesh differs from
//! an opaque one only by its blend state.

use super::super::instance::InstanceRaw;
use super::super::mesh::Vertex as BoxVertex;
use super::super::pipeline::{
    self, Surface, Target, APPEARANCE_SHADER, BOX_SHADER, FILEMESH_SHADER,
};
use super::vertex::{AppearanceVertex, TexturedVertex};

/// What a batch samples, and therefore which pipeline and bind group 2 it needs.
#[derive(Clone, Copy)]
pub(super) enum Skin {
    /// The part's colour alone, shaded by its `BasePart.Material`.
    Plain,
    /// The mesh's own `TextureID`, as an index into the uploaded images.
    Image(usize),
    /// A `SurfaceAppearance` map set, as an index into the uploaded sets.
    Appearance(usize),
}

pub(super) struct Pipelines {
    plain: wgpu::RenderPipeline,
    textured: wgpu::RenderPipeline,
    appearance: wgpu::RenderPipeline,
}

impl Pipelines {
    pub(super) fn get(&self, skin: Skin) -> &wgpu::RenderPipeline {
        match skin {
            Skin::Plain => &self.plain,
            Skin::Image(_) => &self.textured,
            Skin::Appearance(_) => &self.appearance,
        }
    }
}

/// Builds one set. `layouts` is the whole bind group table — frame, the
/// textured pass's image, materials, then the appearance set — from which each
/// variant keeps the groups it actually binds.
pub(super) fn build(
    device: &wgpu::Device,
    target: Target,
    layouts: &Layouts<'_>,
    translucent: bool,
) -> Pipelines {
    // Each variant's own image is the *last* group, above the frame and
    // materials every surface shader shares: the plain layout is then a prefix
    // of the other two, so a batch drawn right after one of a different skin
    // never has a stale, incompatible set sitting under a group it reads —
    // see `pipeline::shape_pipelines`.
    let plain_layouts = [Some(layouts.frame), Some(layouts.materials)];
    let textured_layouts = [
        Some(layouts.frame),
        Some(layouts.materials),
        Some(layouts.image),
    ];
    let appearance_layouts = [
        Some(layouts.frame),
        Some(layouts.materials),
        Some(layouts.appearance),
    ];

    let plain_buffers = [Some(BoxVertex::layout()), Some(InstanceRaw::layout())];
    let textured_buffers = [
        Some(TexturedVertex::layout()),
        Some(InstanceRaw::layout_after_uv()),
    ];
    let appearance_buffers = [
        Some(AppearanceVertex::layout()),
        Some(InstanceRaw::layout_after_tangent()),
    ];

    let variant = |label, shader, layouts: &[Option<&wgpu::BindGroupLayout>], buffers: &[_]| {
        pipeline::surface(
            device,
            target,
            &Surface {
                // No culling: see the module doc for why downloaded meshes keep
                // both faces.
                cull: None,
                translucent,
                ..Surface::new(label, shader, layouts, buffers)
            },
        )
    };

    Pipelines {
        plain: variant(
            "rbxview filemesh (untextured)",
            BOX_SHADER,
            &plain_layouts,
            &plain_buffers,
        ),
        textured: variant(
            "rbxview filemesh (textured)",
            FILEMESH_SHADER,
            &textured_layouts,
            &textured_buffers,
        ),
        appearance: variant(
            "rbxview filemesh (surface appearance)",
            APPEARANCE_SHADER,
            &appearance_layouts,
            &appearance_buffers,
        ),
    }
}

/// The bind group layouts the three variants draw from.
pub(super) struct Layouts<'a> {
    pub(super) frame: &'a wgpu::BindGroupLayout,
    pub(super) image: &'a wgpu::BindGroupLayout,
    pub(super) materials: &'a wgpu::BindGroupLayout,
    pub(super) appearance: &'a wgpu::BindGroupLayout,
}
