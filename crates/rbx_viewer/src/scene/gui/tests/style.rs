//! `StyleSheet`/`StyleRule`/`StyleLink`: which instances a rule reaches, which
//! rule wins where two of them collide, and what a `$Token` resolves to.

use super::*;
use crate::scene::gui::Element;

/// Pure red and pure blue: both channels survive the sRGB linearization
/// [`super::super::plan`] applies, so a test can assert on them exactly.
const RED: [f32; 3] = [1.0, 0.0, 0.0];
const BLUE: [f32; 3] = [0.0, 0.0, 1.0];

fn colour(rgb: [f32; 3]) -> Variant {
    Variant::Color3(Color3Data {
        r: rgb[0],
        g: rgb[1],
        b: rgb[2],
    })
}

/// The attribute blob `StyleRule.PropertiesSerialize` and
/// `Instance.AttributesSerialize` both hold — see `rbx_dom::attributes`, which
/// reads it back.
fn blob(entries: &[(&str, Variant)]) -> Variant {
    let mut bytes = (entries.len() as u32).to_le_bytes().to_vec();
    for (name, value) in entries {
        bytes.extend((name.len() as u32).to_le_bytes());
        bytes.extend(name.as_bytes());
        match value {
            Variant::Int32(value) => {
                bytes.push(0x04);
                bytes.extend(value.to_le_bytes());
            }
            Variant::Color3(value) => {
                bytes.push(0x0f);
                for channel in [value.r, value.g, value.b] {
                    bytes.extend(channel.to_le_bytes());
                }
            }
            Variant::UDim(value) => {
                bytes.push(0x09);
                bytes.extend(value.scale.to_le_bytes());
                bytes.extend(value.offset.to_le_bytes());
            }
            Variant::UDim2(value) => {
                bytes.push(0x0a);
                for axis in [value.x, value.y] {
                    bytes.extend(axis.scale.to_le_bytes());
                    bytes.extend(axis.offset.to_le_bytes());
                }
            }
            Variant::String(value) => {
                bytes.push(0x02);
                bytes.extend((value.len() as u32).to_le_bytes());
                bytes.extend(value.as_bytes());
            }
            other => panic!("the test encoder has no case for {other:?}"),
        }
    }
    Variant::Unknown {
        type_id: 0x01,
        raw: bytes,
    }
}

/// A `StyleSheet` of its own, outside any GUI tree, as Roblox's own examples
/// put one in `ReplicatedStorage`.
fn sheet(dom: &mut WeakDom) -> Ref {
    dom.new_instance("StyleSheet", "Sheet", None)
}

fn rule(dom: &mut WeakDom, parent: Ref, selector: &str, properties: &[(&str, Variant)]) -> Ref {
    let referent = dom.new_instance("StyleRule", selector, Some(parent));
    dom.set_property(referent, "Selector", Variant::String(selector.into()))
        .unwrap();
    dom.set_property(referent, "PropertiesSerialize", blob(properties))
        .unwrap();
    referent
}

fn link(dom: &mut WeakDom, root: Ref, to: Ref) {
    let referent = dom.new_instance("StyleLink", "StyleLink", Some(root));
    dom.set_property(referent, "StyleSheet", Variant::Ref(to))
        .unwrap();
}

fn derive(dom: &mut WeakDom, sheet: Ref, from: Ref, priority: i32) {
    let referent = dom.new_instance("StyleDerive", "StyleDerive", Some(sheet));
    dom.set_property(referent, "StyleSheet", Variant::Ref(from))
        .unwrap();
    dom.set_property(referent, "Priority", Variant::Int32(priority))
        .unwrap();
}

fn attributes(dom: &mut WeakDom, referent: Ref, entries: &[(&str, Variant)]) {
    dom.set_property(referent, "AttributesSerialize", blob(entries))
        .unwrap();
}

fn tag(dom: &mut WeakDom, referent: Ref, tags: &str) {
    dom.set_property(referent, "Tags", Variant::String(tags.into()))
        .unwrap();
}

/// One `Frame` under a styled `ScreenGui`, and the sheet that styles it.
fn one_frame(selector: &str, properties: &[(&str, Variant)]) -> Vec<Element> {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    frame(&mut dom, gui, zero.clone(), zero);
    let sheet = sheet(&mut dom);
    rule(&mut dom, sheet, selector, properties);
    link(&mut dom, gui, sheet);

    resolve(&screens(&dom), VIEWPORT)
}

