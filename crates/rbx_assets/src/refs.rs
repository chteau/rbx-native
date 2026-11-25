//! Every asset a DOM refers to: `Content` properties (meshes, textures,
//! sounds, animations, SurfaceAppearance and MaterialVariant maps), font
//! families, a HumanoidDescription's numbered ids, and, best effort, the
//! asset strings and literal ids written in scripts, string values and
//! attributes (`"rbxassetid://123"`, `require(123)`,
//! `InsertService:LoadAsset(123)`).

use std::collections::BTreeSet;

use rbx_dom::{Content, Ref, Variant, WeakDom};

use crate::AssetRef;

/// HumanoidDescription properties that hold one asset id each.
const DESCRIPTION_IDS: &[&str] = &[
    "Face",
    "Head",
    "LeftArm",
    "LeftLeg",
    "RightArm",
    "RightLeg",
    "Torso",
    "GraphicTShirt",
    "Pants",
    "Shirt",
    "ClimbAnimation",
    "FallAnimation",
    "IdleAnimation",
    "JumpAnimation",
    "MoodAnimation",
    "RunAnimation",
    "SwimAnimation",
    "WalkAnimation",
];

/// HumanoidDescription properties that hold comma-separated asset ids.
const DESCRIPTION_LISTS: &[&str] = &[
    "BackAccessory",
    "FaceAccessory",
    "FrontAccessory",
    "HairAccessory",
    "HatAccessory",
    "NeckAccessory",
    "ShouldersAccessory",
    "WaistAccessory",
];

/// Engine calls whose first argument, written as a number, is an asset id.
const ID_CALLS: &[&str] = &["require(", "LoadAsset(", "LoadAssetAsync("];

/// Every asset `dom` refers to, under each of its roots.
pub fn in_dom(dom: &WeakDom) -> BTreeSet<AssetRef> {
    let mut found = BTreeSet::new();
    for &root in dom.root_refs() {
        collect(dom, root, &mut found);
    }
    found
}

/// Every asset `root` and its descendants refer to.
pub fn in_subtree(dom: &WeakDom, root: Ref) -> BTreeSet<AssetRef> {
    let mut found = BTreeSet::new();
    collect(dom, root, &mut found);
    found
}

fn collect(dom: &WeakDom, root: Ref, into: &mut BTreeSet<AssetRef>) {
    let mut stack = vec![root];
    while let Some(at) = stack.pop() {
        let Some(instance) = dom.get(at) else {
            continue;
        };
        let class = instance.class();
        for (name, value) in instance.properties() {
            value_refs(class, name, value, into);
        }
        for value in instance.attributes().values() {
            value_refs("", "", value, into);
        }
        stack.extend(instance.children().iter().copied());
    }
}

fn value_refs(class: &str, name: &str, value: &Variant, into: &mut BTreeSet<AssetRef>) {
    let id = |n: f64| (n >= 1.0).then_some(AssetRef::Id(n as u64));
    let numbered = (class == "HumanoidDescription" && DESCRIPTION_IDS.contains(&name))
        || (matches!(class, "AccessoryDescription" | "BodyPartDescription") && name == "AssetId");
    let found = match value {
        Variant::Content(Content::Uri(uri)) => AssetRef::parse(uri).ok(),
        Variant::Font(font) => AssetRef::parse(&font.family).ok(),
        Variant::String(list)
            if class == "HumanoidDescription" && DESCRIPTION_LISTS.contains(&name) =>
        {
            into.extend(
                list.split(',')
                    .filter_map(|n| n.trim().parse::<f64>().ok())
                    .filter_map(id),
            );
            None
        }
        Variant::String(text) => {
            in_text(text, into);
            None
        }
        Variant::Int64(n) if numbered => id(*n as f64),
        Variant::Int32(n) if numbered => id(f64::from(*n)),
        Variant::Float64(n) if numbered => id(*n),
        Variant::Float32(n) if numbered => id(f64::from(*n)),
        _ => None,
    };
    into.extend(found.filter(|r| *r != AssetRef::Empty));
}

