//! The services a place file has no instance for: `Selection`,
//! `ChangeHistoryService`, `HttpService`, `RunService`, `StudioService`,
//! `CollectionService`, `CoreGui` and `PluginGuiService` (the plugin's own
//! folders), plus the `settings()` global.

mod http_service;
mod palette;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use mlua::{
    FromLuaMulti, IntoLua, IntoLuaMulti, Lua, MetaMethod, Result, Table, UserData, UserDataFields,
    UserDataMethods, Value,
};
use rbx_dom::{Ref, Variant};

use super::api::{Answer, Request, UiId};
use super::env::env;
use super::http::HttpRequest;
use super::scheduler::AwaitId;
use super::signal::{LuaSignal, Owner, SignalKey};
use crate::datatypes::color3::LuaColor3;
use crate::instance::LuaInstance;

const SERVICES: [&str; 9] = [
    "UserInputService",
    "Selection",
    "ChangeHistoryService",
    "HttpService",
    "RunService",
    "StudioService",
    "CollectionService",
    "CoreGui",
    "PluginGuiService",
];

/// Per-VM service bookkeeping, kept as app data.
#[derive(Default)]
struct State {
    recordings: HashSet<String>,
    modes: HashMap<AwaitId, http_service::Mode>,
    /// Prompt id → (host, requests waiting on the answer).
    prompts: HashMap<UiId, (String, Vec<(AwaitId, HttpRequest)>)>,
}

fn state(lua: &Lua) -> Rc<RefCell<State>> {
    if let Some(existing) = lua.app_data_ref::<Rc<RefCell<State>>>() {
        return existing.clone();
    }
    let fresh = Rc::new(RefCell::new(State::default()));
    lua.set_app_data(fresh.clone());
    fresh
}

struct LuaService(&'static str);

impl UserData for LuaService {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            index(lua, this.0, &key)
        });
        // `UserInputService.MouseBehavior = …` and the like: settings the
        // editor's own pointer handling keeps, accepted and dropped.
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |_, this, (key, _value): (String, Value)| match (this.0, key.as_str()) {
                (
                    "UserInputService",
                    "MouseBehavior" | "MouseIconEnabled" | "MouseIcon" | "MouseDeltaSensitivity",
                ) => Ok(()),
                (service, key) => Err(mlua::Error::runtime(format!(
                    "{key} cannot be set on {service}"
                ))),
            },
        );
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.0));
    }
}

/// `Players` members the host answers itself (see
/// `http_service::players_member`).
pub(crate) fn players_member(lua: &Lua, key: &str) -> Result<Option<Value>> {
    http_service::players_member(lua, key)
}

/// `MarketplaceService` members the host answers (see
/// `http_service::marketplace_member`).
pub(crate) fn marketplace_member(lua: &Lua, key: &str) -> Result<Option<Value>> {
    http_service::marketplace_member(lua, key)
}

/// `game:GetService(name)` for the services above (one userdata per VM per
/// service, cached in the Lua registry); `None` for anything else, which
/// then falls back to the place's own instances.
///
/// - `Selection`: `Get()`, `Set(list)`, `Add(list)`, `Remove(list)`,
///   `SelectionChanged`. Reads `Shared::context.selection` (dropping refs no
///   longer in the DOM); writes it back and sets `Shared::selection_written`.
///   Nothing fires from inside the write: the host fires `SelectionChanged`
///   once the input is handled, so a `Set` inside a handler cannot re-enter.
/// - `ChangeHistoryService`: waypoints and undo/redo as [`Request`]s;
///   recordings are tracked per VM.
/// - `HttpService`: see `http_service`. A Roblox plugin's first request to a
///   host waits on a prompt; a native plugin needs `Permission::Http`.
/// - `RunService`: edit-mode flags and the frame signals the host fires.
/// - `StudioService`, `CollectionService` (over the `Tags` property) and the
///   plugin's own `CoreGui` / `PluginGuiService` folders.
pub(crate) fn service(lua: &Lua, name: &str) -> Result<Option<Value>> {
    let Some(&canon) = SERVICES.iter().find(|n| **n == name) else {
        return super::world::service(lua, name);
    };
    let key = format!("rbxn.service.{canon}");
    if let Some(cached) = lua.named_registry_value::<Option<Value>>(&key)? {
        return Ok(Some(cached));
    }
    let env = env(lua);
    let value = match canon {
        "CoreGui" => LuaInstance::new(env.core_gui, env.ctx.clone()).into_lua(lua)?,
        "PluginGuiService" => {
            LuaInstance::new(env.plugin_gui_service, env.ctx.clone()).into_lua(lua)?
        }
        _ => Value::UserData(lua.create_userdata(LuaService(canon))?),
    };
    lua.set_named_registry_value(&key, value.clone())?;
    Ok(Some(value))
}

