//! A `UIShadow` keeps every property it saves through both formats and back,
//! one property of each value type its class uses (`UDim`, `UDim2`,
//! `Color3`, `Enum`, `float`, `int`, `bool`).

use rbx_dom::{Color3Data, Ref, UDim, UDim2, Variant, WeakDom};

fn shadow() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let frame = dom.new_instance("Frame", "Card", None);
    let shadow = dom.new_instance("UIShadow", "UIShadow", Some(frame));
    let udim2 = |x: (f32, i32), y: (f32, i32)| {
        Variant::UDim2(UDim2 {
            x: UDim {
                scale: x.0,
                offset: x.1,
            },
            y: UDim {
                scale: y.0,
                offset: y.1,
            },
        })
    };
    let properties = [
        (
            "BlurRadius",
            Variant::UDim(UDim {
                scale: 0.05,
                offset: 12,
            }),
        ),
        (
            "Color",
            Variant::Color3(Color3Data {
                r: 0.1,
                g: 0.2,
                b: 0.3,
            }),
        ),
        ("Enabled", Variant::Bool(true)),
        ("Inset", Variant::Bool(false)),
        ("Mode", Variant::Enum(1)),
        ("Offset", udim2((0.0, 4), (0.1, 8))),
        ("ShowBehindParent", Variant::Bool(true)),
        ("Spread", udim2((0.0, -2), (0.0, 6))),
        ("Transparency", Variant::Float32(0.4)),
        ("ZIndex", Variant::Int32(-3)),
    ];
    for (name, value) in properties {
        dom.set_property(shadow, name, value).unwrap();
    }
    (dom, shadow)
}

fn the_shadow(dom: &WeakDom) -> &rbx_dom::Instance {
    let frame = dom.get(dom.root_refs()[0]).unwrap();
    dom.get(frame.children()[0]).unwrap()
}

#[test]
fn a_ui_shadow_round_trips_through_binary_and_xml() {
    let (dom, shadow) = shadow();
    let expected = dom.get(shadow).unwrap();

    let binary = rbx_binary::deserialize(&rbx_binary::serialize(&dom).unwrap()).unwrap();
    let xml = rbx_xml::deserialize(&rbx_xml::serialize(&dom).unwrap()).unwrap();
    // And across: a place saved as one and opened as the other.
    let crossed = rbx_xml::deserialize(&rbx_xml::serialize(&binary).unwrap()).unwrap();

    for (format, dom) in [("binary", &binary), ("xml", &xml), ("crossed", &crossed)] {
        let back = the_shadow(dom);
        assert_eq!(back.class(), "UIShadow", "{format}");
        for (name, value) in expected.properties() {
            assert_eq!(back.properties().get(name), Some(value), "{format}: {name}");
        }
    }
}
