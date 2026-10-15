//! The `plugin` global and what it creates: toolbars and buttons, plugin
//! actions and menus, dock widgets, settings, and rbx-native's additions.

mod menu;
mod widget;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use mlua::{
    AnyUserData, Function, Lua, MetaMethod, MultiValue, Result, Table, UserData, UserDataMethods,
    Value,
};

use super::api::{Answer, Arg, Button, PromptKind, Request, Toolbar, UiId};
use super::arg;
use super::debugger_api;
use super::env::env;
use super::scheduler;
use super::signal::{LuaSignal, Owner, SignalKey};
use crate::instance::LuaInstance;

/// The `plugin` object. Members:
/// - `Name`; `CreateToolbar(name)` → toolbar (one per name per plugin);
///   toolbar `CreateButton(id, tooltip, icon, text?)` → button with
///   `Click` signal, `SetActive(bool)`, settable `Enabled`, `Icon`,
///   `ClickableWhenViewportHidden`;
/// - `CreatePluginAction(id, text, tooltip, icon?, allowBinding?)` →
///   action with `Triggered`, `Enabled`; `CreatePluginMenu(id, title?,
///   icon?)` → menu with `AddAction`, `AddNewAction(id, text, icon?)`,
///   `AddSeparator`, `AddMenu`, `Clear`, `Title`, `Icon`, yielding
///   `ShowAsync()` (→ `PromptKind::Menu`, returns the chosen action or nil);
/// - `CreateDockWidgetPluginGui(id, info)` (and `CreateDockWidgetPluginGuiAsync`,
///   its current name — the same, without yielding) → a `DockWidgetPluginGui`
///   instance under `Env::plugin_gui_service` (`Title` = id until set,
///   `Enabled` from the info), registered as a `Widget` (same id twice
///   returns the same widget); global `DockWidgetPluginGuiInfo.new(
///   initDockState?, initEnabled?, overrideRestore?, floatX?, floatY?,
///   minW?, minH?)`;
/// - `GetSetting(key)`, `SetSetting(key, value)` (JSON-able values only,
///   persisted to `Shared::settings_dir/<plugin name>.json`);
/// - `Activate(exclusive)`, `Deactivate()`, `IsActivated()`,
///   `IsActivatedWithExclusiveMouse()`, `Deactivation` and `Unloading`
///   signals; `GetMouse()` → `PluginMouse` (`Hit` CFrame, `Target`,
///   `Origin`, `UnitRay`-free, from `Context::mouse_*`; `Button1Down`/
///   `Button1Up`/`Move` signals that never fire yet);
/// - `OpenScript(script, line?)` → `Request::OpenScript`;
///   `SelectRibbonTool`, `OpenWikiPage`, `StartDrag` → no-ops;
///   `GetStudioUserId()` 0; `Union`/`Separate`/`Negate` → error naming
///   the missing CSG support;
/// - rbx-native additions: yielding `PromptColor3(initial?)` → Color3 or
///   nil, yielding `PromptBrickColor(initial?)` → BrickColor or nil,
///   `CreateFloatingWidget(id, {Size: Vector2, Position: Vector2?,
///   LightDismiss: bool?})` → a widget instance of kind `Floating`,
///   `GetDebuggerManager()` → `debugger_api::manager`.
pub(crate) struct LuaPlugin;

/// What a prompt this VM is waiting on resolves to.
#[derive(Clone, Copy)]
enum Pending {
    Color3,
    BrickColor,
    Menu,
}

/// Per-VM state that has no place in `Shared`.
#[derive(Default)]
struct State {
    prompts: HashMap<UiId, Pending>,
    menus: HashMap<UiId, menu::Menu>,
    disabled_actions: HashSet<UiId>,
    /// `Some(exclusive)` while activated.
    active: Option<bool>,
}

struct StateSlot(RefCell<State>);

