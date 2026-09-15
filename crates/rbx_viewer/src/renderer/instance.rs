//! Per-part instance data handed to the vertex shader.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::scene::{Part, Slot};

const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
    2 => Float32x4,
    3 => Float32x4,
    4 => Float32x4,
    5 => Float32x4,
    6 => Float32x4,
    7 => Float32,
    8 => Uint32,
    9 => Float32,
    10 => Uint32,
];

// Same fields as `INSTANCE_ATTRIBUTES`, shifted up two locations so this buffer
// can follow a vertex format that already occupies 0-2 (position, normal, uv)
// instead of the box's 0-1 — see `renderer::filemesh`'s textured pipeline.
const INSTANCE_ATTRIBUTES_AFTER_UV: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
    3 => Float32x4,
    4 => Float32x4,
    5 => Float32x4,
    6 => Float32x4,
    7 => Float32x4,
    8 => Float32,
    9 => Uint32,
    10 => Float32,
    11 => Uint32,
];

// Same fields again, shifted up one more location for the vertex format that
// also carries a tangent (0-3) — see `renderer::filemesh`'s appearance pipeline.
const INSTANCE_ATTRIBUTES_AFTER_TANGENT: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
    4 => Float32x4,
    5 => Float32x4,
    6 => Float32x4,
    7 => Float32x4,
    8 => Float32x4,
    9 => Float32,
    10 => Uint32,
    11 => Float32,
    12 => Uint32,
];

/// GPU-ready per-part instance data: model matrix, color with its alpha,
/// `Reflectance`, and the material the surface shaders sample.
///
/// The material triple sits exactly where the padding to 16 bytes used to, so
/// the stride still matches WGSL's default layout for `InstanceInput`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct InstanceRaw {
    model: [[f32; 4]; 4],
    color: [f32; 3],
    alpha: f32,
    reflectance: f32,
    /// Layer of every material texture array (see `renderer::material`).
    material_layer: u32,
    studs_per_tile: f32,
    /// `scene::Kind` as its discriminant; `material.wgsl` names the same values.
    material_kind: u32,
}

impl InstanceRaw {
    pub(super) fn from_part(part: &Part) -> Self {
        Self::new(
            part.transform.to_cols_array_2d(),
            part.color,
            part.alpha,
            part.reflectance,
            part.material,
        )
    }

    /// Used directly by `renderer::filemesh`, which has no `Part` — its model
    /// matrices come from resolved mesh placement instead.
    pub(super) fn new(
        model: [[f32; 4]; 4],
        color: [f32; 3],
        alpha: f32,
        reflectance: f32,
        material: Slot,
    ) -> Self {
        InstanceRaw {
            model,
            color,
            alpha,
            reflectance,
            material_layer: material.layer,
            studs_per_tile: material.studs_per_tile,
            material_kind: material.kind as u32,
        }
    }

    /// Where the instance sits in world space, which is what the translucent pass
    /// sorts on. The model matrix already folds the part's size in, so its
    /// translation column is the centre of the drawn shape.
    pub(super) fn center(&self) -> Vec3 {
        Vec3::new(self.model[3][0], self.model[3][1], self.model[3][2])
    }

