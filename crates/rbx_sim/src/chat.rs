//! What a server's `TextChatService` holds before any script runs (the
//! TextChat golden): the `TextChannels` folder with `RBXGeneral` and
//! `RBXSystem` when `CreateDefaultTextChannels` is on, the four
//! configurations and the `TextChatCommands` folder, and each player's
//! `TextSource` in `RBXGeneral`, named after them. The messages themselves
//! run in `rbx_lua`'s chat module.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::humanoid::flag;

const CONFIGS: &[&str] = &[
    "BubbleChatConfiguration",
    "ChannelTabsConfiguration",
    "ChatInputBarConfiguration",
    "ChatWindowConfiguration",
];

/// `Enum.ChatVersion.TextChatService`.
const TEXT_CHAT_SERVICE: u32 = 1;

fn service(dom: &WeakDom) -> Option<Ref> {
    dom.root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "TextChatService"))
}

fn child(dom: &WeakDom, parent: Ref, name: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))
}

fn ensure(dom: &mut WeakDom, parent: Ref, class: &str, name: &str) -> Ref {
    child(dom, parent, name).unwrap_or_else(|| dom.new_instance(class, name, Some(parent)))
}

/// The general channel, when the place uses TextChatService's.
fn general(dom: &WeakDom) -> Option<Ref> {
    let channels = child(dom, service(dom)?, "TextChannels")?;
    child(dom, channels, "RBXGeneral")
}

/// Makes what a server's TextChatService starts with, when the place's
/// chat is TextChatService.
pub(crate) fn setup(dom: &mut WeakDom, db: &ReflectionDatabase) {
    let chat = service(dom)
        .unwrap_or_else(|| dom.new_instance("TextChatService", "TextChatService", None));
    // A place that doesn't say is a new place, which Roblox gives
    // TextChatService (the reflection default is the legacy chat).
    let version = match dom
        .get(chat)
        .and_then(|i| i.properties().get("ChatVersion"))
    {
        Some(Variant::Enum(v)) => *v,
        _ => TEXT_CHAT_SERVICE,
    };
    if version != TEXT_CHAT_SERVICE {
        return;
    }
    if flag(dom, db, chat, "CreateDefaultTextChannels") {
        let channels = ensure(dom, chat, "Folder", "TextChannels");
        for name in ["RBXGeneral", "RBXSystem"] {
            ensure(dom, channels, "TextChannel", name);
        }
    }
    for class in CONFIGS {
        ensure(dom, chat, class, class);
    }
    if flag(dom, db, chat, "CreateDefaultCommands") {
        ensure(dom, chat, "Folder", "TextChatCommands");
    }
}

/// A player joined: their `TextSource` in the general channel.
pub(crate) fn add_source(dom: &mut WeakDom, name: &str, user_id: i64) {
    let Some(general) = general(dom) else { return };
    let source = dom.new_instance("TextSource", name, Some(general));
    let _ = dom.set_property(source, "UserId", Variant::Int64(user_id));
    let _ = dom.set_property(source, "CanSend", Variant::Bool(true));
}

/// A player's `TextSource`s, to go when they leave.
pub(crate) fn sources(dom: &WeakDom, user_id: i64) -> Vec<Ref> {
    let Some(general) = general(dom) else {
        return Vec::new();
    };
    dom
        .get(general)
        .map(|g| g.children().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|c| {
            dom.get(*c).is_some_and(|i| {
                i.class() == "TextSource"
                    && matches!(i.properties().get("UserId"), Some(Variant::Int64(id)) if *id == user_id)
            })
        })
        .collect()
}