/// Never call into Luau from `f`: the state stays borrowed for its run.
fn with_state<R>(lua: &Lua, f: impl FnOnce(&mut State) -> R) -> R {
    if lua.app_data_ref::<StateSlot>().is_none() {
        lua.set_app_data(StateSlot(RefCell::new(State::default())));
    }
    let slot = lua
        .app_data_ref::<StateSlot>()
        .expect("state slot was just set");
    let mut state = slot.0.borrow_mut();
    f(&mut state)
}

#[derive(Clone, Copy)]
struct LuaToolbar(UiId);

#[derive(Clone, Copy)]
struct LuaButton(UiId);

struct LuaPluginMouse;

fn toolbar(lua: &Lua, name: &str) -> UiId {
    let env = env(lua);
    let mut shared = env.shared.borrow_mut();
    if let Some(t) = shared
        .ui
        .toolbars
        .iter()
        .find(|t| t.plugin == env.plugin && t.name == name)
    {
        return t.id;
    }
    let id = shared.next_id();
    shared.ui.toolbars.push(Toolbar {
        id,
        plugin: env.plugin,
        name: name.to_string(),
        buttons: Vec::new(),
    });
    shared.ui_changed = true;
    id
}

fn button(
    lua: &Lua,
    toolbar: UiId,
    key: &str,
    tooltip: &str,
    icon: &str,
    text: Option<&str>,
) -> UiId {
    let env = env(lua);
    let mut shared = env.shared.borrow_mut();
    let id = shared.next_id();
    let Some(bar) = shared.ui.toolbars.iter_mut().find(|t| t.id == toolbar) else {
        return id;
    };
    if let Some(b) = bar.buttons.iter().find(|b| b.key == key) {
        return b.id;
    }
    bar.buttons.push(Button {
        id,
        key: key.to_string(),
        tooltip: tooltip.to_string(),
        icon: icon.to_string(),
        text: text.unwrap_or(key).to_string(),
        active: false,
        enabled: true,
        clickable_when_viewport_hidden: false,
    });
    shared.owners.insert(id, env.plugin);
    shared.ui_changed = true;
    id
}

fn with_button<R>(lua: &Lua, id: UiId, f: impl FnOnce(&mut Button) -> R) -> Result<R> {
    let env = env(lua);
    let mut shared = env.shared.borrow_mut();
    let found = shared
        .ui
        .toolbars
        .iter_mut()
        .flat_map(|t| t.buttons.iter_mut())
        .find(|b| b.id == id)
        .map(f);
    if found.is_some() {
        shared.ui_changed = true;
    }
    found.ok_or_else(|| mlua::Error::runtime("the button no longer exists"))
}

impl UserData for LuaToolbar {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(
            "CreateButton",
            |lua,
             this,
             (key, tooltip, icon, text): (
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            )| {
                let tooltip = tooltip.unwrap_or_default();
                let icon = icon.unwrap_or_default();
                Ok(LuaButton(button(
                    lua,
                    this.0,
                    &key,
                    &tooltip,
                    &icon,
                    text.as_deref(),
                )))
            },
        );
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("PluginToolbar"));
    }
}

impl UserData for LuaButton {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("SetActive", |lua, this, active: bool| {
            with_button(lua, this.0, |b| b.active = active)
        });
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            if key == "Click" {
                let signal = LuaSignal(SignalKey::new(Owner::Ui(this.0), "Click"));
                return Ok(Value::UserData(lua.create_userdata(signal)?));
            }
            let value = with_button(lua, this.0, |b| match key.as_str() {
                "Enabled" => Some(Arg::Bool(b.enabled)),
                "Icon" => Some(Arg::String(b.icon.clone())),
                "ClickableWhenViewportHidden" => Some(Arg::Bool(b.clickable_when_viewport_hidden)),
                "Name" => Some(Arg::String(b.key.clone())),
                _ => None,
            })?;
            match value {
                Some(value) => arg::to_lua(lua, &env(lua).ctx, &value),
                None => Err(mlua::Error::runtime(format!(
                    "{key} is not a valid member of PluginToolbarButton"
                ))),
            }
        });
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |lua, this, (key, value): (String, Value)| match (key.as_str(), value) {
                ("Enabled", Value::Boolean(v)) => with_button(lua, this.0, |b| b.enabled = v),
                ("ClickableWhenViewportHidden", Value::Boolean(v)) => {
                    with_button(lua, this.0, |b| b.clickable_when_viewport_hidden = v)
                }
                ("Icon", Value::String(s)) => {
                    let icon = s.to_string_lossy().to_string();
                    with_button(lua, this.0, |b| b.icon = icon)
                }
                _ => Err(mlua::Error::runtime(format!(
                    "{key} cannot be set on PluginToolbarButton"
                ))),
            },
        );
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("PluginToolbarButton"));
    }
}

