//! Decoders for vector/geometric leaf types: Vector2, Vector3, Vector3int16,
//! Color3, Color3uint8, Rect2D, Ray.

use rbx_dom::{Color3Data, Rect, Variant, Vector2Data, Vector3Data};

use super::scalar::parse_f32;
use crate::xml_tree::Node;

pub(crate) fn vector2(node: &Node) -> Vector2Data {
    Vector2Data {
        x: child_f32(node, "X"),
        y: child_f32(node, "Y"),
    }
}

pub(crate) fn vector3(node: &Node) -> Vector3Data {
    Vector3Data {
        x: child_f32(node, "X"),
        y: child_f32(node, "Y"),
        z: child_f32(node, "Z"),
    }
}

pub(crate) fn vector2_value(node: &Node) -> Variant {
    Variant::Vector2(vector2(node))
}

pub(crate) fn vector3_value(node: &Node) -> Variant {
    Variant::Vector3(vector3(node))
}

pub(crate) fn vector3int16(node: &Node) -> Variant {
    Variant::Vector3int16 {
        x: child_i16(node, "X"),
        y: child_i16(node, "Y"),
        z: child_i16(node, "Z"),
    }
}

pub(crate) fn color3(node: &Node) -> Variant {
    Variant::Color3(Color3Data {
        r: child_f32(node, "R"),
        g: child_f32(node, "G"),
        b: child_f32(node, "B"),
    })
}

// Packed 0xAARRGGBB; the upper byte (alpha) is ignored on read, matching xml.md's
// note that it merely SHOULD be 0xFF and carries no meaning of its own.
pub(crate) fn color3_uint8(text: &str) -> Variant {
    let packed: u32 = text.trim().parse().unwrap_or(0);
    Variant::Color3uint8 {
        r: ((packed >> 16) & 0xFF) as u8,
        g: ((packed >> 8) & 0xFF) as u8,
        b: (packed & 0xFF) as u8,
    }
}

pub(crate) fn rect2d(node: &Node) -> Variant {
    let zero = Vector2Data { x: 0.0, y: 0.0 };
    let min = node.child("min").map(vector2).unwrap_or(zero);
    let max = node.child("max").map(vector2).unwrap_or(zero);
    Variant::Rect(Rect { min, max })
}

pub(crate) fn ray(node: &Node) -> Variant {
    let zero = Vector3Data {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let origin = node.child("origin").map(vector3).unwrap_or(zero);
    let direction = node.child("direction").map(vector3).unwrap_or(zero);
    Variant::Ray { origin, direction }
}

fn child_f32(node: &Node, tag: &str) -> f32 {
    node.child(tag)
        .map(|c| parse_f32(c.text_trim()))
        .unwrap_or(0.0)
}

fn child_i16(node: &Node, tag: &str) -> i16 {
    node.child(tag)
        .and_then(|c| c.text_trim().parse().ok())
        .unwrap_or(0)
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
    fn vector3_reads_xyz_children() {
        let node = parent(
            "Vector3",
            vec![leaf("X", "-INF"), leaf("Y", "0.15625"), leaf("Z", "-1337")],
        );
        assert_eq!(
            vector3_value(&node),
            Variant::Vector3(Vector3Data {
                x: f32::NEG_INFINITY,
                y: 0.15625,
                z: -1337.0
            })
        );
    }

    #[test]
    fn color3_uint8_unpacks_argb() {
        // 96, 64, 32 with alpha FF, the exact worked example from xml.md.
        assert_eq!(
            color3_uint8("4284497952"),
            Variant::Color3uint8 {
                r: 96,
                g: 64,
                b: 32
            }
        );
    }

    #[test]
    fn ray_reads_origin_and_direction() {
        let node = parent(
            "Ray",
            vec![
                parent(
                    "origin",
                    vec![leaf("X", "1"), leaf("Y", "2"), leaf("Z", "3")],
                ),
                parent(
                    "direction",
                    vec![leaf("X", "-1"), leaf("Y", "-2"), leaf("Z", "-3")],
                ),
            ],
        );
        assert_eq!(
            ray(&node),
            Variant::Ray {
                origin: Vector3Data {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0
                },
                direction: Vector3Data {
                    x: -1.0,
                    y: -2.0,
                    z: -3.0
                },
            }
        );
    }
}
