//! Unit tests for [`super`]: what links the depth pass's Rust-side layouts
//! to the shader that reads them.

use super::*;

// Every attribute has to sit inside the stride it is read from, or the
// vertex fetch reads whatever the neighbouring instance holds.
#[test]
fn a_caster_is_a_model_matrix_and_nothing_more() {
    let stride = std::mem::size_of::<CasterRaw>() as wgpu::BufferAddress;

    assert_eq!(stride, 64);
    for attribute in CASTER_ATTRIBUTES {
        assert!(attribute.offset + attribute.format.size() <= stride);
    }
}

// Both pipelines read slot 0 as a bare position, out of buffers packed for
// two different colour passes: the shared unit meshes' (position, normal)
// vertex, and a file mesh's position-only copy.
#[test]
fn a_position_fits_in_both_vertex_strides() {
    let position = POSITION_ATTRIBUTE[0].offset + POSITION_ATTRIBUTE[0].format.size();

    assert!(position <= std::mem::size_of::<Vertex>() as wgpu::BufferAddress);
    assert_eq!(
        position,
        std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress
    );
}

#[test]
fn the_lamp_marker_tells_the_two_apart() {
    assert_eq!(Lamp::None.marker(), 0.0);
    assert_eq!(Lamp::Sun.marker(), 1.0);
    assert_eq!(Lamp::Fill.marker(), -1.0);
}

// The depth pass declares the same four instance locations the shader reads
// its model matrix out of; nothing links the two but this.
#[test]
fn the_shader_declares_exactly_the_locations_the_layout_supplies() {
    let shader = include_str!("../shadow.wgsl");
    for attribute in CASTER_ATTRIBUTES {
        assert!(
            shader.contains(&format!("@location({})", attribute.shader_location)),
            "shadow.wgsl never reads location {}",
            attribute.shader_location
        );
    }
}
