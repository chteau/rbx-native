//! JSON parsing for Roblox API dump format.

use serde::Deserialize;

use crate::class::{ClassDescriptor, PropertyDescriptor};
use crate::enums::EnumDescriptor;

// The dump marks classes with no parent (e.g. `Instance`) with this sentinel
// instead of omitting the field; treat it the same as "no superclass".
const ROOT_SUPERCLASS: &str = "<<<ROOT>>>";

#[derive(Deserialize)]
struct RawDump {
    #[serde(rename = "Classes")]
    classes: Vec<RawClass>,
    #[serde(rename = "Enums")]
    enums: Vec<RawEnum>,
}

#[derive(Deserialize)]
struct RawClass {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Superclass")]
    superclass: Option<String>,
    #[serde(rename = "Members")]
    members: Vec<RawMember>,
    #[serde(rename = "Tags", default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct RawMember {
    #[serde(rename = "MemberType")]
    member_type: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "ValueType")]
    value_type: Option<RawValueType>,
    // Only Property members carry this; Function/Event/Callback members
    // share the same struct but have no Category, so it must default rather
    // than fail deserialization for them.
    #[serde(rename = "Category", default)]
    category: String,
    #[serde(rename = "Tags", default)]
    tags: Vec<String>,
    // Same story as Category: only Property members carry a Serialization
    // object, so a Function/Event/Callback member falls back to the default
    // (both flags false, never read since those members are filtered out
    // before a PropertyDescriptor is built).
    #[serde(rename = "Serialization", default)]
    serialization: RawSerialization,
}

#[derive(Deserialize)]
struct RawValueType {
    #[serde(rename = "Name")]
    name: String,
}

#[derive(Deserialize, Default)]
struct RawSerialization {
    #[serde(rename = "CanLoad", default)]
    can_load: bool,
    #[serde(rename = "CanSave", default)]
    can_save: bool,
}

#[derive(Deserialize)]
struct RawEnum {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Items")]
    items: Vec<RawEnumItem>,
}

#[derive(Deserialize)]
struct RawEnumItem {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Value")]
    value: u32,
}

/// Parses a Roblox API dump JSON into class and enum descriptors.
pub(crate) fn parse_dump(
    json: &str,
) -> Result<(Vec<ClassDescriptor>, Vec<EnumDescriptor>), serde_json::Error> {
    let raw: RawDump = serde_json::from_str(json)?;

    let classes = raw.classes.into_iter().map(convert_class).collect();
    let enums = raw.enums.into_iter().map(convert_enum).collect();

    Ok((classes, enums))
}

fn convert_class(raw: RawClass) -> ClassDescriptor {
    let superclass = raw.superclass.filter(|name| name != ROOT_SUPERCLASS);

    // Function/Event/Callback members share the same list; only properties
    // carry a ValueType and matter for serialized instance data.
    let properties = raw
        .members
        .into_iter()
        .filter(|member| member.member_type == "Property")
        .filter_map(|member| {
            member.value_type.map(|value_type| PropertyDescriptor {
                name: member.name,
                value_type: value_type.name,
                category: member.category,
                tags: member.tags,
                can_load: member.serialization.can_load,
                can_save: member.serialization.can_save,
            })
        })
        .collect();

    ClassDescriptor {
        name: raw.name,
        superclass,
        properties,
        tags: raw.tags,
    }
}

fn convert_enum(raw: RawEnum) -> EnumDescriptor {
    EnumDescriptor {
        name: raw.name,
        items: raw
            .items
            .into_iter()
            .map(|item| (item.name, item.value))
            .collect(),
    }
}