impl UserData for LuaPluginMouse {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, _, key: String| {
            let env = env(lua);
            let context = env.shared.borrow().context.clone();
            let cframe = |p: [f32; 3]| -> Result<Value> {
                let new: Function = lua.globals().get::<Table>("CFrame")?.get("new")?;
                new.call((p[0], p[1], p[2]))
            };
            match key.as_str() {
                "Hit" => cframe(context.mouse_hit.unwrap_or_default()),
                "Origin" => cframe([0.0; 3]),
                "Target" => match context.mouse_target {
                    Some(r) => arg::to_lua(lua, &env.ctx, &Arg::Instance(r)),
                    None => Ok(Value::Nil),
                },
                "Button1Down" | "Button1Up" | "Move" => {
                    let signal = LuaSignal(SignalKey::new(Owner::Plugin, format!("Mouse.{key}")));
                    Ok(Value::UserData(lua.create_userdata(signal)?))
                }
                _ => Err(mlua::Error::runtime(format!(
                    "{key} is not a valid member of PluginMouse"
                ))),
            }
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("PluginMouse"));
    }
}

fn json_to_arg(value: &serde_json::Value) -> Arg {
    match value {
        serde_json::Value::Null => Arg::Nil,
        serde_json::Value::Bool(b) => Arg::Bool(*b),
        serde_json::Value::Number(n) => Arg::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Arg::String(s.clone()),
        serde_json::Value::Array(items) => Arg::List(items.iter().map(json_to_arg).collect()),
        serde_json::Value::Object(map) => serialized_to_arg(map).unwrap_or_else(|| {
            Arg::Table(
                map.iter()
                    .map(|(k, v)| (k.clone(), json_to_arg(v)))
                    .collect(),
            )
        }),
    }
}

/// Roblox values in a settings file the way Studio writes them: an object
/// tagged `serializedType` — `{"name": "Grass", "serializedType":
/// "Material"}` for an enum item, `{"x", "y", "z", "serializedType":
/// "Vector3"}` and the like (read from Studio's own plugin settings).
fn serialized_to_arg(map: &serde_json::Map<String, serde_json::Value>) -> Option<Arg> {
    let kind = map.get("serializedType")?.as_str()?;
    let number = |key: &str| {
        map.get(key)
            .and_then(serde_json::Value::as_f64)
            .map(|n| n as f32)
    };
    Some(match kind {
        "Vector3" => Arg::Vector3([number("x")?, number("y")?, number("z")?]),
        "Vector2" => Arg::Vector2([number("x")?, number("y")?]),
        "Color3" => Arg::Color3([number("r")?, number("g")?, number("b")?]),
        enum_name => Arg::Enum {
            enum_name: enum_name.to_string(),
            item: map.get("name")?.as_str()?.to_string(),
        },
    })
}

fn tagged(kind: &str, fields: &[(&str, f32)]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "serializedType".into(),
        serde_json::Value::String(kind.into()),
    );
    for (key, value) in fields {
        map.insert((*key).into(), serde_json::json!(f64::from(*value)));
    }
    serde_json::Value::Object(map)
}

