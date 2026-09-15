//! Decoders for CFrame-shaped types: CoordinateFrame and its Optional wrapper.

use rbx_dom::{CFrameData, Variant, Vector3Data};

use super::scalar::parse_f32;
use crate::xml_tree::Node;

pub(crate) fn cframe(node: &Node) -> CFrameData {
    let f = |tag: &str| {
        node.child(tag)
            .map(|c| parse_f32(c.text_trim()))
            .unwrap_or(0.0)
    };
    CFrameData {
        position: Vector3Data {
            x: f("X"),
            y: f("Y"),
            z: f("Z"),
        },
        // Row-major, same field order xml.md documents: R00, R01, R02, R10, ...
        rotation: [
            f("R00"),
            f("R01"),
            f("R02"),
            f("R10"),
            f("R11"),
            f("R12"),
            f("R20"),
            f("R21"),
            f("R22"),
        ],
    }
}

pub(crate) fn cframe_value(node: &Node) -> Variant {
    Variant::CFrame(cframe(node))
}

// `OptionalCoordinateFrame` wraps a nested `CFrame` element (not `CoordinateFrame`
// again, per xml.md's own worked example) that is present only when the value is.
pub(crate) fn optional_cframe(node: &Node) -> Variant {
    Variant::OptionalCFrame(node.child("CFrame").map(cframe))
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

    fn identity_children() -> Vec<Node> {
        vec![
            leaf("X", "0"),
            leaf("Y", "0"),
            leaf("Z", "0"),
            leaf("R00", "1"),
            leaf("R01", "0"),
            leaf("R02", "0"),
            leaf("R10", "0"),
            leaf("R11", "1"),
            leaf("R12", "0"),
            leaf("R20", "0"),
            leaf("R21", "0"),
            leaf("R22", "1"),
        ]
    }

    #[test]
    fn cframe_reads_identity_from_spec_example() {
        let node = Node {
            tag: "CoordinateFrame".into(),
            children: identity_children(),
            ..Node::default()
        };
        assert_eq!(
            cframe(&node),
            CFrameData {
                position: Vector3Data {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
            }
        );
    }

    #[test]
    fn optional_cframe_absent_child_is_none() {
        let node = Node {
            tag: "OptionalCoordinateFrame".into(),
            ..Node::default()
        };
        assert_eq!(optional_cframe(&node), Variant::OptionalCFrame(None));
    }

    #[test]
    fn optional_cframe_present_child_is_some() {
        let inner = Node {
            tag: "CFrame".into(),
            children: identity_children(),
            ..Node::default()
        };
        let node = Node {
            tag: "OptionalCoordinateFrame".into(),
            children: vec![inner],
            ..Node::default()
        };
        assert!(matches!(
            optional_cframe(&node),
            Variant::OptionalCFrame(Some(_))
        ));
    }
}