/// The asset strings and literal ids in `text` (a script's source, a
/// string value): best effort, a computed id is not seen.
pub fn in_text(text: &str, into: &mut BTreeSet<AssetRef>) {
    let mut from = 0;
    while let Some(at) = text[from..].find("://").map(|i| from + i) {
        let start = text[..at]
            .rfind(|c: char| !c.is_ascii_alphanumeric())
            .map_or(0, |i| i + 1);
        let end = text[at..]
            .find(|c: char| c.is_whitespace() || "\"'`)]}<>,;\\".contains(c))
            .map_or(text.len(), |i| at + i);
        if let Ok(found) = AssetRef::parse(&text[start..end]) {
            into.extend(Some(found).filter(|r| *r != AssetRef::Empty));
        }
        from = at + 3;
    }
    for call in ID_CALLS {
        let mut from = 0;
        while let Some(at) = text[from..].find(call).map(|i| from + i + call.len()) {
            from = at;
            let rest = text[at..].trim_start();
            let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            let after = rest[digits..].trim_start();
            let preceded = text[..at - call.len()]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
            if digits > 0 && preceded && (after.starts_with(')') || after.starts_with(',')) {
                if let Ok(id) = rest[..digits].parse::<u64>() {
                    into.extend((id > 0).then_some(AssetRef::Id(id)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::Font;

    fn text_refs(text: &str) -> Vec<AssetRef> {
        let mut found = BTreeSet::new();
        in_text(text, &mut found);
        found.into_iter().collect()
    }

    #[test]
    fn scripts_give_their_asset_strings_and_literal_ids() {
        let source = r#"
            local Insert = game:GetService("InsertService")
            local model = Insert:LoadAsset(1111)
            local lib = require(2222)
            local other = require(script.Parent.Module)
            sound.SoundId = "rbxassetid://3333"
            image.Image = 'http://www.roblox.com/asset/?id=4444'
            thumb.Image = "rbxthumb://type=Asset&id=5555&w=150&h=150"
            local sky = "rbxasset://textures/sky/sky512_bk.tex"
            local notMine = myrequire(6666)
            local computed = require(7777 + 1)
            print("https://example.com/asset?id=8888")
        "#;
        assert_eq!(
            text_refs(source),
            vec![
                AssetRef::Id(1111),
                AssetRef::Id(2222),
                AssetRef::Id(3333),
                AssetRef::Id(4444),
                AssetRef::Native("textures/sky/sky512_bk.tex".into()),
                AssetRef::Thumb("type=Asset&id=5555&w=150&h=150".into()),
            ]
        );
    }

    #[test]
    fn a_place_gives_every_kind_of_reference_once() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let uri = |s: &str| Variant::Content(Content::Uri(s.into()));
        let mesh = dom.new_instance("MeshPart", "Mesh", Some(workspace));
        dom.set_property(mesh, "MeshId", uri("rbxassetid://10"))
            .unwrap();
        dom.set_property(mesh, "TextureID", uri("rbxassetid://11"))
            .unwrap();
        let surface = dom.new_instance("SurfaceAppearance", "Look", Some(mesh));
        dom.set_property(surface, "ColorMap", uri("rbxassetid://12"))
            .unwrap();
        let decal = dom.new_instance("Decal", "Decal", Some(mesh));
        dom.set_property(decal, "Texture", uri("rbxassetid://10"))
            .unwrap();
        let sound = dom.new_instance("Sound", "Sound", Some(workspace));
        dom.set_property(sound, "SoundId", uri("rbxassetid://13"))
            .unwrap();
        dom.set_property(sound, "Name", Variant::String("rbx".into()))
            .unwrap();
        let label = dom.new_instance("TextLabel", "Label", Some(workspace));
        let mut font = Font::from_legacy(4).unwrap();
        font.family = "rbxassetid://14".into();
        dom.set_property(label, "FontFace", Variant::Font(font))
            .unwrap();
        let description = dom.new_instance("HumanoidDescription", "Look", Some(workspace));
        dom.set_property(description, "Shirt", Variant::Int64(15))
            .unwrap();
        dom.set_property(description, "Pants", Variant::Int64(0))
            .unwrap();
        dom.set_property(
            description,
            "HatAccessory",
            Variant::String("16, 17".into()),
        )
        .unwrap();
        let script = dom.new_instance("Script", "Main", Some(workspace));
        dom.set_property(
            script,
            "Source",
            Variant::String("local m = require(18)".into()),
        )
        .unwrap();
        let empty = dom.new_instance("Animation", "Empty", Some(workspace));
        dom.set_property(empty, "AnimationId", uri("")).unwrap();

        let found: Vec<u64> = in_dom(&dom)
            .into_iter()
            .map(|r| match r {
                AssetRef::Id(id) => id,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(found, (10..=18).collect::<Vec<_>>());
        assert_eq!(in_subtree(&dom, sound), BTreeSet::from([AssetRef::Id(13)]));
    }
}
