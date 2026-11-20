//! The game services a Local Run server keeps beyond its scripts:
//! DataStoreService (a per-place store on disk), MemoryStoreService,
//! MessagingService (between every server of the run), HttpService's
//! requests and MarketplaceService's test purchases.
//!
//! Each service's rules live in its own Luau module (`services/*.luau`,
//! our own, from the creator docs and the goldens); this file is their
//! host: the store, the message bus shared by a universe's servers, the
//! requests made off the script thread, and the purchase prompts handed to
//! the editor.

mod store;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use mlua::{AnyUserData, Function, IntoLua, Lua, MultiValue, Result, Table, Value};
use rbx_dom::{Ref, Variant};

use crate::engine::events::instance_key;
use crate::engine::scheduler::{self, AwaitId};
use crate::engine::signal::{self, LuaSignal};
use crate::engine::{engine, json};
use crate::instance::LuaInstance;
use crate::plugin::http::{self, HttpRequest};

use super::pathfinding::PathRequest;

type PathArgs = (f64, f64, f64, f64, f64, f64, f64, bool, f64, Table);

/// How a run's services are set up.
#[derive(Clone, Debug)]
pub struct Services {
    /// Servers with the same universe key share DataStores, ownership and
    /// MessagingService; `None` keeps them to this VM.
    pub universe: Option<String>,
    /// The place's DataStores on disk (a JSON file); `None` for memory
    /// only, empty every run.
    pub data_file: Option<PathBuf>,
    /// Studio's "Enable Studio Access to API Services": DataStore calls
    /// fail as Studio's do when it's off.
    pub api_access: bool,
    /// Where `GetProductInfo` answers are kept for offline runs.
    pub cache_dir: Option<PathBuf>,
}

impl Default for Services {
    /// A live server's: API access on, memory only.
    fn default() -> Self {
        Services {
            universe: None,
            data_file: None,
            api_access: true,
            cache_dir: None,
        }
    }
}

/// A purchase prompt a script asked for, for the editor to show its test
/// purchase dialog: `kind` is `Asset`, `Product`, `GamePass`, `Bundle` or
/// `Subscription`.
#[derive(Clone, Debug, PartialEq)]
pub struct PurchasePrompt {
    pub kind: String,
    pub player: Ref,
    pub id: i64,
    /// A subscription's id (`EXP-...`), empty for the rest.
    pub subscription: String,
}

/// What every server of one universe shares.
struct Universe {
    data: Mutex<store::Data>,
    bus: Mutex<Bus>,
}

#[derive(Default)]
struct Bus {
    next: u64,
    messages: Vec<(u64, String, String, f64)>,
}

/// Messages kept for servers that haven't polled yet.
const BUS_KEEP: usize = 1024;

static UNIVERSES: LazyLock<Mutex<HashMap<String, Weak<Universe>>>> = LazyLock::new(Mutex::default);

fn universe(config: &Services) -> Arc<Universe> {
    let fresh = || {
        Arc::new(Universe {
            data: Mutex::new(store::Data::open(config.data_file.clone())),
            bus: Mutex::default(),
        })
    };
    let Some(key) = &config.universe else {
        return fresh();
    };
    let mut all = UNIVERSES.lock().unwrap_or_else(|e| e.into_inner());
    all.retain(|_, u| u.strong_count() > 0);
    if let Some(found) = all.get(key).and_then(Weak::upgrade) {
        let same_file = found
            .data
            .lock()
            .map(|d| d.file().map(PathBuf::from))
            .ok()
            .flatten()
            == config.data_file;
        if same_file {
            return found;
        }
    }
    let made = fresh();
    all.insert(key.clone(), Arc::downgrade(&made));
    made
}

type Finished = Arc<Mutex<Vec<(AwaitId, std::result::Result<serde_json::Value, String>)>>>;

/// One VM's side of the services.
pub(crate) struct Runtime {
    config: RefCell<Services>,
    universe: RefCell<Arc<Universe>>,
    /// The next message number this VM hasn't delivered.
    cursor: Cell<u64>,
    prompts: RefCell<Vec<PurchasePrompt>>,
    finished: Finished,
    client: Rc<Cell<bool>>,
    physics: Rc<dyn super::Physics>,
}

