//! Decoders for UDim and UDim2.

use rbx_dom::{UDim, UDim2, Variant};

use crate::xml_tree::Node;

pub(crate) fn udim(node: &Node) -> Variant {
    Variant::UDim(read_udim(node, "S", "O"))
}

pub(crate) fn udim2(node: &Node) -> Variant {
    Variant::UDim2(UDim2 {
        x: read_udim(node, "XS", "XO"),
        y: read_udim(node, "YS", "YO"),
    })
}

fn read_udim(node: &Node, scale_tag: &str, offset_tag: &str) -> UDim {
    UDim {
        scale: child(node, scale_tag),
        offset: child(node, offset_tag),
    }
}

fn child<T: std::str::FromStr + Default>(node: &Node, tag: &str) -> T {
    node.child(tag)
        .and_then(|c| c.text_trim().parse().ok())
        .unwrap_or_default()
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

    #[test]
    fn udim_reads_scale_and_offset() {
        let node = Node {
            tag: "UDim".into(),
            children: vec![leaf("S", "0.15625"), leaf("O", "1337")],
            ..Node::default()
        };
        assert_eq!(
            udim(&node),
            Variant::UDim(UDim {
                scale: 0.15625,
                offset: 1337
            })
        );
    }

    #[test]
    fn udim2_reads_both_components() {
        let node = Node {
            tag: "UDim2".into(),
            children: vec![
                leaf("XS", "0.15625"),
                leaf("XO", "1337"),
                leaf("YS", "-123"),
                leaf("YO", "456"),
            ],
            ..Node::default()
        };
        assert_eq!(
            udim2(&node),
            Variant::UDim2(UDim2 {
                x: UDim {
                    scale: 0.15625,
                    offset: 1337
                },
                y: UDim {
                    scale: -123.0,
                    offset: 456
                },
            })
        );
    }
}