#[test]
fn a_class_selector_restyles_the_instances_of_that_class() {
    let styled = one_frame("Frame", &[("BackgroundColor3", colour(RED))]);
    assert_eq!(styled[0].background, RED);

    // The docs do not say a class selector reaches subclasses, so it does not.
    let missed = one_frame("GuiObject", &[("BackgroundColor3", colour(RED))]);
    assert_eq!(missed[0].background, [1.0, 1.0, 1.0]);
}

#[test]
fn a_rule_sets_a_property_the_instance_carries_itself() {
    // The instance's own `Size` is zero; the rule's is what gets drawn.
    let styled = one_frame("Frame", &[("Size", udim2(0.0, 120, 0.0, 40))]);

    assert_eq!(styled[0].rect.size(), [120.0, 40.0]);
}

#[test]
fn a_rule_reaches_a_ui_component_like_any_other_instance() {
    let (mut dom, gui) = screen_gui();
    let parent = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 100, 0.0, 100),
    );
    dom.new_instance("UIPadding", "UIPadding", Some(parent));
    let zero = udim2(0.0, 0, 0.0, 0);
    frame(&mut dom, parent, zero, udim2(1.0, 0, 1.0, 0));
    let sheet = sheet(&mut dom);
    rule(
        &mut dom,
        sheet,
        "UIPadding",
        &[(
            "PaddingLeft",
            Variant::UDim(UDim {
                scale: 0.0,
                offset: 10,
            }),
        )],
    );
    link(&mut dom, gui, sheet);

    let child = resolve(&screens(&dom), VIEWPORT)[1].rect;
    assert_eq!((child.x, child.width), (10.0, 90.0));
}

#[test]
fn a_state_selector_never_matches_in_a_still_frame() {
    let styled = one_frame("Frame:Hover", &[("BackgroundColor3", colour(RED))]);

    assert_eq!(styled[0].background, [1.0, 1.0, 1.0]);
}

#[test]
fn a_tag_selector_reaches_only_the_tagged_instance() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    let tagged = frame(&mut dom, gui, zero.clone(), zero.clone());
    frame(&mut dom, gui, zero.clone(), zero);
    tag(&mut dom, tagged, "Card\0");
    let sheet = sheet(&mut dom);
    rule(
        &mut dom,
        sheet,
        ".Card",
        &[("BackgroundColor3", colour(RED))],
    );
    link(&mut dom, gui, sheet);

    let styled = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(styled[0].background, RED);
    assert_eq!(styled[1].background, [1.0, 1.0, 1.0]);
}

#[test]
fn a_child_combinator_stops_where_a_descendant_one_carries_on() {
    // ScreenGui > outer > inner, with the rule aimed at the ScreenGui's tree.
    let build = |selector: &str| {
        let (mut dom, gui) = screen_gui();
        let zero = udim2(0.0, 0, 0.0, 0);
        let outer = frame(&mut dom, gui, zero.clone(), udim2(1.0, 0, 1.0, 0));
        frame(&mut dom, outer, zero.clone(), zero);
        let sheet = sheet(&mut dom);
        rule(
            &mut dom,
            sheet,
            selector,
            &[("BackgroundColor3", colour(RED))],
        );
        link(&mut dom, gui, sheet);
        resolve(&screens(&dom), VIEWPORT)
    };

    // A parent paints before its child, so the inner frame is the second.
    assert_eq!(build("ScreenGui > Frame")[1].background, [1.0, 1.0, 1.0]);
    assert_eq!(build("ScreenGui >> Frame")[1].background, RED);
}

#[test]
fn a_selector_list_styles_every_selector_in_it() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    frame(&mut dom, gui, zero.clone(), zero.clone());
    let button = dom.new_instance("TextButton", "TextButton", Some(gui));
    dom.set_property(button, "Size", zero.clone()).unwrap();
    dom.set_property(button, "Position", zero).unwrap();
    let sheet = sheet(&mut dom);
    rule(
        &mut dom,
        sheet,
        "Frame, TextButton",
        &[("BackgroundColor3", colour(RED))],
    );
    link(&mut dom, gui, sheet);

    let styled = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(styled[0].background, RED);
    assert_eq!(styled[1].background, RED);
}