static PRODUCT_INFO: LazyLock<Mutex<HashMap<String, serde_json::Value>>> =
    LazyLock::new(Mutex::default);

impl Runtime {
    pub(crate) fn new(client: Rc<Cell<bool>>, physics: Rc<dyn super::Physics>) -> Rc<Runtime> {
        let config = Services::default();
        let universe = universe(&config);
        let cursor = universe.bus.lock().map(|b| b.next).unwrap_or(0);
        Rc::new(Runtime {
            config: RefCell::new(config),
            universe: RefCell::new(universe),
            cursor: Cell::new(cursor),
            prompts: RefCell::default(),
            finished: Arc::default(),
            client,
            physics,
        })
    }

    pub(crate) fn configure(&self, config: Services) {
        let universe = universe(&config);
        self.cursor
            .set(universe.bus.lock().map(|b| b.next).unwrap_or(0));
        *self.universe.borrow_mut() = universe;
        *self.config.borrow_mut() = config;
    }

    pub(crate) fn take_prompts(&self) -> Vec<PurchasePrompt> {
        std::mem::take(&mut self.prompts.borrow_mut())
    }

    fn data<R>(&self, f: impl FnOnce(&mut store::Data) -> R) -> R {
        let universe = self.universe.borrow().clone();
        let mut data = universe.data.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut data)
    }
}

const MODULES: &[(&str, &str)] = &[
    ("DataStoreService", include_str!("services/datastore.luau")),
    (
        "MemoryStoreService",
        include_str!("services/memorystore.luau"),
    ),
    ("MessagingService", include_str!("services/messaging.luau")),
    (
        "MarketplaceService",
        include_str!("services/marketplace.luau"),
    ),
    ("HttpService", include_str!("services/http.luau")),
    (
        "PathfindingService",
        include_str!("services/pathfinding.luau"),
    ),
    ("Helpers", include_str!("services/helpers.luau")),
];

/// The module that answers `class`.
fn module_of(class: &str) -> Option<&'static str> {
    Some(match class {
        "DataStoreService"
        | "GlobalDataStore"
        | "DataStore"
        | "OrderedDataStore"
        | "DataStoreKeyInfo"
        | "DataStorePages"
        | "DataStoreKeyPages"
        | "DataStoreVersionPages"
        | "DataStoreListingPages"
        | "DataStoreKey"
        | "DataStoreObjectVersionInfo"
        | "DataStoreInfo"
        | "DataStoreSetOptions"
        | "DataStoreIncrementOptions"
        | "DataStoreOptions" => "DataStoreService",
        "MemoryStoreService"
        | "MemoryStoreSortedMap"
        | "MemoryStoreHashMap"
        | "MemoryStoreQueue"
        | "MemoryStoreHashMapPages" => "MemoryStoreService",
        "MessagingService" => "MessagingService",
        "MarketplaceService" => "MarketplaceService",
        "HttpService" => "HttpService",
        "PathfindingService" | "Path" => "PathfindingService",
        "Debris" | "Players" | "Player" | "RunService" | "FriendPages" => "Helpers",
        _ => return None,
    })
}

/// Services only a server has.
const SERVER_ONLY: &[&str] = &["DataStoreService", "MemoryStoreService", "MessagingService"];

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn registry_key(name: &str) -> String {
    format!("rbxn.server.services.{name}")
}

fn loaded(lua: &Lua, name: &str) -> Result<Option<Table>> {
    lua.named_registry_value::<Option<Table>>(&registry_key(name))
}

fn module(lua: &Lua, runtime: &Rc<Runtime>, name: &str) -> Result<Table> {
    if let Some(module) = loaded(lua, name)? {
        return Ok(module);
    }
    let source = MODULES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| *s)
        .ok_or_else(|| mlua::Error::runtime(format!("no module {name}")))?;
    let module: Table = lua
        .load(source)
        .set_name(format!("={name}"))
        .call(host(lua, runtime)?)?;
    lua.set_named_registry_value(&registry_key(name), module.clone())?;
    Ok(module)
}