fn arg_to_json(value: &Arg) -> Result<serde_json::Value> {
    Ok(match value {
        Arg::Nil => serde_json::Value::Null,
        Arg::Bool(b) => serde_json::Value::Bool(*b),
        Arg::Number(n) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .ok_or_else(|| mlua::Error::runtime("SetSetting cannot store NaN or infinity"))?,
        Arg::String(s) => serde_json::Value::String(s.clone()),
        Arg::List(items) => {
            serde_json::Value::Array(items.iter().map(arg_to_json).collect::<Result<_>>()?)
        }
        Arg::Table(entries) => serde_json::Value::Object(
            entries
                .iter()
                .map(|(k, v)| Ok((k.clone(), arg_to_json(v)?)))
                .collect::<Result<_>>()?,
        ),
        Arg::Enum { enum_name, item } => serde_json::json!({
            "name": item,
            "serializedType": enum_name,
        }),
        Arg::Vector3([x, y, z]) => tagged("Vector3", &[("x", *x), ("y", *y), ("z", *z)]),
        Arg::Vector2([x, y]) => tagged("Vector2", &[("x", *x), ("y", *y)]),
        Arg::Color3([r, g, b]) => tagged("Color3", &[("r", *r), ("g", *g), ("b", *b)]),
        _ => {
            return Err(mlua::Error::runtime(
                "SetSetting only stores JSON-compatible values",
            ))
        }
    })
}

fn set_setting(lua: &Lua, key: String, value: Value) -> Result<()> {
    let json = arg_to_json(&arg::from_lua(&value))?;
    let env = env(lua);
    let mut shared = env.shared.borrow_mut();
    let map = shared.settings.entry(env.name.clone()).or_default();
    map.insert(key, json);
    let text = serde_json::to_string_pretty(map).map_err(mlua::Error::external)?;
    if let Some(dir) = shared.settings_dir.clone() {
        std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(dir.join(format!("{}.json", env.name)), text))
            .map_err(|e| mlua::Error::runtime(format!("could not save plugin settings: {e}")))?;
    }
    Ok(())
}

fn prompt_function(lua: &Lua, pending: Pending) -> Result<Function> {
    let registry = match pending {
        Pending::Color3 => "__plugin_prompt_color3",
        Pending::BrickColor => "__plugin_prompt_brickcolor",
        Pending::Menu => "__plugin_prompt_menu",
    };
    if let Some(f) = lua.named_registry_value::<Option<Function>>(registry)? {
        return Ok(f);
    }
    let start = lua.create_function(move |lua, (_this, initial): (Value, Value)| {
        let kind = match (pending, arg::from_lua(&initial)) {
            (Pending::Color3, Arg::Color3(c)) => PromptKind::Color3 { initial: Some(c) },
            (Pending::Color3, _) => PromptKind::Color3 { initial: None },
            (_, Arg::BrickColor(n)) => PromptKind::BrickColor { initial: Some(n) },
            _ => PromptKind::BrickColor { initial: None },
        };
        let id = scheduler::new_await(lua);
        let env = env(lua);
        env.shared.borrow_mut().requests.push(Request::Prompt {
            id,
            plugin: env.plugin,
            kind,
        });
        with_state(lua, |s| s.prompts.insert(id, pending));
        Ok(id)
    })?;
    let f = scheduler::yielding(lua, start)?;
    lua.set_named_registry_value(registry, f.clone())?;
    Ok(f)
}

fn signal(lua: &Lua, event: &str) -> Result<Value> {
    Ok(Value::UserData(lua.create_userdata(LuaSignal(
        SignalKey::new(Owner::Plugin, event),
    ))?))
}

