//! The write guard plugins run under: place content needs Edit your game,
//! scripts need Script injection, the plugin's own instances need nothing.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::Lua;
use rbx_dom::{Ref, WeakDom};
use rbx_permissions::Decision;

use super::api::{Answer, Capability, OutputKind, Permission, PluginId, PluginProfile, PromptKind};
use super::api::{Request, UiId};
use super::env::{env, Shared};
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

/// The classes Studio's Script Injection permission covers.
const SCRIPT_CLASSES: [&str; 3] = ["Script", "LocalScript", "ModuleScript"];

/// The guard every plugin writes under. Place content needs Edit your game.
/// Script injection is needed to change a script in the place, or to parent
/// a subtree holding a script into it; scripts outside the place (the
/// plugin's own, a loose one it builds) need nothing. A plugin not answered
/// for yet gets the write refused and the user asked, once, as Studio asks.
pub(crate) fn writes(
    plugin: PluginId,
    profile: PluginProfile,
    shared: Rc<RefCell<Shared>>,
) -> impl Fn(&WeakDom, Ref, Option<Ref>) -> Result<(), String> {
    move |dom, referent, child| {
        if !in_place(dom, referent) {
            return Ok(());
        }
        let store = shared.borrow().permissions.clone();
        let decision = {
            let store = store.borrow();
            let policy = store.policy();
            if !policy.allows(&profile, Capability::EditPlace) {
                return Err(lacks(&profile, Capability::EditPlace));
            }
            let scripted = match child {
                Some(child) => holds_script(dom, child),
                None => is_script(dom, referent),
            };
            if !scripted {
                return Ok(());
            }
            policy.decide(&profile, Capability::EditScripts)
        };
        match decision {
            Decision::Allow => Ok(()),
            Decision::Deny => Err(lacks(&profile, Capability::EditScripts)),
            Decision::Ask => {
                let mut shared = shared.borrow_mut();
                if !shared.script_prompts.contains_key(&plugin) {
                    let id = shared.next_id();
                    shared.script_prompts.insert(plugin, id);
                    shared.requests.push(Request::Prompt {
                        id,
                        plugin,
                        kind: PromptKind::Permission(Permission::Capability(
                            Capability::EditScripts,
                        )),
                    });
                }
                Err(format!(
                    "{} needs the '{}' permission; the editor is asking you for it",
                    profile.name,
                    Capability::EditScripts.label()
                ))
            }
        }
    }
}

fn in_place(dom: &WeakDom, referent: Ref) -> bool {
    let mut top = referent;
    while let Some(parent) = tree::parent_of(dom, top) {
        top = parent;
    }
    dom.get(top)
        .is_some_and(|root| PLACE_SERVICES.contains(&root.class()))
}

fn is_script(dom: &WeakDom, referent: Ref) -> bool {
    dom.get(referent)
        .is_some_and(|i| SCRIPT_CLASSES.contains(&i.class()))
}

fn holds_script(dom: &WeakDom, root: Ref) -> bool {
    is_script(dom, root)
        || dom
            .get(root)
            .is_some_and(|i| i.children().iter().any(|c| holds_script(dom, *c)))
}

fn lacks(profile: &PluginProfile, capability: Capability) -> String {
    format!(
        "{} lacks the '{}' permission (Settings \u{203a} Permissions)",
        profile.name,
        capability.label()
    )
}

/// Stores the answer to a script injection prompt; `false` when `prompt`
/// was not one.
pub(crate) fn script_prompt_answered(lua: &Lua, prompt: UiId, answer: &Answer) -> bool {
    let Answer::Allow(allow) = answer else {
        return false;
    };
    let env = env(lua);
    let mut shared = env.shared.borrow_mut();
    if shared.script_prompts.get(&env.plugin) != Some(&prompt) {
        return false;
    }
    shared.script_prompts.remove(&env.plugin);
    let decision = if *allow {
        Decision::Allow
    } else {
        Decision::Deny
    };
    let saved = shared.permissions.borrow_mut().change(|policy| {
        policy.set(&env.profile.name, Capability::EditScripts, Some(decision))
    });
    if let Err(error) = saved {
        shared.push_output(&env.name, OutputKind::Warn, format!("permissions: {error}"));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLACE: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/tests/TestPlace.rbxl"
    ));

    fn place() -> (WeakDom, Ref) {
        let dom = rbx_binary::deserialize(PLACE).unwrap();
        let workspace = dom
            .root_refs()
            .iter()
            .copied()
            .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
            .unwrap();
        (dom, workspace)
    }

    #[test]
    fn edit_your_game_covers_place_services_only() {
        let (mut dom, workspace) = place();
        let inside = dom.new_instance("Part", "Part", Some(workspace));
        let other = dom.new_instance("Folder", "Loose", None);
        let profile = PluginProfile {
            name: "Tool".into(),
            kind: rbx_permissions::PluginKind::Luau,
            ..Default::default()
        };
        let guard = writes(1, profile, Rc::default());
        assert_eq!(
            guard(&dom, inside, None),
            Err("Tool lacks the 'Edit your game' permission (Settings \u{203a} Permissions)".into())
        );
        assert!(guard(&dom, workspace, None).is_err());
        assert!(guard(&dom, other, None).is_ok());
    }

    #[test]
    fn scripts_entering_the_place_ask_once_then_follow_the_answer() {
        let (mut dom, workspace) = place();
        let script = dom.new_instance("Script", "Injected", Some(workspace));
        let loose_root = dom.new_instance("Model", "Loose", None);
        let loose = dom.new_instance("ModuleScript", "Lib", Some(loose_root));
        let part = dom.new_instance("Part", "P", None);
        let shared: Rc<RefCell<Shared>> = Rc::default();
        let guard = writes(1, PluginProfile::roblox("Tool"), shared.clone());

        // Building scripts outside the place is free; moving them in is not.
        assert!(guard(&dom, loose, None).is_ok());
        assert!(guard(&dom, workspace, Some(part)).is_ok());
        assert!(guard(&dom, workspace, Some(loose_root))
            .unwrap_err()
            .contains("asking you"));
        assert!(guard(&dom, script, None).is_err());
        let prompts = shared
            .borrow()
            .requests
            .iter()
            .filter(|r| matches!(r, Request::Prompt { kind: PromptKind::Permission(_), .. }))
            .count();
        assert_eq!(prompts, 1);

        let store = shared.borrow().permissions.clone();
        store
            .borrow_mut()
            .change(|p| p.set("Tool", Capability::EditScripts, Some(Decision::Allow)))
            .unwrap();
        assert!(guard(&dom, script, None).is_ok());
        store
            .borrow_mut()
            .change(|p| p.set("Tool", Capability::EditScripts, Some(Decision::Deny)))
            .unwrap();
        assert!(guard(&dom, script, None).unwrap_err().contains("lacks"));
    }
}
