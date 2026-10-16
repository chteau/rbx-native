//! `PluginAction` and `PluginMenu`.

use mlua::{
    AnyUserData, Lua, MetaMethod, Result, UserData, UserDataFields, UserDataMethods, Value,
};

use super::{with_state, Pending};
use crate::plugin::api::{Action, MenuItem, PromptKind, Request, UiId};
use crate::plugin::env::env;
use crate::plugin::scheduler;
use crate::plugin::signal::{LuaSignal, Owner, SignalKey};

const SHOW_KEY: &str = "__plugin_menu_show";

/// A menu's contents; actions live in `Shared::ui.actions`.
#[derive(Default)]
pub(super) struct Menu {
    title: String,
    icon: String,
    entries: Vec<Entry>,
}

enum Entry {
    Action(UiId),
    Separator,
    Submenu(UiId),
}

#[derive(Clone, Copy)]
pub(crate) struct LuaAction(pub(crate) UiId);

#[derive(Clone, Copy)]
pub(crate) struct LuaMenu(pub(crate) UiId);

/// `CreatePluginAction`: the same key twice returns the same action.
pub(super) fn create_action(
    lua: &Lua,
    key: String,
    text: String,
    tooltip: String,
    icon: Option<String>,
    allow_binding: Option<bool>,
) -> LuaAction {
    let env = env(lua);
    let mut shared = env.shared.borrow_mut();
    if let Some(action) = shared
        .ui
        .actions
        .iter()
        .find(|a| a.plugin == env.plugin && a.key == key)
    {
        return LuaAction(action.id);
    }
    let id = shared.next_id();
    shared.ui.actions.push(Action {
        id,
        plugin: env.plugin,
        key,
        text,
        tooltip,
        icon: icon.unwrap_or_default(),
        allow_binding: allow_binding.unwrap_or(true),
    });
    shared.owners.insert(id, env.plugin);
    shared.ui_changed = true;
    LuaAction(id)
}

pub(super) fn create_menu(lua: &Lua, title: Option<String>, icon: Option<String>) -> LuaMenu {
    let id = env(lua).shared.borrow_mut().next_id();
    with_state(lua, |state| {
        state.menus.insert(
            id,
            Menu {
                title: title.unwrap_or_default(),
                icon: icon.unwrap_or_default(),
                entries: Vec::new(),
            },
        )
    });
    LuaMenu(id)
}

fn action_field<T>(lua: &Lua, id: UiId, f: impl FnOnce(&Action) -> T) -> Option<T> {
    let env = env(lua);
    let shared = env.shared.borrow();
    shared.ui.actions.iter().find(|a| a.id == id).map(f)
}

impl UserData for LuaAction {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            let id = this.0;
            Ok(match key.as_str() {
                "Triggered" => Value::UserData(
                    lua.create_userdata(LuaSignal(SignalKey::new(Owner::Ui(id), "Triggered")))?,
                ),
                "Enabled" => Value::Boolean(!with_state(lua, |s| s.disabled_actions.contains(&id))),
                "ActionId" | "Name" => match action_field(lua, id, |a| a.key.clone()) {
                    Some(k) => Value::String(lua.create_string(&k)?),
                    None => Value::Nil,
                },
                "Text" => match action_field(lua, id, |a| a.text.clone()) {
                    Some(t) => Value::String(lua.create_string(&t)?),
                    None => Value::Nil,
                },
                "AllowBinding" => {
                    Value::Boolean(action_field(lua, id, |a| a.allow_binding).unwrap_or(false))
                }
                _ => {
                    return Err(mlua::Error::runtime(format!(
                        "{key} is not a valid member of PluginAction"
                    )))
                }
            })
        });
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |lua, this, (key, value): (String, Value)| match (key.as_str(), value) {
                ("Enabled", Value::Boolean(enabled)) => {
                    with_state(lua, |s| {
                        if enabled {
                            s.disabled_actions.remove(&this.0);
                        } else {
                            s.disabled_actions.insert(this.0);
                        }
                    });
                    Ok(())
                }
                _ => Err(mlua::Error::runtime(format!(
                    "{key} cannot be set on PluginAction"
                ))),
            },
        );
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: AnyUserData| {
            Ok(other.borrow::<LuaAction>().is_ok_and(|o| o.0 == this.0))
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("PluginAction"));
    }
}