fn func<A: FromLuaMulti, R: IntoLuaMulti>(
    lua: &Lua,
    f: impl Fn(&Lua, A) -> Result<R> + 'static,
) -> Result<Value> {
    Ok(Value::Function(lua.create_function(f)?))
}

fn signal(lua: &Lua, owner: &'static str, event: &str) -> Result<Value> {
    Ok(Value::UserData(lua.create_userdata(LuaSignal(
        SignalKey::new(Owner::Service(owner), event),
    ))?))
}

fn no_member(service: &str, key: &str) -> mlua::Error {
    mlua::Error::runtime(format!("{key} is not a valid member of {service}"))
}

fn index(lua: &Lua, service: &'static str, key: &str) -> Result<Value> {
    match service {
        "Selection" => selection(lua, key),
        "ChangeHistoryService" => change_history(lua, key),
        "HttpService" => http_service::index(lua, key),
        "RunService" => run_service(lua, key),
        "StudioService" => studio_service(lua, key),
        "UserInputService" => user_input_service(lua, key),
        _ => collection_service(lua, key),
    }
}

fn user_input_service(lua: &Lua, key: &str) -> Result<Value> {
    let item = |lua: &Lua, enum_name: &str, name: &str| -> Result<Value> {
        let ctx = env(lua).ctx.clone();
        super::input::enum_item(&ctx, enum_name, name)?.interned(lua)
    };
    if key == "GetStringForKeyCode" {
        // The key's own label: Roblox gives the character a key types on
        // a US layout, which for letters and digits is the item's name.
        return func(lua, |lua, (_, code): (Value, Value)| {
            let name = match &code {
                Value::UserData(u) => mlua::ObjectLike::get::<String>(u, "Name")?,
                _ => String::new(),
            };
            let digits = [
                "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
            ];
            let label = match digits.iter().position(|d| *d == name) {
                Some(n) => n.to_string(),
                None => name,
            };
            lua.create_string(label)
        });
    }
    match key {
        "InputBegan"
        | "InputChanged"
        | "InputEnded"
        | "WindowFocused"
        | "WindowFocusReleased"
        | "TextBoxFocused"
        | "TextBoxFocusReleased" => signal(lua, "UserInputService", key),
        "GetMouseLocation" => func(lua, |lua, _: Value| {
            let ((x, y), _) = super::events::pointer_state(lua);
            lua.create_userdata(crate::datatypes::misc::LuaVector2(rbx_dom::Vector2Data {
                x,
                y,
            }))
        }),
        "IsKeyDown" => func(lua, |_, _: MultiValueRest| Ok(false)),
        "IsMouseButtonPressed" => func(lua, |lua, (_, button): (Value, Value)| {
            let (_, pressed) = super::events::pointer_state(lua);
            let text = button.to_string()?;
            Ok(match text.rsplit('.').next() {
                Some("MouseButton1") => pressed[0],
                Some("MouseButton2") => pressed[1],
                _ => false,
            })
        }),
        "GetFocusedTextBox" => func(lua, |lua, _: Value| {
            Ok(super::events::focused_text_box(lua)
                .map(|r| LuaInstance::new(r, env(lua).ctx.clone())))
        }),
        "MouseEnabled" | "KeyboardEnabled" | "MouseIconEnabled" => Ok(Value::Boolean(true)),
        "TouchEnabled" | "GamepadEnabled" | "VREnabled" => Ok(Value::Boolean(false)),
        "MouseBehavior" => item(lua, "MouseBehavior", "Default"),
        _ => Err(no_member("UserInputService", key)),
    }
}