impl UserData for LuaPlugin {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("CreateToolbar", |lua, _, name: String| {
            Ok(LuaToolbar(toolbar(lua, &name)))
        });
        methods.add_method(
            "CreatePluginAction",
            |lua,
             _,
             (key, text, tooltip, icon, binding): (
                String,
                String,
                Option<String>,
                Option<String>,
                Option<bool>,
            )| {
                Ok(menu::create_action(
                    lua,
                    key,
                    text,
                    tooltip.unwrap_or_default(),
                    icon,
                    binding,
                ))
            },
        );
        methods.add_method(
            "CreatePluginMenu",
            |lua, _, (_key, title, icon): (Value, Option<String>, Option<String>)| {
                Ok(menu::create_menu(lua, title, icon))
            },
        );
        for name in [
            "CreateDockWidgetPluginGui",
            "CreateDockWidgetPluginGuiAsync",
        ] {
            methods.add_method(
                name,
                |lua, _, (key, info): (String, Option<AnyUserData>)| {
                    widget::create_dock(lua, key, info)
                },
            );
        }
        methods.add_method(
            "CreateFloatingWidget",
            |lua, _, (key, options): (String, Option<Table>)| {
                widget::create_floating(lua, key, options)
            },
        );
        methods.add_method("GetSetting", |lua, _, key: String| {
            let env = env(lua);
            let value = env
                .shared
                .borrow()
                .settings
                .get(&env.name)
                .and_then(|m| m.get(&key))
                .map(json_to_arg)
                .unwrap_or(Arg::Nil);
            arg::to_lua(lua, &env.ctx, &value)
        });
        methods.add_method("SetSetting", |lua, _, (key, value): (String, Value)| {
            set_setting(lua, key, value)
        });
        methods.add_method("Activate", |lua, _, exclusive: Option<bool>| {
            with_state(lua, |s| s.active = Some(exclusive.unwrap_or(false)));
            Ok(())
        });
        methods.add_method("Deactivate", |lua, _, ()| {
            if with_state(lua, |s| s.active.take()).is_some() {
                super::signal::fire(lua, &SignalKey::new(Owner::Plugin, "Deactivation"), &[])?;
            }
            Ok(())
        });
        methods.add_method("IsActivated", |lua, _, ()| {
            Ok(with_state(lua, |s| s.active.is_some()))
        });
        methods.add_method("IsActivatedWithExclusiveMouse", |lua, _, ()| {
            Ok(with_state(lua, |s| s.active == Some(true)))
        });
        methods.add_method("GetMouse", |_, _, ()| Ok(LuaPluginMouse));
        methods.add_method(
            "OpenScript",
            |lua, _, (script, line): (AnyUserData, Option<u32>)| {
                let script = script.borrow::<LuaInstance>()?.referent();
                env(lua)
                    .shared
                    .borrow_mut()
                    .requests
                    .push(Request::OpenScript { script, line });
                Ok(())
            },
        );
        for name in ["SelectRibbonTool", "OpenWikiPage", "StartDrag"] {
            methods.add_method(name, |_, _, _: MultiValue| Ok(()));
        }
        methods.add_method("GetStudioUserId", |_, _, ()| Ok(0));
        // Studio's "Join Surfaces" toggle, off by default; rbx-native has no
        // joints to make.
        methods.add_method("GetJoinMode", |lua, _, ()| {
            let ctx = env(lua).ctx.clone();
            crate::plugin::input::enum_item(&ctx, "JointCreationMode", "None")?.interned(lua)
        });
        for name in ["Union", "Separate", "Negate"] {
            methods.add_method(name, move |_, _, _: MultiValue| -> Result<()> {
                Err(mlua::Error::runtime(format!(
                    "plugin:{name} is unavailable: rbx-native cannot run CSG operations from plugins"
                )))
            });
        }
        methods.add_method("GetDebuggerManager", |lua, _, ()| {
            debugger_api::manager(lua)
        });
        methods.add_meta_method(MetaMethod::Index, |lua, _, key: String| {
            match key.as_str() {
                "Name" => Ok(Value::String(lua.create_string(&env(lua).name)?)),
                "Deactivation" | "Unloading" => signal(lua, &key),
                "PromptColor3" => prompt_function(lua, Pending::Color3).map(Value::Function),
                "PromptBrickColor" => {
                    prompt_function(lua, Pending::BrickColor).map(Value::Function)
                }
                _ => Err(mlua::Error::runtime(format!(
                    "{key} is not a valid member of Plugin"
                ))),
            }
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("Plugin"));
    }
}

