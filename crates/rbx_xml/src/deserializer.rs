//! Orchestrates parsing an XML document into a `WeakDom`: parse the generic tree,
//! assign referents, build the SharedStrings table, then walk `Item`s to insert
//! instances, decode their properties, and wire up parent/child edges.

use std::collections::HashMap;

use rbx_dom::{Instance, Ref, Variant, WeakDom};

use crate::error::XmlError;
use crate::value::{self, Ctx};
use crate::xml_tree::{self, Node};

// The instance name lives in a dedicated field of the DOM rather than in the
// property map, so it is redirected instead of stored twice, mirroring
// rbx_binary's own deserializer.
const NAME_PROPERTY: &str = "Name";

/// Deserializes a Roblox XML place/model file (`.rbxlx`/`.rbxmx`) into a DOM tree.
///
/// Property decoding is best-effort by design: an unrecognized type element, a
/// dangling `Ref`, or an `Item` missing a required attribute is skipped rather
/// than failing the whole document (see the `value` module).
pub fn deserialize(input: &str) -> Result<WeakDom, XmlError> {
    let document = xml_tree::parse(input)?;
    let root = document.child("roblox").ok_or(XmlError::MissingRoot)?;

    let shared = shared_strings(root);
    let mut referents = HashMap::new();
    let mut next_ref = 1u32;
    assign_referents(root, &mut referents, &mut next_ref);

    let ctx = Ctx {
        referents: &referents,
        shared: &shared,
    };
    let mut dom = WeakDom::new();
    insert_items(root, None, &referents, &ctx, &mut dom);

    Ok(dom)
}

fn shared_strings(root: &Node) -> HashMap<String, Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let Some(table) = root.child("SharedStrings") else {
        return HashMap::new();
    };

    table
        .children_named("SharedString")
        .filter_map(|node| {
            let md5 = node.attr("md5")?.to_owned();
            let bytes = STANDARD.decode(node.text_trim()).ok()?;
            Some((md5, bytes))
        })
        .collect()
}

// Assigns a stable `Ref` to every `Item` referent up front, in file order, so
// property decoding (which needs to resolve `Ref`/`Content` referents) never
// depends on an `Item` it has not visited yet, regardless of nesting or of
// whether the reference is forward or backward.
fn assign_referents(node: &Node, referents: &mut HashMap<String, Ref>, next_ref: &mut u32) {
    for item in node.children_named("Item") {
        if let Some(referent) = item.attr("referent") {
            referents.entry(referent.to_owned()).or_insert_with(|| {
                let r = Ref::new(*next_ref);
                *next_ref += 1;
                r
            });
        }
        assign_referents(item, referents, next_ref);
    }
}

fn insert_items(
    node: &Node,
    parent: Option<Ref>,
    referents: &HashMap<String, Ref>,
    ctx: &Ctx<'_>,
    dom: &mut WeakDom,
) {
    for item in node.children_named("Item") {
        // `class`/`referent` are both required by xml.md; an `Item` missing
        // either is unusable (it cannot be inserted, or cannot be referenced),
        // so it and its subtree are dropped rather than guessed at.
        let (Some(class), Some(referent_key)) = (item.attr("class"), item.attr("referent")) else {
            continue;
        };
        let Some(&referent) = referents.get(referent_key) else {
            continue;
        };

        // Instances start out named after their class; a `Name` property
        // overwrites it below when the file carries one.
        dom.insert(Instance::new(referent, class, class));
        if let Some(properties) = item.child("Properties") {
            apply_properties(dom, referent, properties, ctx);
        }
        dom.set_parent(referent, parent);

        insert_items(item, Some(referent), referents, ctx, dom);
    }
}