fn instance_ref(value: &Value) -> Result<Ref> {
    value
        .as_userdata()
        .and_then(|u| u.borrow::<LuaInstance>().ok())
        .map(|i| i.referent())
        .ok_or_else(|| mlua::Error::runtime("Instance expected"))
}

fn refs_of(list: &Table) -> Result<Vec<Ref>> {
    list.sequence_values::<Value>()
        .map(|v| instance_ref(&v?))
        .collect()
}

fn current_selection(lua: &Lua) -> Vec<Ref> {
    let env = env(lua);
    let selection = env.shared.borrow().context.selection.clone();
    let dom = env.ctx.dom();
    selection
        .into_iter()
        .filter(|r| dom.get(*r).is_some())
        .collect()
}

fn write_selection(lua: &Lua, refs: Vec<Ref>) {
    let env = env(lua);
    let mut shared = env.shared.borrow_mut();
    shared.context.selection = refs;
    shared.selection_written = true;
}

fn instances_table(lua: &Lua, refs: &[Ref]) -> Result<Table> {
    let env = env(lua);
    let table = lua.create_table()?;
    for (i, r) in refs.iter().enumerate() {
        table.raw_set(i + 1, LuaInstance::new(*r, env.ctx.clone()))?;
    }
    Ok(table)
}

fn selection(lua: &Lua, key: &str) -> Result<Value> {
    match key {
        "Get" => func(lua, |lua, _: Value| {
            instances_table(lua, &current_selection(lua))
        }),
        "Set" => func(lua, |lua, (_, list): (Value, Table)| {
            write_selection(lua, refs_of(&list)?);
            Ok(())
        }),
        "Add" => func(lua, |lua, (_, list): (Value, Table)| {
            let mut refs = current_selection(lua);
            for r in refs_of(&list)? {
                if !refs.contains(&r) {
                    refs.push(r);
                }
            }
            write_selection(lua, refs);
            Ok(())
        }),
        "Remove" => func(lua, |lua, (_, list): (Value, Table)| {
            let gone = refs_of(&list)?;
            let mut refs = current_selection(lua);
            refs.retain(|r| !gone.contains(r));
            write_selection(lua, refs);
            Ok(())
        }),
        "SelectionChanged" => signal(lua, "Selection", "SelectionChanged"),
        "SelectionThickness" => Ok(Value::Number(1.0)),
        _ => Err(no_member("Selection", key)),
    }
}

fn push_request(lua: &Lua, request: Request) {
    env(lua).shared.borrow_mut().requests.push(request);
}

fn change_history(lua: &Lua, key: &str) -> Result<Value> {
    match key {
        "SetWaypoint" => func(lua, |lua, (_, name): (Value, String)| {
            push_request(lua, Request::Waypoint(name));
            Ok(())
        }),
        "TryBeginRecording" => func(lua, |lua, (_, name): (Value, String)| {
            let id = format!("{}:{name}", env(lua).shared.borrow_mut().next_id());
            state(lua).borrow_mut().recordings.insert(id.clone());
            Ok(id)
        }),
        "FinishRecording" => func(lua, |lua, (_, id, _op): (Value, String, MultiValueRest)| {
            state(lua).borrow_mut().recordings.remove(&id);
            let name = id
                .split_once(':')
                .map_or(id.as_str(), |(_, n)| n)
                .to_string();
            push_request(lua, Request::Waypoint(name));
            Ok(())
        }),
        "Undo" => func(lua, |lua, _: Value| {
            push_request(lua, Request::Undo);
            Ok(())
        }),
        "Redo" => func(lua, |lua, _: Value| {
            push_request(lua, Request::Redo);
            Ok(())
        }),
        "GetCanUndo" | "GetCanRedo" => func(lua, |_, _: Value| Ok(true)),
        "IsRecordingInProgress" => func(lua, |lua, _: MultiValueRest| {
            Ok(!state(lua).borrow().recordings.is_empty())
        }),
        "ResetWaypoints" | "SetEnabled" => func(lua, |_, _: MultiValueRest| Ok(())),
        "OnUndo" | "OnRedo" | "OnRecordingStarted" | "OnRecordingFinished" => {
            signal(lua, "ChangeHistoryService", key)
        }
        _ => Err(no_member("ChangeHistoryService", key)),
    }
}