/// Installs `plugin` and `DockWidgetPluginGuiInfo`. Call before sandboxing.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    let globals = lua.globals();
    globals.set("plugin", LuaPlugin)?;
    globals.set("DockWidgetPluginGuiInfo", widget::info_global(lua)?)?;
    Ok(())
}

/// Finds or creates this plugin's toolbar `toolbar` and a button `key` on
/// it (the same key twice returns the same button), as `CreateButton`
/// does; its `Click` signal is `SignalKey::new(Owner::Ui(id), "Click")`.
/// For `rbxnative.command`.
pub(crate) fn toolbar_button(
    lua: &Lua,
    toolbar: &str,
    key: &str,
    tooltip: &str,
    icon: &str,
    text: &str,
) -> Result<UiId> {
    let bar = self::toolbar(lua, toolbar);
    Ok(button(lua, bar, key, tooltip, icon, Some(text)))
}

/// Resolves a `PromptColor3`/`PromptBrickColor`/`ShowAsync` prompt this VM
/// is waiting on. Returns false when the id is not one of its prompts.
pub(crate) fn answer(lua: &Lua, prompt: UiId, answer: &Answer) -> Result<bool> {
    let Some(pending) = with_state(lua, |s| s.prompts.remove(&prompt)) else {
        return Ok(false);
    };
    let value = match (pending, answer) {
        (Pending::Color3, Answer::Color3(Some(c))) => {
            arg::to_lua(lua, &env(lua).ctx, &Arg::Color3(*c))?
        }
        (Pending::BrickColor, Answer::BrickColor(Some(n))) => {
            arg::to_lua(lua, &env(lua).ctx, &Arg::BrickColor(*n))?
        }
        (Pending::Menu, Answer::Menu(Some(id))) => {
            Value::UserData(lua.create_userdata(menu::LuaAction(*id))?)
        }
        _ => Value::Nil,
    };
    scheduler::resolve(lua, prompt, Ok(MultiValue::from_vec(vec![value])))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use rbx_dom::WeakDom;

    use super::*;
    use crate::plugin::api::WidgetKind;
    use crate::plugin::testing::test_vm;

    fn vm() -> (
        Lua,
        std::rc::Rc<crate::plugin::env::Env>,
        std::rc::Rc<RefCell<WeakDom>>,
    ) {
        let (lua, env, dom) = test_vm(WeakDom::new(), false);
        install(&lua).unwrap();
        (lua, env, dom)
    }

    fn run(lua: &Lua, src: &str) {
        scheduler::spawn(
            lua,
            lua.load(src).into_function().unwrap(),
            MultiValue::new(),
        )
        .unwrap();
    }

    fn errors(env: &crate::plugin::env::Env) -> Vec<String> {
        env.shared
            .borrow()
            .output
            .iter()
            .map(|l| l.text.clone())
            .collect()
    }

    fn last_prompt(env: &crate::plugin::env::Env) -> (UiId, PromptKind) {
        match env.shared.borrow().requests.last() {
            Some(Request::Prompt { id, kind, .. }) => (*id, kind.clone()),
            other => panic!("no prompt: {other:?}"),
        }
    }

    #[test]
    fn toolbar_and_button_in_ui() {
        let (lua, env, _) = vm();
        run(
            &lua,
            "local t = plugin:CreateToolbar('Tools') b = t:CreateButton('go', 'Go now', 'rbxassetid://1', 'Go')
             local t2 = plugin:CreateToolbar('Tools') b2 = t2:CreateButton('other', 'tip', '')
             b:SetActive(true) b.Enabled = false",
        );
        assert!(errors(&env).is_empty(), "{:?}", errors(&env));
        let shared = env.shared.borrow();
        assert_eq!(shared.ui.toolbars.len(), 1);
        let bar = &shared.ui.toolbars[0];
        assert_eq!(bar.name, "Tools");
        assert_eq!(bar.buttons.len(), 2);
        let b = &bar.buttons[0];
        assert_eq!(
            (b.icon.as_str(), b.text.as_str(), b.tooltip.as_str()),
            ("rbxassetid://1", "Go", "Go now")
        );
        assert!(b.active && !b.enabled);
        assert_eq!(bar.buttons[1].text, "other");
        assert!(shared.ui_changed);
    }

    #[test]
    fn dock_widget_created_once() {
        let (lua, env, dom) = vm();
        run(
            &lua,
            "local info = DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Float, true, false, 300, 200, 150, 100)
             w = plugin:CreateDockWidgetPluginGui('Panel', info)
             w2 = plugin:CreateDockWidgetPluginGui('Panel', info)",
        );
        assert!(errors(&env).is_empty(), "{:?}", errors(&env));
        let shared = env.shared.borrow();
        assert_eq!(shared.ui.widgets.len(), 1);
        let w = &shared.ui.widgets[0];
        assert_eq!(
            w.kind,
            WidgetKind::Dock {
                initial: 4,
                float_size: (300.0, 200.0),
                min_size: (150.0, 100.0)
            }
        );
        assert!(w.enabled);
        assert!(shared.widget_roots.contains(&w.root));
        let dom = dom.borrow();
        assert!(dom.get(w.root).is_some());
        assert_eq!(dom.parent(w.root), Some(env.plugin_gui_service));
    }

    #[test]
    fn settings_persist() {
        let (lua, env, _) = vm();
        let dir =
            std::env::temp_dir().join(format!("rbx_lua_plugin_settings_{}", std::process::id()));
        env.shared.borrow_mut().settings_dir = Some(dir.clone());
        run(&lua, "plugin:SetSetting('size', 4) plugin:SetSetting('tags', {'a', 'b'}) got = plugin:GetSetting('size') missing = plugin:GetSetting('none')");
        assert_eq!(lua.globals().get::<f64>("got").unwrap(), 4.0);
        assert!(lua
            .globals()
            .get::<Option<f64>>("missing")
            .unwrap()
            .is_none());
        let text = std::fs::read_to_string(dir.join("Test.json")).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(saved["size"], serde_json::json!(4.0));
        assert_eq!(saved["tags"], serde_json::json!(["a", "b"]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prompt_color3_resumes_with_answer() {
        let (lua, env, _) = vm();
        run(&lua, "local c = plugin:PromptColor3() red = c.R");
        let (id, kind) = last_prompt(&env);
        assert_eq!(kind, PromptKind::Color3 { initial: None });
        assert!(lua.globals().get::<Option<f64>>("red").unwrap().is_none());
        assert!(!answer(&lua, id + 100, &Answer::Color3(None)).unwrap());
        assert!(answer(&lua, id, &Answer::Color3(Some([1.0, 0.0, 0.0]))).unwrap());
        assert!(errors(&env).is_empty(), "{:?}", errors(&env));
        assert_eq!(lua.globals().get::<f64>("red").unwrap(), 1.0);
    }

    #[test]
    fn menu_show_async_returns_choice() {
        let (lua, env, _) = vm();
        run(
            &lua,
            "local m = plugin:CreatePluginMenu('m', 'Menu') local a = m:AddNewAction('a', 'Alpha') m:AddSeparator()
             local chosen = m:ShowAsync() picked = chosen == a name = chosen.ActionId",
        );
        let (id, kind) = last_prompt(&env);
        let PromptKind::Menu { title, items } = kind else {
            panic!("not a menu")
        };
        assert_eq!(title, "Menu");
        let action = match &items[0] {
            crate::plugin::api::MenuItem::Action { id, text, .. } => {
                assert_eq!(text, "Alpha");
                *id
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(items[1], crate::plugin::api::MenuItem::Separator);
        answer(&lua, id, &Answer::Menu(Some(action))).unwrap();
        assert!(errors(&env).is_empty(), "{:?}", errors(&env));
        assert!(lua.globals().get::<bool>("picked").unwrap());
        assert_eq!(lua.globals().get::<String>("name").unwrap(), "a");
    }
}
