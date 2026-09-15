//! Encoders for bitfield and multi-field flag types: Axes, Faces,
//! PhysicalProperties, and SecurityCapabilities. Mirrors `value::flags`.

use rbx_dom::{Axes, Faces, PhysicalProperties};

use crate::serializer::writer::Writer;

pub(crate) fn axes(writer: &mut Writer, name: &str, value: &Axes) {
    let bits = u8::from(value.x) | (u8::from(value.y) << 1) | (u8::from(value.z) << 2);
    writer.open("Axes", &[("name", name)]);
    writer.leaf("axes", &[], &bits.to_string());
    writer.close("Axes");
}

// xml.md packs the low 6 bits in the opposite order from `Faces`'s own field
// order; `reverse_low6` is its own inverse, so the same bit-reversal that
// decodes the wire byte also re-encodes it. Duplicated from the reader's
// private helper rather than shared, since that one is not part of this
// crate's internal API surface.
fn reverse_low6(bits: u8) -> u8 {
    let mut out = 0u8;
    for i in 0..6 {
        if bits & (1 << i) != 0 {
            out |= 1 << (5 - i);
        }
    }
    out
}

pub(crate) fn faces(writer: &mut Writer, name: &str, value: &Faces) {
    let logical = u8::from(value.front)
        | (u8::from(value.bottom) << 1)
        | (u8::from(value.left) << 2)
        | (u8::from(value.back) << 3)
        | (u8::from(value.top) << 4)
        | (u8::from(value.right) << 5);
    writer.open("Faces", &[("name", name)]);
    writer.leaf("faces", &[], &reverse_low6(logical).to_string());
    writer.close("Faces");
}

pub(crate) fn physical_properties(writer: &mut Writer, name: &str, value: &PhysicalProperties) {
    writer.open("PhysicalProperties", &[("name", name)]);
    match value {
        PhysicalProperties::Default => writer.leaf("CustomPhysics", &[], "false"),
        PhysicalProperties::Custom {
            density,
            friction,
            elasticity,
            friction_weight,
            elasticity_weight,
        } => {
            writer.leaf("CustomPhysics", &[], "true");
            writer.leaf("Density", &[], &density.to_string());
            writer.leaf("Friction", &[], &friction.to_string());
            writer.leaf("Elasticity", &[], &elasticity.to_string());
            writer.leaf("FrictionWeight", &[], &friction_weight.to_string());
            writer.leaf("ElasticityWeight", &[], &elasticity_weight.to_string());
        }
    }
    writer.close("PhysicalProperties");
}

pub(crate) fn security_capabilities(writer: &mut Writer, name: &str, value: u64) {
    writer.leaf(
        "SecurityCapabilities",
        &[("name", name)],
        &value.to_string(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_bit_order_matches_the_spec_example() {
        let mut writer = Writer::new();
        faces(
            &mut writer,
            "X",
            &Faces {
                front: true,
                bottom: false,
                left: true,
                back: false,
                top: true,
                right: false,
            },
        );
        assert!(writer.into_string().contains("<faces>42</faces>"));
    }

    #[test]
    fn axes_bit_order_matches_the_spec_example() {
        let mut writer = Writer::new();
        axes(
            &mut writer,
            "X",
            &Axes {
                x: true,
                y: false,
                z: false,
            },
        );
        assert!(writer.into_string().contains("<axes>1</axes>"));
    }
}