/// `class.key` for the services here, `None` for anything else.
pub(crate) fn member(
    lua: &Lua,
    runtime: &Rc<Runtime>,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    let Some(name) = module_of(class) else {
        return Ok(None);
    };
    if runtime.client.get() && SERVER_ONLY.contains(&name) && class == name {
        let message = format!("{name} can't be accessed from the client");
        let f = lua.create_function(move |_, _: MultiValue| -> Result<()> {
            Err(mlua::Error::runtime(message.clone()))
        })?;
        return Ok(Some(Value::Function(f)));
    }
    let (value, found, error): (Value, bool, Option<String>) = module(lua, runtime, name)?
        .get::<Function>("member")?
        .call((class, referent.value(), key))?;
    if let Some(message) = error {
        return Err(mlua::Error::runtime(message));
    }
    Ok(found.then_some(value))
}

/// A write the services take (`MarketplaceService.ProcessReceipt`).
pub(crate) fn set(
    lua: &Lua,
    runtime: &Rc<Runtime>,
    referent: Ref,
    class: &str,
    key: &str,
    value: &Value,
) -> Result<bool> {
    if class != "MarketplaceService" || key != "ProcessReceipt" {
        return Ok(false);
    }
    module(lua, runtime, class)?.get::<Function>("set")?.call((
        class,
        referent.value(),
        key,
        value.clone(),
    ))
}

/// Once a frame: requests that finished resume their scripts, messages
/// any server published reach this one's subscribers.
pub(crate) fn poll(lua: &Lua, runtime: &Rc<Runtime>) -> Result<()> {
    let done = std::mem::take(&mut *runtime.finished.lock().unwrap_or_else(|e| e.into_inner()));
    for (id, result) in done {
        let values = match result {
            Ok(json) => {
                MultiValue::from_vec(vec![Value::Boolean(true), json::from_json(lua, &json)?])
            }
            Err(message) => MultiValue::from_vec(vec![
                Value::Boolean(false),
                Value::String(lua.create_string(&message)?),
            ]),
        };
        scheduler::resolve(lua, id, Ok(values))?;
    }
    let fresh: Vec<(u64, String, String, f64)> = {
        let universe = runtime.universe.borrow().clone();
        let bus = universe.bus.lock().unwrap_or_else(|e| e.into_inner());
        bus.messages
            .iter()
            .filter(|m| m.0 >= runtime.cursor.get())
            .cloned()
            .collect()
    };
    if let Some(last) = fresh.last() {
        runtime.cursor.set(last.0 + 1);
    }
    if fresh.is_empty() || runtime.client.get() {
        return Ok(());
    }
    if let Some(messaging) = loaded(lua, "MessagingService")? {
        let deliver: Function = messaging.get("deliver")?;
        for (_, topic, text, sent) in fresh {
            deliver.call::<()>((topic, text, sent))?;
        }
    }
    Ok(())
}

/// The editor's purchase dialog closed on `prompt`.
pub(crate) fn finished(
    lua: &Lua,
    runtime: &Rc<Runtime>,
    prompt: &PurchasePrompt,
    bought: bool,
) -> Result<()> {
    if loaded(lua, "MarketplaceService")?.is_none() {
        return Ok(());
    }
    let ctx = engine(lua).ctx.clone();
    if ctx.dom().get(prompt.player).is_none() {
        return Ok(());
    }
    let player = LuaInstance::new(prompt.player, ctx).into_lua(lua)?;
    module(lua, runtime, "MarketplaceService")?
        .get::<Function>("finished")?
        .call::<()>((
            prompt.kind.as_str(),
            player,
            prompt.id,
            bought,
            prompt.subscription.as_str(),
        ))
}

fn entry_table(lua: &Lua, entry: Option<store::Entry>) -> Result<Value> {
    let Some(entry) = entry else {
        return Ok(Value::Nil);
    };
    let t = lua.create_table()?;
    t.set("value", entry.version.value)?;
    t.set("version", entry.version.id)?;
    t.set("created", entry.created)?;
    t.set("updated", entry.version.created)?;
    t.set("users", entry.version.users)?;
    t.set("meta", entry.version.meta)?;
    t.set("deleted", entry.version.deleted)?;
    Ok(Value::Table(t))
}