type MultiValueRest = mlua::Variadic<Value>;

/// Fires `GetInstanceAddedSignal(tag)` / `GetInstanceRemovedSignal(tag)`
/// for a tag a plugin changed through `CollectionService`.
fn tag_signal(lua: &Lua, event: &str, tag: &str, r: Ref) -> Result<()> {
    let key = SignalKey::new(
        Owner::Service("CollectionService"),
        format!("{event}:{tag}"),
    );
    super::signal::fire(lua, &key, &[super::api::Arg::Instance(r)])
}

/// `BindToRenderStep(name, priority, fn)` / `UnbindFromRenderStep(name)`:
/// named `RenderStepped` connections (priority is not ordered).
fn render_step(lua: &Lua, unbind: bool) -> Result<Value> {
    const KEY: &str = "rbxn.render_step";
    let pair: Table = match lua.named_registry_value::<Option<Table>>(KEY)? {
        Some(pair) => pair,
        None => {
            let stepped = signal(lua, "RunService", "RenderStepped")?;
            let pair: Table = lua
                .load(
                    r#"
                    local stepped = ...
                    local bound = {}
                    local function bind(_, name, _, fn)
                        if bound[name] then bound[name]:Disconnect() end
                        bound[name] = stepped:Connect(fn)
                    end
                    local function unbind(_, name)
                        if bound[name] then bound[name]:Disconnect() end
                        bound[name] = nil
                    end
                    return { bind, unbind }
                    "#,
                )
                .set_name("=RunService")
                .call(stepped)?;
            lua.set_named_registry_value(KEY, pair.clone())?;
            pair
        }
    };
    pair.get(if unbind { 2 } else { 1 })
}

fn run_service(lua: &Lua, key: &str) -> Result<Value> {
    match key {
        "BindToRenderStep" => render_step(lua, false),
        "UnbindFromRenderStep" => render_step(lua, true),
        "IsStudio" | "IsEdit" => func(lua, |_, _: Value| Ok(true)),
        "IsRunning" | "IsRunMode" | "IsClient" | "IsServer" => func(lua, |_, _: Value| Ok(false)),
        "Heartbeat" | "RenderStepped" | "Stepped" => signal(lua, "RunService", key),
        _ => Err(no_member("RunService", key)),
    }
}

fn studio_service(lua: &Lua, key: &str) -> Result<Value> {
    match key {
        "ActiveScript" => Ok(Value::Nil),
        "GetUserId" => func(lua, |lua, _: Value| {
            Ok(env(lua).shared.borrow().context.user_id.unwrap_or(0))
        }),
        "StudioLocaleId" => Ok(Value::String(lua.create_string("en_us")?)),
        "GridSize" => Ok(Value::Number(1.0)),
        "RotateIncrement" => Ok(Value::Number(15.0)),
        "UseLocalSpace" | "DraggerSolveConstraints" | "ShowConstraintDetails" => {
            Ok(Value::Boolean(false))
        }
        "InstalledPluginData" => Ok(Value::String(lua.create_string("")?)),
        // Studio's class icons come from its own sheet, which rbx-native
        // does not ship; an empty image keeps an icon row laid out.
        "GetClassIcon" => func(lua, |lua, _: mlua::MultiValue| {
            let icon = lua.create_table()?;
            icon.set("Image", "")?;
            let vector2: Table = lua.globals().get("Vector2")?;
            let zero: Value = vector2.get("zero")?;
            icon.set("ImageRectOffset", zero.clone())?;
            icon.set("ImageRectSize", zero)?;
            Ok(icon)
        }),
        _ => Err(no_member("StudioService", key)),
    }
}