    pub(super) const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES,
        }
    }

    pub(super) const fn layout_after_uv() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES_AFTER_UV,
        }
    }

    pub(super) const fn layout_after_tangent() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES_AFTER_TANGENT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::pipeline::{APPEARANCE_SHADER, BOX_SHADER, FILEMESH_SHADER};
    use super::*;
    use crate::scene::Kind;

    fn plastic() -> Slot {
        Slot {
            layer: 0,
            kind: Kind::Plastic,
            studs_per_tile: 10.0,
        }
    }

    #[test]
    fn the_material_travels_with_the_instance() {
        let material = Slot {
            layer: 3,
            kind: Kind::Neon,
            studs_per_tile: 4.0,
        };
        let instance = InstanceRaw::new([[0.0; 4]; 4], [1.0; 3], 1.0, 0.0, material);

        assert_eq!(instance.material_layer, 3);
        assert_eq!(instance.studs_per_tile, 4.0);
        assert_eq!(instance.material_kind, 2);
    }

    /// The `@location(n)` list of the `InstanceInput` struct one shader
    /// declares, in source order.
    fn declared_locations(shader: &str) -> Vec<u32> {
        let start = shader
            .find("struct InstanceInput {")
            .expect("every surface shader declares its instance input");
        let block = &shader[start..start + shader[start..].find('}').unwrap_or(0)];

        block
            .split("@location(")
            .skip(1)
            .filter_map(|rest| rest.split(')').next()?.parse().ok())
            .collect()
    }

    // Nothing links the WGSL declaration to the Rust layout, so a location added
    // to one and not the other simply reads whatever the other slot holds.
    #[test]
    fn the_shaders_declare_exactly_the_locations_the_layouts_supply() {
        let expected: Vec<u32> = INSTANCE_ATTRIBUTES
            .iter()
            .map(|attribute| attribute.shader_location)
            .collect();
        assert_eq!(declared_locations(BOX_SHADER), expected);

        let after_uv: Vec<u32> = INSTANCE_ATTRIBUTES_AFTER_UV
            .iter()
            .map(|attribute| attribute.shader_location)
            .collect();
        assert_eq!(declared_locations(FILEMESH_SHADER), after_uv);

        let after_tangent: Vec<u32> = INSTANCE_ATTRIBUTES_AFTER_TANGENT
            .iter()
            .map(|attribute| attribute.shader_location)
            .collect();
        assert_eq!(declared_locations(APPEARANCE_SHADER), after_tangent);
    }

    // Studs per tile only ever reaches the GPU, so this is the one place its
    // arrival can be checked: the projection has to divide by it, or a part's
    // texel density would follow its size instead of staying fixed.
    #[test]
    fn the_material_projection_scales_by_studs_per_tile() {
        assert!(BOX_SHADER.contains(") / max(input.studs_per_tile, 0.001)"));
    }

    // The vertex attributes address the struct by byte offset, so a field added
    // on one side and not the other reads neighbouring garbage instead of
    // failing to compile.
    #[test]
    fn every_attribute_addresses_a_field_inside_the_stride() {
        let stride = std::mem::size_of::<InstanceRaw>() as wgpu::BufferAddress;
        // 4x4 matrix, then color+alpha, then reflectance, then the material
        // triple — a multiple of 16, which is what `InstanceInput` in
        // shader.wgsl declares.
        assert_eq!(stride, 96);

        for attributes in [
            INSTANCE_ATTRIBUTES,
            INSTANCE_ATTRIBUTES_AFTER_UV,
            INSTANCE_ATTRIBUTES_AFTER_TANGENT,
        ] {
            for attribute in attributes {
                assert!(attribute.offset + attribute.format.size() <= stride);
            }
        }
        assert_eq!(INSTANCE_ATTRIBUTES[4].offset, 64);
        assert_eq!(INSTANCE_ATTRIBUTES[5].offset, 80);
        // The material triple, which both layouts address last.
        assert_eq!(INSTANCE_ATTRIBUTES[6].offset, 84);
        assert_eq!(INSTANCE_ATTRIBUTES[7].offset, 88);
        assert_eq!(INSTANCE_ATTRIBUTES[8].offset, 92);
        assert_eq!(
            INSTANCE_ATTRIBUTES_AFTER_UV[8].offset,
            INSTANCE_ATTRIBUTES[8].offset
        );
        assert_eq!(
            INSTANCE_ATTRIBUTES_AFTER_TANGENT[8].offset,
            INSTANCE_ATTRIBUTES[8].offset
        );
    }

    #[test]
    fn the_center_is_the_model_matrix_translation() {
        let mut model = [[0.0; 4]; 4];
        model[3] = [3.0, -4.0, 5.0, 1.0];

        let instance = InstanceRaw::new(model, [1.0; 3], 1.0, 0.0, plastic());

        assert_eq!(instance.center(), Vec3::new(3.0, -4.0, 5.0));
    }
}