#[test]
fn priority_settles_a_conflict_and_document_order_settles_a_tie() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    frame(&mut dom, gui, zero.clone(), zero);
    let sheet = sheet(&mut dom);
    // Written first but higher priority, so it beats the later rule.
    let strong = rule(
        &mut dom,
        sheet,
        "Frame",
        &[("BackgroundColor3", colour(RED))],
    );
    dom.set_property(strong, "Priority", Variant::Int32(10))
        .unwrap();
    rule(
        &mut dom,
        sheet,
        "Frame",
        &[("BackgroundColor3", colour(BLUE))],
    );
    link(&mut dom, gui, sheet);

    assert_eq!(resolve(&screens(&dom), VIEWPORT)[0].background, RED);

    // Levelled, the later of the two wins instead.
    dom.set_property(strong, "Priority", Variant::Int32(0))
        .unwrap();

    assert_eq!(resolve(&screens(&dom), VIEWPORT)[0].background, BLUE);
}

#[test]
fn a_sheet_overrides_the_sheet_it_derives_from() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    frame(&mut dom, gui, zero.clone(), zero);
    let base = sheet(&mut dom);
    rule(
        &mut dom,
        base,
        "Frame",
        &[("BackgroundColor3", colour(BLUE))],
    );
    let design = sheet(&mut dom);
    derive(&mut dom, design, base, 0);
    rule(
        &mut dom,
        design,
        "Frame",
        &[("BackgroundColor3", colour(RED))],
    );
    link(&mut dom, gui, design);

    assert_eq!(resolve(&screens(&dom), VIEWPORT)[0].background, RED);
}

#[test]
fn a_token_resolves_along_the_derive_chain() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    frame(&mut dom, gui, zero.clone(), zero);

    let tokens = sheet(&mut dom);
    attributes(&mut dom, tokens, &[("Magenta", colour(RED))]);
    let theme = sheet(&mut dom);
    derive(&mut dom, theme, tokens, 0);
    // A token standing for another token, as the theme sheets in the docs do.
    attributes(
        &mut dom,
        theme,
        &[("FrameColor", Variant::String("$Magenta".into()))],
    );
    let design = sheet(&mut dom);
    derive(&mut dom, design, theme, 0);
    rule(
        &mut dom,
        design,
        "Frame",
        &[("BackgroundColor3", Variant::String("$FrameColor".into()))],
    );
    link(&mut dom, gui, design);

    assert_eq!(resolve(&screens(&dom), VIEWPORT)[0].background, RED);
}

#[test]
fn a_token_that_stands_for_nothing_leaves_the_property_alone() {
    let styled = one_frame(
        "Frame",
        &[("BackgroundColor3", Variant::String("$Missing".into()))],
    );

    assert_eq!(styled[0].background, [1.0, 1.0, 1.0]);
}

#[test]
fn a_nested_rule_merges_its_selector_into_its_parents() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    let menu = frame(&mut dom, gui, zero.clone(), udim2(1.0, 0, 1.0, 0));
    dom.set_name(menu, "Menu").unwrap();
    frame(&mut dom, menu, zero.clone(), zero);
    let sheet = sheet(&mut dom);
    let outer = rule(&mut dom, sheet, "#Menu", &[]);
    rule(
        &mut dom,
        outer,
        "> Frame",
        &[("BackgroundColor3", colour(RED))],
    );
    link(&mut dom, gui, sheet);

    let styled = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(styled[0].background, [1.0, 1.0, 1.0]);
    assert_eq!(styled[1].background, RED);
}

#[test]
fn a_sheet_reaches_no_further_than_the_tree_its_link_sits_in() {
    let (mut dom, styled_gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    frame(&mut dom, styled_gui, zero.clone(), zero.clone());
    let other_gui = dom.new_instance("ScreenGui", "Other", None);
    frame(&mut dom, other_gui, zero.clone(), zero);
    let sheet = sheet(&mut dom);
    rule(
        &mut dom,
        sheet,
        "Frame",
        &[("BackgroundColor3", colour(RED))],
    );
    link(&mut dom, styled_gui, sheet);

    let styled = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(styled[0].background, RED);
    assert_eq!(styled[1].background, [1.0, 1.0, 1.0]);
}

#[test]
fn a_place_with_no_style_link_is_untouched() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    frame(&mut dom, gui, zero.clone(), zero);
    let sheet = sheet(&mut dom);
    rule(
        &mut dom,
        sheet,
        "Frame",
        &[("BackgroundColor3", colour(RED))],
    );

    assert_eq!(
        resolve(&screens(&dom), VIEWPORT)[0].background,
        [1.0, 1.0, 1.0]
    );
}