fn items(lua: &Lua, menu: UiId, depth: usize) -> (String, Vec<MenuItem>) {
    let (title, entries): (String, Vec<Entry>) = with_state(lua, |s| match s.menus.get(&menu) {
        Some(m) => (
            m.title.clone(),
            m.entries
                .iter()
                .map(|e| match e {
                    Entry::Action(id) => Entry::Action(*id),
                    Entry::Separator => Entry::Separator,
                    Entry::Submenu(id) => Entry::Submenu(*id),
                })
                .collect(),
        ),
        None => (String::new(), Vec::new()),
    });
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            Entry::Action(id) => {
                if let Some((text, icon)) =
                    action_field(lua, id, |a| (a.text.clone(), a.icon.clone()))
                {
                    out.push(MenuItem::Action { id, text, icon });
                }
            }
            Entry::Separator => out.push(MenuItem::Separator),
            // A menu added to itself (directly or not) would never end.
            Entry::Submenu(id) if depth < 8 => {
                let (title, items) = items(lua, id, depth + 1);
                out.push(MenuItem::Submenu { title, items });
            }
            Entry::Submenu(_) => {}
        }
    }
    (title, out)
}

fn show_function(lua: &Lua) -> Result<mlua::Function> {
    if let Some(f) = lua.named_registry_value::<Option<mlua::Function>>(SHOW_KEY)? {
        return Ok(f);
    }
    let start = lua.create_function(|lua, this: AnyUserData| {
        let menu = this.borrow::<LuaMenu>()?.0;
        let (title, items) = items(lua, menu, 0);
        let id = scheduler::new_await(lua);
        let env = env(lua);
        env.shared.borrow_mut().requests.push(Request::Prompt {
            id,
            plugin: env.plugin,
            kind: PromptKind::Menu { title, items },
        });
        with_state(lua, |s| s.prompts.insert(id, Pending::Menu));
        Ok(id)
    })?;
    let f = scheduler::yielding(lua, start)?;
    lua.set_named_registry_value(SHOW_KEY, f.clone())?;
    Ok(f)
}

fn push(lua: &Lua, menu: UiId, entry: Entry) {
    with_state(lua, |s| {
        if let Some(m) = s.menus.get_mut(&menu) {
            m.entries.push(entry);
        }
    });
}

impl UserData for LuaMenu {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("AddAction", |lua, this, action: AnyUserData| {
            let id = action.borrow::<LuaAction>()?.0;
            push(lua, this.0, Entry::Action(id));
            Ok(())
        });
        methods.add_method(
            "AddNewAction",
            |lua, this, (key, text, icon): (String, String, Option<String>)| {
                let action = create_action(lua, key, text.clone(), text, icon, Some(false));
                push(lua, this.0, Entry::Action(action.0));
                Ok(action)
            },
        );
        methods.add_method("AddSeparator", |lua, this, ()| {
            push(lua, this.0, Entry::Separator);
            Ok(())
        });
        methods.add_method("AddMenu", |lua, this, menu: AnyUserData| {
            let id = menu.borrow::<LuaMenu>()?.0;
            push(lua, this.0, Entry::Submenu(id));
            Ok(())
        });
        methods.add_method("Clear", |lua, this, ()| {
            with_state(lua, |s| {
                if let Some(m) = s.menus.get_mut(&this.0) {
                    m.entries.clear();
                }
            });
            Ok(())
        });
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            let id = this.0;
            Ok(match key.as_str() {
                "ShowAsync" => Value::Function(show_function(lua)?),
                "Title" | "Icon" => {
                    let text = with_state(lua, |s| {
                        s.menus
                            .get(&id)
                            .map(|m| {
                                if key == "Title" {
                                    m.title.clone()
                                } else {
                                    m.icon.clone()
                                }
                            })
                            .unwrap_or_default()
                    });
                    Value::String(lua.create_string(&text)?)
                }
                _ => {
                    return Err(mlua::Error::runtime(format!(
                        "{key} is not a valid member of PluginMenu"
                    )))
                }
            })
        });
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |lua, this, (key, value): (String, String)| {
                with_state(lua, |s| match (key.as_str(), s.menus.get_mut(&this.0)) {
                    ("Title", Some(m)) => {
                        m.title = value;
                        Ok(())
                    }
                    ("Icon", Some(m)) => {
                        m.icon = value;
                        Ok(())
                    }
                    _ => Err(mlua::Error::runtime(format!(
                        "{key} cannot be set on PluginMenu"
                    ))),
                })
            },
        );
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("PluginMenu"));
    }
}
