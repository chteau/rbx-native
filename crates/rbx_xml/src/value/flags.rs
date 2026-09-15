//! Decoders for bitfield and multi-field flag types: Axes, Faces,
//! PhysicalProperties, and the (xml.md-undocumented) SecurityCapabilities.

use rbx_dom::{Axes, Faces, PhysicalProperties, Variant};

use crate::xml_tree::Node;

pub(crate) fn axes(node: &Node) -> Variant {
    let bits = child_u8(node, "axes");
    // xml.md's own bit order already matches `Axes::from_bits` (X = bit 0, Y = bit
    // 1, Z = bit 2); verified against the spec's worked example (X alone -> 1).
    Variant::Axes(Axes::from_bits(bits))
}

pub(crate) fn faces(node: &Node) -> Variant {
    let bits = child_u8(node, "faces");
    // xml.md packs the low 6 bits in the *opposite* order from the binary
    // format's `Faces::from_bits` (Right..Front low-to-high here, vs Front..Right
    // there); reversing the 6-bit field before decoding makes the two agree.
    // Verified against the spec's worked example (42 -> Front, Left, Top).
    Variant::Faces(Faces::from_bits(reverse_low6(bits)))
}

fn reverse_low6(bits: u8) -> u8 {
    let mut out = 0u8;
    for i in 0..6 {
        if bits & (1 << i) != 0 {
            out |= 1 << (5 - i);
        }
    }
    out
}

fn child_u8(node: &Node, tag: &str) -> u8 {
    node.child(tag)
        .and_then(|c| c.text_trim().parse().ok())
        .unwrap_or(0)
}

pub(crate) fn physical_properties(node: &Node) -> Variant {
    let is_custom = node
        .child("CustomPhysics")
        .map(|c| c.text_trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !is_custom {
        return Variant::PhysicalProperties(PhysicalProperties::Default);
    }

    let f = |tag: &str| -> f32 {
        node.child(tag)
            .and_then(|c| c.text_trim().parse().ok())
            .unwrap_or(0.0)
    };
    // `AcousticAbsorption` has no field on this crate's `PhysicalProperties` (the
    // same gap the binary decoder has); it is simply not read.
    Variant::PhysicalProperties(PhysicalProperties::Custom {
        density: f("Density"),
        friction: f("Friction"),
        elasticity: f("Elasticity"),
        friction_weight: f("FrictionWeight"),
        elasticity_weight: f("ElasticityWeight"),
    })
}

// Not documented in xml.md at the time of writing; inferred by analogy with the
// other plain-integer leaf types (`int64`/`token`), since like them it is a
// single opaque numeric bitfield with no sub-structure to encode.
pub(crate) fn security_capabilities(text: &str) -> Variant {
    Variant::SecurityCapabilities(text.trim().parse().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(tag: &str, text: &str) -> Node {
        Node {
            tag: tag.into(),
            text: text.into(),
            ..Node::default()
        }
    }

    fn parent(tag: &str, children: Vec<Node>) -> Node {
        Node {
            tag: tag.into(),
            children,
            ..Node::default()
        }
    }

    #[test]
    fn faces_bit_order_matches_spec_example() {
        // Front, Left, and Top enabled, per xml.md's own worked example.
        let node = parent("Faces", vec![leaf("faces", "42")]);
        assert_eq!(
            faces(&node),
            Variant::Faces(Faces {
                front: true,
                bottom: false,
                left: true,
                back: false,
                top: true,
                right: false
            })
        );
    }

    #[test]
    fn axes_bit_order_matches_spec_example() {
        // Only X enabled, per xml.md's own worked example.
        let node = parent("Axes", vec![leaf("axes", "1")]);
        assert_eq!(
            axes(&node),
            Variant::Axes(Axes {
                x: true,
                y: false,
                z: false
            })
        );
    }

    #[test]
    fn physical_properties_reads_the_spec_example() {
        let node = parent(
            "PhysicalProperties",
            vec![
                leaf("CustomPhysics", "true"),
                leaf("Density", "1"),
                leaf("Friction", "2"),
                leaf("Elasticity", "1"),
                leaf("FrictionWeight", "0.15625"),
                leaf("ElasticityWeight", "1.25"),
                leaf("AcousticAbsorption", "1"),
            ],
        );
        assert_eq!(
            physical_properties(&node),
            Variant::PhysicalProperties(PhysicalProperties::Custom {
                density: 1.0,
                friction: 2.0,
                elasticity: 1.0,
                friction_weight: 0.15625,
                elasticity_weight: 1.25,
            })
        );
    }

    #[test]
    fn physical_properties_non_custom_is_default() {
        let node = parent("PhysicalProperties", vec![leaf("CustomPhysics", "false")]);
        assert_eq!(
            physical_properties(&node),
            Variant::PhysicalProperties(PhysicalProperties::Default)
        );
    }
}
