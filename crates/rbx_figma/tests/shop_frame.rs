//! Inference on a real frame: FigBloxUI's `Shop_Frame`, as the importer
//! dumped it (`fixtures/shop_frame`). Offline; nothing is fetched.

use rbx_dom::Variant;
use rbx_figma::infer::{infer, Image, Node};

fn shop() -> Node {
    let text = include_str!("fixtures/shop_frame/node.json");
    let root: serde_json::Value = serde_json::from_str(text).unwrap();
    let tree = infer(&root).unwrap();
    if std::env::var_os("FIGMA_OUTLINE").is_some() {
        fn show(n: &Node, d: usize) {
            println!("{}{} {:?} {:?}", "  ".repeat(d), n.class, n.name, n.image);
            for (k, v) in &n.properties {
                println!("{}  .{k} = {v:?}", "  ".repeat(d));
            }
            for c in &n.children {
                show(c, d + 1);
            }
        }
        show(&tree, 0);
    }
    tree
}

fn all(node: &Node) -> Vec<&Node> {
    let mut out = vec![node];
    for child in &node.children {
        out.extend(all(child));
    }
    out
}

fn named<'a>(tree: &'a Node, name: &str) -> Vec<&'a Node> {
    all(tree).into_iter().filter(|n| n.name == name).collect()
}

#[test]
fn the_shop_frame_keeps_its_look() {
    let tree = shop();
    let flag = |n: &Node, p: &str| n.get(p) == Some(&Variant::Bool(true));
    assert!(flag(&tree, "ClipsDescendants"), "the frame clips");

    // SERASHOP: gradient text.
    let [title] = named(&tree, "BOUTIQUE_TextLabel")[..] else {
        panic!("one title")
    };
    assert!(title.children.iter().any(|c| c.class == "UIGradient"));

    let cards = named(&tree, "Shop__Item_Frame");
    assert!(cards.len() >= 4, "{} cards", cards.len());
    for card in cards {
        assert!(flag(card, "ClipsDescendants"), "a card clips");
        // The checkered PATTERN fill: frame 2:2 rendered and tiled.
        let layer = card
            .children
            .iter()
            .find(|c| c.name == "Shop__Item_FramePattern")
            .expect("a pattern layer");
        assert_eq!(layer.class, "ImageLabel");
        assert_eq!(layer.image, Some(Image::Render("2:2".into())));
        assert!(layer.tile.is_some());
        // The spring leans left: Figma's `rotation` is clockwise already.
        let spring = card.children.iter().find(|c| c.name == "image 1").unwrap();
        let Some(Variant::Float32(turn)) = spring.get("Rotation") else {
            panic!()
        };
        assert!((turn + 75.0).abs() < 0.01, "{turn}");
        for name in ["Name_TextLabel", "Price_TextLabel"] {
            let label = card.children.iter().find(|c| c.name == name).expect(name);
            assert!(flag(label, "TextScaled"), "{name} scales");
            assert_eq!(label.get("TextTransparency"), Some(&Variant::Float32(0.0)));
            assert_ne!(label.get("Visible"), Some(&Variant::Bool(false)));
        }
    }
}
