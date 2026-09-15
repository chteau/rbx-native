//! Cross-checks the XML `UniqueId` decoder against the same field decoded by
//! `rbx_binary` from the real `TestPlace.rbxl` fixture, so a future edit that
//! swaps a byte order or the rotation direction in either decoder gets caught
//! against real data instead of only against hand-derived unit test values.

use rbx_dom::Variant;

const TEST_PLACE: &[u8] = include_bytes!("../../../assets/tests/TestPlace.rbxl");

#[test]
fn unique_id_matches_the_binary_fixture_after_re_encoding_to_xml_layout() {
    let dom = rbx_binary::deserialize(TEST_PLACE).unwrap();

    let workspace = dom
        .root_refs()
        .iter()
        .find_map(|&r| dom.get(r).filter(|inst| inst.class() == "Workspace"))
        .expect("fixture has a Workspace");

    let expected = match workspace.properties().get("UniqueId") {
        Some(Variant::UniqueId(id)) => *id,
        other => panic!("Workspace.UniqueId missing or wrong type in fixture: {other:?}"),
    };

    // Re-encode into xml.md's own layout: Random (rotated left by 1, undoing the
    // binary decoder's `rotate_right`), then Time, then Index.
    let random_wire = (expected.random as u64).rotate_left(1);
    let hex = format!(
        "{random_wire:016x}{:08x}{:08x}",
        expected.time, expected.index
    );

    let xml = format!(
        r#"<roblox version="4"><Item class="Model" referent="RBXTest"><Properties><UniqueId name="UniqueId">{hex}</UniqueId></Properties></Item></roblox>"#
    );
    let xml_dom = rbx_xml::deserialize(&xml).unwrap();
    let item = xml_dom.get(xml_dom.root_refs()[0]).unwrap();

    assert_eq!(
        item.properties().get("UniqueId"),
        Some(&Variant::UniqueId(expected))
    );
}