fn apply_properties(dom: &mut WeakDom, referent: Ref, properties: &Node, ctx: &Ctx<'_>) {
    for property in &properties.children {
        let Some(name) = property.attr("name") else {
            continue;
        };
        let Some(value) = value::decode(property, ctx) else {
            continue;
        };
        let Some(instance) = dom.get_mut(referent) else {
            continue;
        };

        match (name, value) {
            (NAME_PROPERTY, Variant::String(new_name)) => instance.set_name(new_name),
            (_, value) => {
                instance.properties_mut().insert(name.to_owned(), value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_root_is_an_error() {
        assert!(matches!(
            deserialize("<not-roblox/>"),
            Err(XmlError::MissingRoot)
        ));
    }

    #[test]
    fn two_instance_place_builds_the_expected_tree() {
        let xml = r#"
            <roblox version="4">
                <Item class="Workspace" referent="RBXWorkspace">
                    <Properties><string name="Name">Workspace</string></Properties>
                    <Item class="Part" referent="RBXPart">
                        <Properties>
                            <string name="Name">Baseplate</string>
                            <Vector3 name="size"><X>512</X><Y>1.2</Y><Z>512</Z></Vector3>
                        </Properties>
                    </Item>
                </Item>
            </roblox>
        "#;

        let dom = deserialize(xml).unwrap();

        let roots = dom.root_refs();
        assert_eq!(roots.len(), 1);
        let workspace = dom.get(roots[0]).unwrap();
        assert_eq!(workspace.class(), "Workspace");
        assert_eq!(workspace.name(), "Workspace");
        assert_eq!(workspace.children().len(), 1);

        let part = dom.get(workspace.children()[0]).unwrap();
        assert_eq!(part.class(), "Part");
        assert_eq!(part.name(), "Baseplate");
        assert_eq!(
            part.properties().get("size"),
            Some(&Variant::Vector3(rbx_dom::Vector3Data {
                x: 512.0,
                y: 1.2,
                z: 512.0
            }))
        );
    }

    #[test]
    fn ref_property_resolves_across_items_including_forward_references() {
        // The Ref property is declared before the Item it points to, in file order.
        let xml = r#"
            <roblox version="4">
                <Item class="Part" referent="RBXA">
                    <Properties><Ref name="Target">RBXB</Ref></Properties>
                </Item>
                <Item class="Part" referent="RBXB">
                    <Properties></Properties>
                </Item>
            </roblox>
        "#;

        let dom = deserialize(xml).unwrap();
        let roots = dom.root_refs();
        let a = dom.get(roots[0]).unwrap();
        let target = a.properties().get("Target").unwrap();
        assert_eq!(*target, Variant::Ref(roots[1]));
    }

    #[test]
    fn null_ref_property_is_omitted() {
        let xml = r#"
            <roblox version="4">
                <Item class="Part" referent="RBXA">
                    <Properties><Ref name="Target">null</Ref></Properties>
                </Item>
            </roblox>
        "#;

        let dom = deserialize(xml).unwrap();
        let a = dom.get(dom.root_refs()[0]).unwrap();
        assert!(a.properties().get("Target").is_none());
    }

    #[test]
    fn shared_string_property_resolves_against_the_table() {
        let xml = r#"
            <roblox version="4">
                <SharedStrings>
                    <SharedString md5="key1">dGFnZ2Vk</SharedString>
                </SharedStrings>
                <Item class="Script" referent="RBXA">
                    <Properties><SharedString name="Source">key1</SharedString></Properties>
                </Item>
            </roblox>
        "#;

        let dom = deserialize(xml).unwrap();
        let a = dom.get(dom.root_refs()[0]).unwrap();
        assert_eq!(
            a.properties().get("Source"),
            Some(&Variant::String("tagged".to_owned()))
        );
    }

    #[test]
    fn unknown_type_element_degrades_to_unknown_variant() {
        let xml = r#"
            <roblox version="4">
                <Item class="Part" referent="RBXA">
                    <Properties><FutureType name="Odd">mystery</FutureType></Properties>
                </Item>
            </roblox>
        "#;

        let dom = deserialize(xml).unwrap();
        let a = dom.get(dom.root_refs()[0]).unwrap();
        assert!(matches!(
            a.properties().get("Odd"),
            Some(Variant::Unknown { .. })
        ));
    }

    #[test]
    fn item_missing_required_attribute_is_skipped() {
        let xml = r#"
            <roblox version="4">
                <Item class="Part">
                    <Properties></Properties>
                </Item>
            </roblox>
        "#;

        let dom = deserialize(xml).unwrap();
        assert!(dom.root_refs().is_empty());
    }
}