fn tags_of(lua: &Lua, r: Ref) -> Vec<String> {
    let env = env(lua);
    let dom = env.ctx.dom();
    dom.get(r)
        .map(|i| {
            rbx_dom::attributes::tags(i.properties().get("Tags"))
                .into_iter()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn write_tags(lua: &Lua, r: Ref, tags: &[String]) -> Result<()> {
    env(lua).ctx.check_write(r)?;
    let refs: Vec<&str> = tags.iter().map(String::as_str).collect();
    let text = rbx_dom::attributes::encode_tags(&refs);
    env(lua)
        .ctx
        .dom_mut()
        .set_property(r, "Tags", Variant::String(text))
        .map(|_| ())
        .map_err(mlua::Error::external)
}

fn collection_service(lua: &Lua, key: &str) -> Result<Value> {
    match key {
        "GetTagged" => func(lua, |lua, (_, tag): (Value, String)| {
            let env = env(lua);
            let parked = env.roots.borrow().clone();
            let found: Vec<Ref> = {
                let dom = env.ctx.dom();
                let mut stack: Vec<Ref> = dom
                    .root_refs()
                    .iter()
                    .filter(|r| !parked.contains(r))
                    .copied()
                    .collect();
                let mut found = Vec::new();
                while let Some(r) = stack.pop() {
                    let Some(instance) = dom.get(r) else { continue };
                    if rbx_dom::attributes::tags(instance.properties().get("Tags"))
                        .contains(&tag.as_str())
                    {
                        found.push(r);
                    }
                    stack.extend(instance.children().iter().rev());
                }
                found
            };
            instances_table(lua, &found)
        }),
        "HasTag" => func(lua, |lua, (_, inst, tag): (Value, Value, String)| {
            Ok(tags_of(lua, instance_ref(&inst)?).contains(&tag))
        }),
        "GetTags" => func(lua, |lua, (_, inst): (Value, Value)| {
            let tags = tags_of(lua, instance_ref(&inst)?);
            lua.create_sequence_from(tags)
        }),
        "AddTag" => func(lua, |lua, (_, inst, tag): (Value, Value, String)| {
            let r = instance_ref(&inst)?;
            let mut tags = tags_of(lua, r);
            if !tags.contains(&tag) {
                tags.push(tag.clone());
                write_tags(lua, r, &tags)?;
                tag_signal(lua, "InstanceAdded", &tag, r)?;
            }
            Ok(())
        }),
        "RemoveTag" => func(lua, |lua, (_, inst, tag): (Value, Value, String)| {
            let r = instance_ref(&inst)?;
            let mut tags = tags_of(lua, r);
            if tags.contains(&tag) {
                tags.retain(|t| *t != tag);
                write_tags(lua, r, &tags)?;
                tag_signal(lua, "InstanceRemoved", &tag, r)?;
            }
            Ok(())
        }),
        "GetInstanceAddedSignal" | "GetInstanceRemovedSignal" => {
            let event = &key[3..key.len() - 6];
            let event = event.to_string();
            func(lua, move |_, (_, tag): (Value, String)| {
                Ok(LuaSignal(SignalKey::new(
                    Owner::Service("CollectionService"),
                    format!("{event}:{tag}"),
                )))
            })
        }
        _ => Err(no_member("CollectionService", key)),
    }
}

/// Studio's Dark theme colour for an `Enum.StudioStyleGuideColor` (or its
/// name) under an optional `Enum.StudioStyleGuideModifier`; mid grey for a
/// name Studio does not know.
fn palette(color: &Value, modifier: Option<&Value>) -> Result<[f32; 3]> {
    let name = |value: &Value| -> Result<String> {
        let text = match value {
            Value::String(s) => s.to_string_lossy().to_string(),
            other => other.to_string()?,
        };
        Ok(text.rsplit('.').next().unwrap_or("").to_string())
    };
    let modifier = match modifier {
        Some(Value::Nil) | None => String::new(),
        Some(value) => name(value)?,
    };
    let [r, g, b] = palette::dark(&name(color)?, &modifier).unwrap_or([128, 128, 128]);
    Ok([r, g, b].map(|channel| f32::from(channel) / 255.0))
}

/// Installs `settings()` (and `UserSettings()`): `settings().Studio.Theme`
/// with `Name` "Dark", `GetColor(styleGuideColor, modifier?)` from Studio's
/// own Dark palette (see `palette`, falling back to mid grey) and
/// `ThemeChanged` (never fires).
pub(crate) fn install_settings(lua: &Lua) -> Result<()> {
    let theme = lua.create_table()?;
    theme.set("Name", "Dark")?;
    theme.set(
        "GetColor",
        lua.create_function(|lua, (_, color, modifier): (Value, Value, Option<Value>)| {
            let [r, g, b] = palette(&color, modifier.as_ref())?;
            lua.create_userdata(LuaColor3::new(r, g, b))
        })?,
    )?;
    theme.set(
        "ThemeChanged",
        LuaSignal(SignalKey::new(
            Owner::Service("StudioTheme"),
            "ThemeChanged",
        )),
    )?;
    let studio = lua.create_table()?;
    // `ThemeChanged` belongs to `Studio` (settings().Studio.ThemeChanged);
    // the theme keeps a copy for older plugins that read it there.
    studio.set("ThemeChanged", theme.get::<Value>("ThemeChanged")?)?;
    studio.set("Theme", theme)?;
    let root = lua.create_table()?;
    root.set("Studio", studio)?;
    // `settings():GetService("Studio")`, the long way to the same table.
    root.set(
        "GetService",
        lua.create_function(|_, (this, name): (Table, String)| {
            match this.get::<Value>(name.as_str())? {
                Value::Nil => Err(mlua::Error::runtime(format!(
                    "{name} is not a valid service of settings()"
                ))),
                found => Ok(found),
            }
        })?,
    )?;
    let getter = lua.create_function(move |_, ()| Ok(root.clone()))?;
    lua.globals().set("settings", getter.clone())?;
    lua.globals().set("UserSettings", getter)
}

/// Continues HTTP requests that were waiting on a permission prompt:
/// allowed → sent, refused → their threads resume with an error
/// ("Http requests to <host> are not allowed").
pub(crate) fn http_permission_answered(lua: &Lua, prompt: UiId, answer: &Answer) -> Result<()> {
    http_service::permission_answered(lua, prompt, answer)
}

/// Resumes threads whose HTTP request finished (`Http::poll`, filtered to
/// this VM's plugin by the host).
pub(crate) fn http_finished(lua: &Lua, finished: Vec<super::http::Finished>) -> Result<()> {
    http_service::finished(lua, finished)
}

#[cfg(test)]
mod tests {
    use mlua::MultiValue;
    use rbx_dom::WeakDom;

    use super::*;
    use crate::plugin::api::OutputKind;
    use crate::plugin::scheduler;
    use crate::plugin::testing::test_vm;

    fn vm(dom: WeakDom) -> (Lua, Rc<crate::plugin::env::Env>) {
        let (lua, env, _) = test_vm(dom, false);
        for name in SERVICES {
            let value = service(&lua, name).unwrap().unwrap();
            lua.globals().set(name, value).unwrap();
        }
        install_settings(&lua).unwrap();
        (lua, env)
    }

    fn run(lua: &Lua, src: &str) {
        scheduler::spawn(
            lua,
            lua.load(src).into_function().unwrap(),
            MultiValue::new(),
        )
        .unwrap();
    }

    fn no_errors(env: &crate::plugin::env::Env) {
        let shared = env.shared.borrow();
        let errors: Vec<_> = shared
            .output
            .iter()
            .filter(|l| l.kind == OutputKind::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    }

    fn two_parts() -> (WeakDom, Ref, Ref) {
        let mut dom = WeakDom::new();
        let a = dom.new_instance("Part", "A", None);
        let b = dom.new_instance("Part", "B", None);
        (dom, a, b)
    }

    #[test]
    fn selection_get_set_add_remove() {
        let (dom, a, b) = two_parts();
        let (lua, env) = vm(dom);
        env.shared.borrow_mut().context.selection = vec![a];
        run(&lua, "n = #Selection:Get() first = Selection:Get()[1].Name");
        assert_eq!(lua.globals().get::<f64>("n").unwrap(), 1.0);
        assert_eq!(lua.globals().get::<String>("first").unwrap(), "A");
        assert!(!env.shared.borrow().selection_written);

        lua.globals()
            .set("workspace_b", LuaInstance::new(b, env.ctx.clone()))
            .unwrap();
        run(&lua, "Selection:Add({workspace_b})");
        assert_eq!(env.shared.borrow().context.selection, vec![a, b]);
        assert!(env.shared.borrow().selection_written);

        run(&lua, "Selection:Remove({workspace_b})");
        assert_eq!(env.shared.borrow().context.selection, vec![a]);
        run(&lua, "Selection:Set({workspace_b})");
        assert_eq!(env.shared.borrow().context.selection, vec![b]);
        no_errors(&env);
    }

    #[test]
    fn selection_drops_removed_instances() {
        let (dom, a, _) = two_parts();
        let (lua, env) = vm(dom);
        env.shared.borrow_mut().context.selection = vec![a];
        env.ctx.dom_mut().remove(a);
        run(&lua, "n = #Selection:Get()");
        assert_eq!(lua.globals().get::<f64>("n").unwrap(), 0.0);
    }

    #[test]
    fn change_history_requests() {
        let (lua, env) = vm(WeakDom::new());
        run(
            &lua,
            "ChangeHistoryService:SetWaypoint('Move') ChangeHistoryService:Undo() ChangeHistoryService:Redo()
             local id = ChangeHistoryService:TryBeginRecording('Rec')
             recording = ChangeHistoryService:IsRecordingInProgress()
             ChangeHistoryService:FinishRecording(id, nil)
             after = ChangeHistoryService:IsRecordingInProgress()",
        );
        no_errors(&env);
        assert_eq!(
            env.shared.borrow().requests,
            vec![
                Request::Waypoint("Move".into()),
                Request::Undo,
                Request::Redo,
                Request::Waypoint("Rec".into())
            ]
        );
        assert!(lua.globals().get::<bool>("recording").unwrap());
        assert!(!lua.globals().get::<bool>("after").unwrap());
    }

    #[test]
    fn json_round_trip_nested() {
        let (lua, env) = vm(WeakDom::new());
        run(
            &lua,
            "local t = HttpService:JSONDecode('{\"a\":[1,2,[3,{\"b\":null}]],\"s\":\"x\",\"f\":1.5,\"ok\":true}')
             inner = t.a[3][1] second = t.a[2] f = t.f
             encoded = HttpService:JSONEncode({1, 2, {3, 4}})
             obj = HttpService:JSONDecode(HttpService:JSONEncode(t))
             again = obj.a[3][1] .. ''
             empty = HttpService:JSONEncode({})
             bad = select(2, pcall(HttpService.JSONEncode, HttpService, 0/0))",
        );
        no_errors(&env);
        let g = lua.globals();
        assert_eq!(g.get::<f64>("inner").unwrap(), 3.0);
        assert_eq!(g.get::<f64>("second").unwrap(), 2.0);
        assert_eq!(g.get::<f64>("f").unwrap(), 1.5);
        assert_eq!(g.get::<String>("encoded").unwrap(), "[1,2,[3,4]]");
        assert_eq!(g.get::<String>("again").unwrap(), "3");
        assert_eq!(g.get::<String>("empty").unwrap(), "[]");
        assert!(g
            .get::<Value>("bad")
            .unwrap()
            .to_string()
            .unwrap()
            .contains("non-finite"));
    }

    #[test]
    fn guid_format() {
        let (lua, env) = vm(WeakDom::new());
        run(
            &lua,
            "wrapped = HttpService:GenerateGUID() bare = HttpService:GenerateGUID(false)",
        );
        no_errors(&env);
        let wrapped = lua.globals().get::<String>("wrapped").unwrap();
        let bare = lua.globals().get::<String>("bare").unwrap();
        assert_eq!(wrapped.len(), 38);
        assert!(wrapped.starts_with('{') && wrapped.ends_with('}'));
        assert_eq!(bare.len(), 36);
        assert_eq!(bare.matches('-').count(), 4);
    }

    #[test]
    fn http_to_refused_host_errors() {
        let (lua, env) = vm(WeakDom::new());
        env.shared.borrow_mut().settings.insert(
            "Test".into(),
            serde_json::json!({ "__rbxnative_http": { "127.0.0.1": true } })
                .as_object()
                .cloned()
                .unwrap(),
        );
        run(
            &lua,
            "ok, err = pcall(HttpService.GetAsync, HttpService, 'http://127.0.0.1:1/')",
        );
        assert!(env.shared.borrow().http.busy());
        let mut finished = Vec::new();
        for _ in 0..250 {
            finished = env.shared.borrow_mut().http.poll();
            if !finished.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(finished.len(), 1);
        http_finished(&lua, finished).unwrap();
        assert!(!lua.globals().get::<bool>("ok").unwrap());
        assert!(lua
            .globals()
            .get::<String>("err")
            .unwrap()
            .contains("HttpService"));
    }

    #[test]
    fn http_prompt_then_denied() {
        let (lua, env) = vm(WeakDom::new());
        run(
            &lua,
            "ok, err = pcall(HttpService.GetAsync, HttpService, 'https://example.com/x')",
        );
        let prompt = match env.shared.borrow().requests.first() {
            Some(Request::Prompt { id, .. }) => *id,
            other => panic!("expected prompt, got {other:?}"),
        };
        http_permission_answered(&lua, prompt, &Answer::Allow(false)).unwrap();
        assert!(lua
            .globals()
            .get::<String>("err")
            .unwrap()
            .contains("example.com"));
        let shared = env.shared.borrow();
        assert_eq!(
            shared.settings["Test"]["__rbxnative_http"]["example.com"],
            false
        );
    }

    #[test]
    fn run_service_flags() {
        let (lua, env) = vm(WeakDom::new());
        run(
            &lua,
            "studio = RunService:IsStudio() edit = RunService:IsEdit() running = RunService:IsRunning()
             client = RunService:IsClient() server = RunService:IsServer() mode = RunService:IsRunMode()
             RunService.Heartbeat:Connect(function() end)",
        );
        no_errors(&env);
        let g = lua.globals();
        assert!(g.get::<bool>("studio").unwrap() && g.get::<bool>("edit").unwrap());
        for flag in ["running", "client", "server", "mode"] {
            assert!(!g.get::<bool>(flag).unwrap(), "{flag}");
        }
    }

    #[test]
    fn collection_tags_round_trip() {
        let (dom, a, b) = two_parts();
        let (lua, env) = vm(dom);
        lua.globals()
            .set("pa", LuaInstance::new(a, env.ctx.clone()))
            .unwrap();
        lua.globals()
            .set("pb", LuaInstance::new(b, env.ctx.clone()))
            .unwrap();
        run(
            &lua,
            "CollectionService:AddTag(pa, 'x') CollectionService:AddTag(pa, 'y') CollectionService:AddTag(pb, 'x')
             has = CollectionService:HasTag(pa, 'y') tagged = #CollectionService:GetTagged('x')
             CollectionService:RemoveTag(pa, 'x')
             left = #CollectionService:GetTagged('x') tags = table.concat(CollectionService:GetTags(pa), ',')",
        );
        no_errors(&env);
        let g = lua.globals();
        assert!(g.get::<bool>("has").unwrap());
        assert_eq!(g.get::<f64>("tagged").unwrap(), 2.0);
        assert_eq!(g.get::<f64>("left").unwrap(), 1.0);
        assert_eq!(g.get::<String>("tags").unwrap(), "y");
    }

    #[test]
    fn services_are_cached_and_theme_exists() {
        let (lua, env) = vm(WeakDom::new());
        run(
            &lua,
            "name =settings().Studio.Theme.Name
             c = settings().Studio.Theme:GetColor('MainBackground')",
        );
        no_errors(&env);
        assert_eq!(lua.globals().get::<String>("name").unwrap(), "Dark");
        let a = service(&lua, "Selection").unwrap().unwrap();
        let b = service(&lua, "Selection").unwrap().unwrap();
        assert!(a.equals(&b).unwrap());
        assert!(service(&lua, "Workspace").unwrap().is_none());
    }
}
