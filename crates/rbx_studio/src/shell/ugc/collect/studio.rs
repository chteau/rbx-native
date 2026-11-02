//! What Studio's Save to Roblox checks on an accessory's `Handle` beyond the
//! spec pages, matched against it on the same items: the body scale comes
//! from the handle's `AvatarPartScaleType` value (none means Classic, so an
//! unmarked hat gets the 3 x 4 x 3 box), and a handle needs a `TextureID` or
//! a `SurfaceAppearance` (rigid items only; a layered one passes without).

use rbx_dom::{Content, Ref, Variant, WeakDom};
use rbx_ugc::schema::Node;
use rbx_ugc::{BodyScale, Category, Finding, Rule, Severity, Value};

use super::super::ItemKind;

/// Sets `kind.scale` to the one Studio will size the item for, with a note
/// when that is not the scale that was picked, and adds the texture rule.
pub(super) fn handle_rules(
    dom: &WeakDom,
    handle: Ref,
    kind: &mut ItemKind,
    item: &str,
    out: &mut Vec<Finding>,
) {
    let category = kind.asset.category();
    // Layered items pass Studio's check with neither, so only rigid ones
    // get these two.
    if category != Category::Rigid {
        return;
    }
    {
        let marked = scale_of(dom, handle);
        let scale = marked.unwrap_or(BodyScale::Classic);
        if scale != kind.scale {
            let why = match marked {
                Some(_) => format!(
                    "its Handle\u{2019}s AvatarPartScaleType says {}",
                    scale.part_scale_type()
                ),
                None => "its Handle has no AvatarPartScaleType value".to_string(),
            };
            out.push(Finding {
                rule: Rule::ItemSize,
                severity: Severity::Note,
                subject: format!("{item}.Handle"),
                measured: Some(Value::Text(scale.label().into())),
                limit: Some(Value::Text(kind.scale.label().into())),
                message: format!(
                    "Checked for {} bodies, not {}: {why}, and that is what Roblox sizes it by. Add a StringValue named AvatarPartScaleType under the Handle set to {} to size it for {} bodies.",
                    scale.label(),
                    kind.scale.label(),
                    kind.scale.part_scale_type(),
                    kind.scale.label(),
                ),
            });
            kind.scale = scale;
        }
    }
    if !textured(dom, handle) {
        out.push(Finding {
            rule: Rule::MissingTexture,
            severity: Severity::Error,
            subject: format!("{item}.Handle"),
            measured: None,
            limit: None,
            message: "The Handle has an empty TextureID and no SurfaceAppearance. Set a TextureID or add a SurfaceAppearance; Roblox refuses the item without one.".into(),
        });
    }
}

fn scale_of(dom: &WeakDom, handle: Ref) -> Option<BodyScale> {
    let value = dom
        .get(handle)?
        .children()
        .iter()
        .filter_map(|&c| dom.get(c))
        .find(|c| c.class() == "StringValue" && c.name() == "AvatarPartScaleType")?;
    let Some(Variant::String(text)) = value.properties().get("Value") else {
        return None;
    };
    BodyScale::ALL
        .into_iter()
        .find(|s| s.part_scale_type() == text)
}

/// `root` and everything under it as the schema rules read it.
pub(super) fn node(dom: &WeakDom, root: Ref) -> Node {
    let Some(inst) = dom.get(root) else {
        return Node::default();
    };
    let props = inst.properties();
    let vector = |keys: &[&str]| {
        keys.iter().find_map(|k| match props.get(*k) {
            Some(Variant::Vector3(v)) => Some([v.x, v.y, v.z]),
            _ => None,
        })
    };
    let scaled = inst.class() == "MeshPart"
        && match (
            vector(&["size", "Size"]),
            vector(&["InitialSize", "MeshSize"]),
        ) {
            (Some(size), Some(mesh)) => (0..3).any(|i| (size[i] - mesh[i]).abs() > 1e-3),
            _ => false,
        };
    Node {
        class: inst.class().to_string(),
        name: inst.name().to_string(),
        tagged: !inst.tags().is_empty(),
        attributed: !inst.attributes().is_empty(),
        value: match props.get("Value") {
            Some(Variant::String(text)) => Some(text.clone()),
            _ => None,
        },
        anchored: matches!(props.get("Anchored"), Some(Variant::Bool(true))),
        scaled,
        reference_mesh: super::uri(props, "ReferenceMeshId").is_some(),
        // Enum.WrapLayerAutoSkin.EnabledOverride
        auto_skin_override: matches!(props.get("AutoSkin"), Some(Variant::Enum(2))),
        children: inst.children().iter().map(|&c| node(dom, c)).collect(),
    }
}

fn textured(dom: &WeakDom, handle: Ref) -> bool {
    let Some(inst) = dom.get(handle) else {
        return false;
    };
    let set = |key: &str| match inst.properties().get(key) {
        Some(Variant::String(text)) | Some(Variant::Content(Content::Uri(text))) => {
            !text.is_empty()
        }
        _ => false,
    };
    set("TextureID")
        || set("TextureContent")
        || inst
            .children()
            .iter()
            .filter_map(|&c| dom.get(c))
            .any(|c| c.class() == "SurfaceAppearance")
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Variant, WeakDom};
    use rbx_ugc::{AssetType, BodyScale, Rule};

    use super::handle_rules;
    use crate::shell::ugc::ItemKind;

    fn hat(dom: &mut WeakDom) -> rbx_dom::Ref {
        let acc = dom.new_instance("Accessory", "Hat", None);
        dom.new_instance("MeshPart", "Handle", Some(acc))
    }

    #[test]
    fn an_unmarked_hat_is_sized_for_classic_and_needs_a_texture() {
        let mut dom = WeakDom::new();
        let handle = hat(&mut dom);
        let mut kind = ItemKind {
            asset: AssetType::Hat,
            scale: BodyScale::Normal,
        };
        let mut out = Vec::new();
        handle_rules(&dom, handle, &mut kind, "Hat", &mut out);
        assert_eq!(kind.scale, BodyScale::Classic);
        assert!(out.iter().any(|f| f.rule == Rule::MissingTexture));
    }

    #[test]
    fn the_scale_value_and_a_texture_are_read() {
        let mut dom = WeakDom::new();
        let handle = hat(&mut dom);
        dom.set_property(
            handle,
            "TextureID",
            Variant::String("rbxassetid://5".into()),
        )
        .unwrap();
        let value = dom.new_instance("StringValue", "AvatarPartScaleType", Some(handle));
        dom.set_property(value, "Value", Variant::String("ProportionsSlender".into()))
            .unwrap();
        let mut kind = ItemKind::default();
        let mut out = Vec::new();
        handle_rules(&dom, handle, &mut kind, "Hat", &mut out);
        assert_eq!(kind.scale, BodyScale::Slender);
        assert!(out.iter().all(|f| f.rule != Rule::MissingTexture));
    }
}
