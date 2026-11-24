//! `MaterialVariant.AlphaMode` (Studio's "MaterialVariant Alpha Mode" beta)
//! keeps every value through both formats and back, whether or not this
//! editor has the beta on: the switch only decides how it draws.

use rbx_dom::{Variant, WeakDom};

#[test]
fn every_alpha_mode_round_trips_through_binary_and_xml() {
    // `Enum.AlphaMode`: Overlay 0, Transparency 1, TintMask 2, Opaque 3.
    for mode in 0..=3 {
        let mut dom = WeakDom::new();
        let service = dom.new_instance("MaterialService", "MaterialService", None);
        let variant = dom.new_instance("MaterialVariant", "Painted", Some(service));
        dom.set_property(variant, "AlphaMode", Variant::Enum(mode))
            .unwrap();
        dom.set_property(variant, "BaseMaterial", Variant::Enum(256))
            .unwrap();

        let binary = rbx_binary::deserialize(&rbx_binary::serialize(&dom).unwrap()).unwrap();
        let xml = rbx_xml::deserialize(&rbx_xml::serialize(&dom).unwrap()).unwrap();
        let crossed = rbx_xml::deserialize(&rbx_xml::serialize(&binary).unwrap()).unwrap();

        for (format, dom) in [("binary", &binary), ("xml", &xml), ("crossed", &crossed)] {
            let service = dom.get(dom.root_refs()[0]).unwrap();
            let back = dom.get(service.children()[0]).unwrap();
            assert_eq!(back.class(), "MaterialVariant", "{format}");
            assert_eq!(
                back.properties().get("AlphaMode"),
                Some(&Variant::Enum(mode)),
                "{format}: mode {mode}"
            );
        }
    }
}
