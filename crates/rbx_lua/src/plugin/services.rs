//! The services a place file has no instance for: `Selection`,
//! `ChangeHistoryService`, `HttpService`, `RunService`, `StudioService`,
//! `CollectionService`, `CoreGui` and `PluginGuiService` (the plugin's own
//! folders), plus the `settings()` global.

mod http_service;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use mlua::{FromLuaMulti, IntoLuaMulti, Lua, MetaMethod, Result, Table, UserData, UserDataMethods, Value};
use rbx_dom::{Ref, Variant};

use super::api::{Answer, Request, UiId};
use super::env::env;
use super::http::HttpRequest;
use super::scheduler::AwaitId;
use super::signal::{LuaSignal, Owner, SignalKey};
use crate::datatypes::color3::LuaColor3;
use crate::instance::LuaInstance;

const SERVICES: [&str; 8] = [
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
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| index(lua, this.0, &key));
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.0));
    }
}

/// `game:GetService(name)` for the services above (one userdata per VM per
/// service, cached in the Lua registry); `None` for anything else, which
/// then falls back to the place's own instances.
///
/// - `Selection`: `Get()`, `Set(list)`, `Add(list)`, `Remove(list)`,
///   `SelectionChanged`. Reads `Shared::context.selection` (dropping refs no
///   longer in the DOM); writes it back and sets `Shared::selection_written`.
///   Nothing fires from a Rust-side write: the host compares selections on
///   its next dispatch, so a `Set` inside a handler cannot re-enter.
/// - `ChangeHistoryService`: waypoints and undo/redo as [`Request`]s;
///   recordings are tracked per VM.
/// - `HttpService`: see `http_service`. A Roblox plugin's first request to a
///   host waits on a prompt; a native plugin needs `Permission::Http`.
/// - `RunService`: edit-mode flags and the frame signals the host fires.
/// - `StudioService`, `CollectionService` (over the `Tags` property) and the
///   plugin's own `CoreGui` / `PluginGuiService` folders.
pub(crate) fn service(lua: &Lua, name: &str) -> Result<Option<Value>> {
    let Some(&canon) = SERVICES.iter().find(|n| **n == name) else {
        return Ok(None);
    };
    let key = format!("rbxn.service.{canon}");
    if let Some(cached) = lua.named_registry_value::<Option<Value>>(&key)? {
        return Ok(Some(cached));
    }
    let env = env(lua);
    let value = match canon {
        "CoreGui" => Value::UserData(lua.create_userdata(LuaInstance::new(env.core_gui, env.ctx.clone()))?),
        "PluginGuiService" => {
            Value::UserData(lua.create_userdata(LuaInstance::new(env.plugin_gui_service, env.ctx.clone()))?)
        }
        _ => Value::UserData(lua.create_userdata(LuaService(canon))?),
    };
    lua.set_named_registry_value(&key, value.clone())?;
    Ok(Some(value))
}

fn func<A: FromLuaMulti, R: IntoLuaMulti>(lua: &Lua, f: impl Fn(&Lua, A) -> Result<R> + 'static) -> Result<Value> {
    Ok(Value::Function(lua.create_function(f)?))
}

fn signal(lua: &Lua, owner: &'static str, event: &str) -> Result<Value> {
    Ok(Value::UserData(
        lua.create_userdata(LuaSignal(SignalKey::new(Owner::Service(owner), event)))?,
    ))
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
        _ => collection_service(lua, key),
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
    selection.into_iter().filter(|r| dom.get(*r).is_some()).collect()
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
        "Get" => func(lua, |lua, _: Value| instances_table(lua, &current_selection(lua))),
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
            let name = id.split_once(':').map_or(id.as_str(), |(_, n)| n).to_string();
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

fn run_service(lua: &Lua, key: &str) -> Result<Value> {
    match key {
        "IsStudio" | "IsEdit" => func(lua, |_, _: Value| Ok(true)),
        "IsRunning" | "IsRunMode" | "IsClient" | "IsServer" => func(lua, |_, _: Value| Ok(false)),
        "Heartbeat" | "RenderStepped" | "Stepped" => signal(lua, "RunService", key),
        _ => Err(no_member("RunService", key)),
    }
}

fn studio_service(lua: &Lua, key: &str) -> Result<Value> {
    match key {
        "ActiveScript" => Ok(Value::Nil),
        "GetUserId" => func(lua, |_, _: Value| Ok(0)),
        "StudioLocaleId" => Ok(Value::String(lua.create_string("en_us")?)),
        "GridSize" => Ok(Value::Number(1.0)),
        "RotateIncrement" => Ok(Value::Number(15.0)),
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
                let mut stack: Vec<Ref> = dom.root_refs().iter().filter(|r| !parked.contains(r)).copied().collect();
                let mut found = Vec::new();
                while let Some(r) = stack.pop() {
                    let Some(instance) = dom.get(r) else { continue };
                    if rbx_dom::attributes::tags(instance.properties().get("Tags")).contains(&tag.as_str()) {
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
                tags.push(tag);
                write_tags(lua, r, &tags)?;
            }
            Ok(())
        }),
        "RemoveTag" => func(lua, |lua, (_, inst, tag): (Value, Value, String)| {
            let r = instance_ref(&inst)?;
            let mut tags = tags_of(lua, r);
            if tags.contains(&tag) {
                tags.retain(|t| *t != tag);
                write_tags(lua, r, &tags)?;
            }
            Ok(())
        }),
        _ => Err(no_member("CollectionService", key)),
    }
}

