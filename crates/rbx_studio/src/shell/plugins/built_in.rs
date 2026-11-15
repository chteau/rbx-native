//! Studio plugins whose job RbxNative does itself, never run here. The Team
//! Presence plugin is Studio's half of the Collaborators dock: run inside
//! RbxNative it would open a second, broken Collaborators widget (the
//! plugin host has no place id, so it only ever says to publish first).

use rbx_dom::{Variant, WeakDom};
use rbx_lua::plugin::{PluginSource, SourceBody};

/// The Team Presence plugin on the Creator Store.
pub(super) const TEAM_PRESENCE_ASSET: u64 = 96_675_894_010_955;

/// The Manage Plugins status of a skipped plugin.
pub(super) const NOTE: &str = "Built into RbxNative, not needed here";

/// Whether `source` is the Team Presence plugin, told by its code rather
/// than its file name: a Creator Store install is named after the listing
/// and a copy in Studio's folder can be renamed. Its scripts make the
/// "rbx-native" toolbar and keep the key in the `RbxNativePresence` folder.
pub(super) fn is_team_presence(source: &PluginSource) -> bool {
    let sources: Vec<&str> = match &source.body {
        SourceBody::Luau(text) => vec![text.as_str()],
        SourceBody::Model(dom) => script_sources(dom),
    };
    let has = |needle: &str| sources.iter().any(|text| text.contains(needle));
    has("CreateToolbar(\"rbx-native\")") && has(rbx_presence::secret::FOLDER_NAME)
}

fn script_sources(dom: &WeakDom) -> Vec<&str> {
    let mut found = Vec::new();
    let mut stack = dom.root_refs().to_vec();
    while let Some(instance) = stack.pop().and_then(|r| dom.get(r)) {
        stack.extend_from_slice(instance.children());
        if let Some(Variant::String(text)) = instance.properties().get("Source") {
            found.push(text.as_str());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_lua::plugin::Origin;

    fn luau(text: &str) -> PluginSource {
        PluginSource {
            name: "Anything".into(),
            body: SourceBody::Luau(text.into()),
            origin: Origin::Roblox {
                path: "Anything.lua".into(),
            },
        }
    }

    #[test]
    fn the_built_plugin_is_recognised_whatever_its_file_is_called() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/team-presence/src");
        let main = std::fs::read_to_string(root.join("TeamPresence.server.luau")).unwrap();
        let secret = std::fs::read_to_string(root.join("Secret.luau")).unwrap();
        let mut dom = WeakDom::new();
        let script = dom.new_instance("Script", "Renamed", None);
        let module = dom.new_instance("ModuleScript", "Secret", Some(script));
        dom.set_property(script, "Source", Variant::String(main))
            .unwrap();
        dom.set_property(module, "Source", Variant::String(secret))
            .unwrap();
        let source = PluginSource {
            name: "My copy".into(),
            body: SourceBody::Model(dom),
            origin: Origin::Roblox {
                path: "My copy.rbxm".into(),
            },
        };
        assert!(is_team_presence(&source));
    }

    #[test]
    fn other_plugins_are_not() {
        assert!(!is_team_presence(&luau(
            "local t = plugin:CreateToolbar(\"rbx-native\")"
        )));
        assert!(!is_team_presence(&luau("print('RbxNativePresence')")));
    }
}