/// A request run off the script thread; its answer resumes the script at
/// the next [`poll`] as `(true, value)` or `(false, message)`.
fn in_background(
    lua: &Lua,
    runtime: &Rc<Runtime>,
    work: impl FnOnce() -> std::result::Result<serde_json::Value, String> + Send + 'static,
) -> AwaitId {
    let id = scheduler::new_await(lua);
    let finished = runtime.finished.clone();
    std::thread::spawn(move || {
        let result = work();
        finished
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((id, result));
    });
    id
}

fn instance_id(value: &Value) -> Result<u32> {
    value
        .as_userdata()
        .and_then(|u| u.borrow::<LuaInstance>().ok())
        .map(|i| i.referent().value())
        .ok_or_else(|| mlua::Error::runtime("Instance expected"))
}

/// The table every module gets: what the Luau side can't do itself.
fn host(lua: &Lua, runtime: &Rc<Runtime>) -> Result<Table> {
    let host = lua.create_table()?;
    host.set(
        "clock",
        lua.create_function(|lua, ()| Ok(engine(lua).host.now()))?,
    )?;
    host.set("now_ms", lua.create_function(|_, ()| Ok(now_ms()))?)?;
    let r = runtime.clone();
    host.set(
        "api_access",
        lua.create_function(move |_, ()| Ok(r.config.borrow().api_access))?,
    )?;
    let r = runtime.clone();
    host.set(
        "server",
        lua.create_function(move |_, ()| Ok(!r.client.get()))?,
    )?;
    host.set(
        "http_enabled",
        lua.create_function(|lua, ()| {
            let ctx = engine(lua).ctx.clone();
            let dom = ctx.dom();
            let enabled = dom
                .root_refs()
                .iter()
                .filter_map(|r| dom.get(*r))
                .find(|i| i.class() == "HttpService")
                .and_then(|i| i.properties().get("HttpEnabled").cloned());
            Ok(matches!(enabled, Some(Variant::Bool(true))))
        })?,
    )?;
    host.set(
        "new",
        lua.create_function(|lua, (class, name): (String, String)| {
            let ctx = engine(lua).ctx.clone();
            let referent = ctx.dom_mut().new_instance(&class, &name, None);
            // Parented to nothing, as Instance.new's: not a root of the place.
            ctx.dom_mut().unparent(referent);
            let object = LuaInstance::new(referent, ctx).into_lua(lua)?;
            Ok((object, referent.value()))
        })?,
    )?;
    host.set(
        "id",
        lua.create_function(|_, value: Value| instance_id(&value))?,
    )?;
    host.set(
        "encode",
        lua.create_function(|_, value: Value| json::encode(&value))?,
    )?;
    host.set(
        "decode",
        lua.create_function(|lua, text: String| json::decode(lua, &text))?,
    )?;
    host.set("guid", lua.create_function(|_, ()| Ok(json::guid(false)))?)?;
    host.set(
        "bucket",
        lua.create_function(|_, (ordered, name, scope): (bool, String, String)| {
            Ok(store::bucket(ordered, &name, &scope))
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "ds_get",
        lua.create_function(move |lua, (bucket, key): (String, String)| {
            entry_table(lua, r.data(|d| d.get(&bucket, &key)))
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "ds_version",
        lua.create_function(move |lua, (bucket, key, id): (String, String, String)| {
            entry_table(lua, r.data(|d| d.version(&bucket, &key, &id)))
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "ds_version_at",
        lua.create_function(move |lua, (bucket, key, time): (String, String, i64)| {
            entry_table(lua, r.data(|d| d.version_at(&bucket, &key, time)))
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "ds_put",
        lua.create_function(
            move |lua, (bucket, key, value, users, meta): (String, String, String, String, String)| {
                let entry = r.data(|d| d.put(&bucket, &key, Some(value), users, meta, now_ms()));
                entry_table(lua, Some(entry))
            },
        )?,
    )?;
    let r = runtime.clone();
    host.set(
        "ds_remove",
        lua.create_function(move |lua, (bucket, key): (String, String)| {
            let old = r.data(|d| {
                let old = d.get(&bucket, &key)?;
                d.put(&bucket, &key, None, "[]".into(), "{}".into(), now_ms());
                Some(old)
            });
            entry_table(lua, old)
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "ds_versions",
        lua.create_function(move |lua, (bucket, key): (String, String)| {
            let list = lua.create_table()?;
            for (i, v) in r
                .data(|d| d.versions(&bucket, &key))
                .into_iter()
                .enumerate()
            {
                let t = lua.create_table()?;
                t.set("version", v.id)?;
                t.set("created", v.created)?;
                t.set("deleted", v.deleted)?;
                list.raw_set(i + 1, t)?;
            }
            Ok(list)
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "ds_remove_version",
        lua.create_function(move |_, (bucket, key, id): (String, String, String)| {
            Ok(r.data(|d| d.remove_version(&bucket, &key, &id)))
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "ds_keys",
        lua.create_function(
            move |lua, (name, scope, prefix, deleted): (String, Option<String>, String, bool)| {
                lua.create_sequence_from(
                    r.data(|d| d.keys(&name, scope.as_deref(), &prefix, deleted)),
                )
            },
        )?,
    )?;
    let r = runtime.clone();
    host.set(
        "ds_stores",
        lua.create_function(move |lua, prefix: String| {
            let list = lua.create_table()?;
            for (i, (name, created, updated)) in
                r.data(|d| d.stores(&prefix)).into_iter().enumerate()
            {
                let t = lua.create_table()?;
                t.set("name", name)?;
                t.set("created", created)?;
                t.set("updated", updated)?;
                list.raw_set(i + 1, t)?;
            }
            Ok(list)
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "ds_entries",
        lua.create_function(move |lua, bucket: String| {
            let list = lua.create_table()?;
            for (i, (key, value)) in r.data(|d| d.entries(&bucket)).into_iter().enumerate() {
                let t = lua.create_table()?;
                t.set("key", key)?;
                t.set("value", value)?;
                list.raw_set(i + 1, t)?;
            }
            Ok(list)
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "owns",
        lua.create_function(move |_, (user, item): (i64, String)| {
            Ok(r.data(|d| d.owns(user, &item)))
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "grant",
        lua.create_function(move |_, (user, item): (i64, String)| {
            r.data(|d| d.grant(user, &item));
            Ok(())
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "publish",
        lua.create_function(move |_, (topic, text): (String, String)| {
            let universe = r.universe.borrow().clone();
            let mut bus = universe.bus.lock().unwrap_or_else(|e| e.into_inner());
            let seq = bus.next;
            bus.next += 1;
            bus.messages
                .push((seq, topic, text, now_ms() as f64 / 1000.0));
            let excess = bus.messages.len().saturating_sub(BUS_KEEP);
            bus.messages.drain(..excess);
            Ok(())
        })?,
    )?;
    host.set(
        "signal",
        lua.create_function(|_, (id, name): (u32, String)| {
            Ok(LuaSignal(instance_key(Ref::new(id), name)))
        })?,
    )?;
    host.set(
        "fire",
        lua.create_function(|lua, (id, name, args): (u32, String, MultiValue)| {
            signal::fire_values(lua, &instance_key(Ref::new(id), name), args)
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "prompt",
        lua.create_function(
            move |_, (kind, player, id, subscription): (String, AnyUserData, i64, Option<String>)| {
                let player = player.borrow::<LuaInstance>()?.referent();
                r.prompts.borrow_mut().push(PurchasePrompt {
                    kind,
                    player,
                    id,
                    subscription: subscription.unwrap_or_default(),
                });
                Ok(())
            },
        )?,
    )?;
    let r = runtime.clone();
    let request = lua.create_function(move |lua, options: Table| {
        let request = HttpRequest {
            url: options.get("Url")?,
            method: options.get("Method")?,
            headers: options
                .get::<Option<Table>>("Headers")?
                .map(|h| h.pairs::<String, String>().collect::<Result<Vec<_>>>())
                .transpose()?
                .unwrap_or_default(),
            body: options.get("Body")?,
        };
        Ok(in_background(lua, &r, move || {
            let response = http::send(&request).map_err(|e| failure(&e))?;
            let headers: serde_json::Map<String, serde_json::Value> = response
                .headers
                .into_iter()
                .map(|(k, v)| (k, serde_json::Value::String(v)))
                .collect();
            Ok(serde_json::json!({
                "Success": response.success,
                "StatusCode": response.status_code,
                "StatusMessage": response.status_message,
                "Headers": headers,
                "Body": response.body,
            }))
        }))
    })?;
    let r = runtime.clone();
    host.set(
        "find_path",
        lua.create_function(move |lua, args: PathArgs| {
            let (sx, sy, sz, gx, gy, gz, height, can_jump, spacing, costs) = args;
            let request = PathRequest {
                start: [sx, sy, sz],
                goal: [gx, gy, gz],
                height,
                can_jump,
                spacing,
                costs: costs.pairs::<String, f64>().collect::<Result<_>>()?,
            };
            let ctx = engine(lua).ctx.clone();
            let Some(found) = super::pathfinding::compute(r.physics.as_ref(), &ctx, &request)
            else {
                return Ok(Value::Nil);
            };
            let list = lua.create_table()?;
            for (i, w) in found.into_iter().enumerate() {
                let t = lua.create_table()?;
                t.set("x", w.position[0])?;
                t.set("y", w.position[1])?;
                t.set("z", w.position[2])?;
                t.set("action", w.action)?;
                t.set("label", w.label)?;
                list.raw_set(i + 1, t)?;
            }
            Ok(Value::Table(list))
        })?,
    )?;
    let r = runtime.clone();
    host.set(
        "occlusion",
        lua.create_function(move |lua, (flat, from, height): (Vec<f64>, f64, f64)| {
            let points: Vec<[f64; 3]> = flat.as_chunks::<3>().0.to_vec();
            let ctx = engine(lua).ctx.clone();
            Ok(super::pathfinding::occlusion(
                r.physics.as_ref(),
                &ctx,
                &points,
                from.max(1.0) as usize,
                height,
            ))
        })?,
    )?;
    let request = scheduler::yielding(lua, request)?;
    host.set("request", checked(lua, request)?)?;
    let r = runtime.clone();
    let info = lua.create_function(move |lua, (id, kind): (i64, String)| {
        let cache = r.config.borrow().cache_dir.clone();
        Ok(in_background(lua, &r, move || {
            product_info(id, &kind, cache)
        }))
    })?;
    let info = scheduler::yielding(lua, info)?;
    for (name, by_name) in [("user_name", false), ("user_id", true)] {
        let r = runtime.clone();
        let call = lua.create_function(move |lua, who: Value| {
            let cache = r.config.borrow().cache_dir.clone();
            let who = match who {
                Value::String(s) => s.to_str()?.to_owned(),
                other => format!("{}", other.as_i64().unwrap_or_default()),
            };
            Ok(in_background(lua, &r, move || {
                user_lookup(&who, by_name, cache)
            }))
        })?;
        host.set(name, checked(lua, scheduler::yielding(lua, call)?)?)?;
    }
    host.set("product_info", checked(lua, info)?)?;
    Ok(host)
}

/// A yielding host call that answers `(ok, value)` as its value, or an
/// error without a position (as an engine call's has none).
fn checked(lua: &Lua, call: Function) -> Result<Function> {
    lua.load(
        r#"local call = ...
        return function(...)
            local ok, value = call(...)
            if not ok then
                error(value, 0)
            end
            return value
        end"#,
    )
    .set_name("=services")
    .call(call)
}

/// A failed request as Roblox words it: offline and unreachable alike are
/// a connection failure.
fn failure(error: &str) -> String {
    if rbx_net::is_offline_message(error) || error.contains("onnect") || error.contains("resolve") {
        "HttpError: ConnectFail".to_owned()
    } else if error.to_ascii_lowercase().contains("timeout") || error.contains("timed out") {
        "HttpError: Timedout".to_owned()
    } else {
        format!("HttpError: {error}")
    }
}

/// `GetProductInfo` from Roblox's public catalog APIs, kept (in memory and
/// under `cache`) so a later offline run still has it.
fn product_info(
    id: i64,
    kind: &str,
    cache: Option<PathBuf>,
) -> std::result::Result<serde_json::Value, String> {
    let key = format!("{kind}-{id}");
    let file = cache.map(|dir| dir.join("product-info").join(format!("{key}.json")));
    let url = match kind {
        "GamePass" => {
            format!("https://apis.roblox.com/game-passes/v1/game-passes/{id}/product-info")
        }
        "Product" => format!(
            "https://apis.roblox.com/developer-products/v1/developer-products/{id}/product-info"
        ),
        "Bundle" => format!("https://catalog.roblox.com/v1/bundles/{id}/details"),
        _ => format!("https://economy.roblox.com/v2/assets/{id}/details"),
    };
    fetch_cached(
        key,
        HttpRequest {
            url,
            method: "GET".into(),
            headers: Vec::new(),
            body: None,
        },
        "MarketplaceService:GetProductInfo()",
        file,
    )
}

/// `Players:GetNameFromUserIdAsync` (`by_name: false`, `who` is the id) or
/// `GetUserIdFromNameAsync`, from Roblox's users API, kept like product
/// info.
fn user_lookup(
    who: &str,
    by_name: bool,
    cache: Option<PathBuf>,
) -> std::result::Result<serde_json::Value, String> {
    let kind = if by_name { "name" } else { "id" };
    let key = format!("user-{kind}-{}", who.to_lowercase());
    let file = cache.map(|dir| dir.join("users").join(format!("{key}.json")));
    let (request, what) = if by_name {
        let body = serde_json::json!({ "usernames": [who], "excludeBannedUsers": false });
        (
            HttpRequest {
                url: "https://users.roblox.com/v1/usernames/users".into(),
                method: "POST".into(),
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: Some(body.to_string()),
            },
            "Players:GetUserIdFromNameAsync()",
        )
    } else {
        (
            HttpRequest {
                url: format!("https://users.roblox.com/v1/users/{who}"),
                method: "GET".into(),
                headers: Vec::new(),
                body: None,
            },
            "Players:GetNameFromUserIdAsync()",
        )
    };
    let found = fetch_cached(key, request, what, file)?;
    let answer = if by_name {
        found["data"][0]["id"].clone()
    } else {
        found["name"].clone()
    };
    if answer.is_null() {
        return Err(format!("{what} failed: Unknown user"));
    }
    Ok(answer)
}

/// A JSON answer from Roblox's web APIs, kept in memory and in `file`; the
/// kept copy answers when the request fails. `what` names the call in the
/// errors.
fn fetch_cached(
    key: String,
    request: HttpRequest,
    what: &str,
    file: Option<PathBuf>,
) -> std::result::Result<serde_json::Value, String> {
    if let Some(found) = PRODUCT_INFO.lock().ok().and_then(|m| m.get(&key).cloned()) {
        return Ok(found);
    }
    let fetched = http::send(&request);

    let from_disk = || {
        file.as_ref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    };
    let value = match fetched {
        Ok(response) if response.success => {
            serde_json::from_str::<serde_json::Value>(&response.body)
                .map_err(|_| format!("{what} failed: the answer wasn't JSON"))?
        }
        Ok(response) => {
            return from_disk().ok_or_else(|| {
                format!(
                    "{what} failed because HTTP {} ({})",
                    response.status_code, response.status_message
                )
            })
        }
        Err(error) => {
            return from_disk().ok_or_else(|| {
                if rbx_net::is_offline_message(&error) {
                    format!("{what} failed because the app is offline and it isn't cached")
                } else {
                    format!("{what} failed because {}", failure(&error))
                }
            })
        }
    };
    if let Some(file) = &file {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, value.to_string());
    }
    if let Ok(mut map) = PRODUCT_INFO.lock() {
        map.insert(key, value.clone());
    }
    Ok(value)
}
