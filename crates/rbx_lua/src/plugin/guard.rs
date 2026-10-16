//! The write guard native plugins run under: place content is off limits
//! without the `dom.write` permission, the plugin's own instances are not.

use rbx_dom::{Ref, WeakDom};

use crate::instance::tree;

const PLACE_SERVICES: [&str; 15] = [
    "Workspace",
    "Lighting",
    "ReplicatedStorage",
    "ReplicatedFirst",
    "ServerStorage",
    "ServerScriptService",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "Teams",
    "SoundService",
    "Chat",
    "LocalizationService",
    "TextChatService",
    "MaterialService",
];

pub(crate) fn place_writes_denied(
    plugin_name: String,
) -> impl Fn(&WeakDom, Ref) -> Result<(), String> {
    move |dom, referent| {
        let mut top = referent;
        while let Some(parent) = tree::parent_of(dom, top) {
            top = parent;
        }
        match dom.get(top) {
            Some(root) if PLACE_SERVICES.contains(&root.class()) => {
                Err(format!("{plugin_name} lacks the dom.write permission"))
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLACE: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/tests/TestPlace.rbxl"
    ));

    #[test]
    fn denies_place_services_and_allows_other_roots() {
        let mut dom = rbx_binary::deserialize(PLACE).unwrap();
        let workspace = dom
            .root_refs()
            .iter()
            .copied()
            .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
            .unwrap();
        let inside = dom.new_instance("Part", "Part", Some(workspace));
        let own = dom.new_instance("Folder", "Own", None);
        let guard = place_writes_denied("Tool".to_string());
        assert_eq!(
            guard(&dom, inside),
            Err("Tool lacks the dom.write permission".to_string())
        );
        assert!(guard(&dom, workspace).is_err());
        assert!(guard(&dom, own).is_ok());
    }
}