/// The dark theme's colours by `Enum.StudioStyleGuideColor` name.
fn palette(name: &str) -> [f32; 3] {
    let rgb = |r: u8, g: u8, b: u8| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
    match name {
        "MainBackground" | "Titlebar" | "ScrollBarBackground" => rgb(46, 46, 46),
        "MainText" | "ButtonText" | "BrightText" | "TitlebarText" => rgb(204, 204, 204),
        "SubText" | "DimmedText" => rgb(170, 170, 170),
        "Button" => rgb(60, 60, 60),
        "ButtonBorder" | "Border" | "CategoryItem" => rgb(34, 34, 34),
        "InputFieldBackground" => rgb(37, 37, 37),
        "InputFieldBorder" => rgb(58, 58, 58),
        "Item" | "ScriptBackground" => rgb(37, 37, 37),
        "MainButton" => rgb(0, 162, 255),
        "ErrorText" => rgb(255, 68, 68),
        "WarningText" => rgb(255, 150, 0),
        _ => rgb(128, 128, 128),
    }
}

/// Installs `settings()` (and `UserSettings()`): `settings().Studio.Theme`
/// with `Name` "Dark", `GetColor(styleGuideColor, modifier?)` from a fixed
/// dark palette keyed by `Enum.StudioStyleGuideColor` names (falling back to
/// mid grey) and `ThemeChanged` (never fires).
pub(crate) fn install_settings(lua: &Lua) -> Result<()> {
    let theme = lua.create_table()?;
    theme.set("Name", "Dark")?;
    theme.set(
        "GetColor",
        lua.create_function(|lua, (_, color, _modifier): (Value, Value, Option<Value>)| {
            let text = match &color {
                Value::String(s) => s.to_string_lossy().to_string(),
                other => other.to_string()?,
            };
            let name = text.rsplit('.').next().unwrap_or("");
            let [r, g, b] = palette(name);
            lua.create_userdata(LuaColor3::new(r, g, b))
        })?,
    )?;
    theme.set(
        "ThemeChanged",
        LuaSignal(SignalKey::new(Owner::Service("StudioTheme"), "ThemeChanged")),
    )?;
    let studio = lua.create_table()?;
    studio.set("Theme", theme)?;
    let root = lua.create_table()?;
    root.set("Studio", studio)?;
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
        scheduler::spawn(lua, lua.load(src).into_function().unwrap(), MultiValue::new()).unwrap();
    }

    fn no_errors(env: &crate::plugin::env::Env) {
        let shared = env.shared.borrow();
        let errors: Vec<_> = shared.output.iter().filter(|l| l.kind == OutputKind::Error).collect();
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
        assert!(g.get::<Value>("bad").unwrap().to_string().unwrap().contains("non-finite"));
    }

    #[test]
    fn guid_format() {
        let (lua, env) = vm(WeakDom::new());
        run(&lua, "wrapped = HttpService:GenerateGUID() bare = HttpService:GenerateGUID(false)");
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
        run(&lua, "ok, err = pcall(HttpService.GetAsync, HttpService, 'http://127.0.0.1:1/')");
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
        assert!(lua.globals().get::<String>("err").unwrap().contains("HttpService"));
    }

    #[test]
    fn http_prompt_then_denied() {
        let (lua, env) = vm(WeakDom::new());
        run(&lua, "ok, err = pcall(HttpService.GetAsync, HttpService, 'https://example.com/x')");
        let prompt = match env.shared.borrow().requests.first() {
            Some(Request::Prompt { id, .. }) => *id,
            other => panic!("expected prompt, got {other:?}"),
        };
        http_permission_answered(&lua, prompt, &Answer::Allow(false)).unwrap();
        assert!(lua.globals().get::<String>("err").unwrap().contains("example.com"));
        let shared = env.shared.borrow();
        assert_eq!(shared.settings["Test"]["__rbxnative_http"]["example.com"], false);
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
