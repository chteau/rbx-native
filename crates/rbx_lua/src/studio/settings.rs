//! `settings()` and `UserSettings()`: Studio's settings objects (`Studio`,
//! `Diagnostics`, `Physics`, `Rendering`, `Network`, …) for plugins and the
//! Command Bar.
//!
//! They aren't part of any place, so they live outside the DOM as
//! [`SettingsObject`]s: `typeof` gives "Instance", `ClassName`, `Name`,
//! `Parent`, `IsA`, `GetFullName`, `Changed` and `GetPropertyChangedSignal`
//! work, properties read and write with Roblox's type checks and errors.
//! The values a fresh Studio 0.748 gives (read from its Command Bar,
//! 2026-11-21) are in `settings.tsv`; the read-only ones that track the
//! session (`DebugSettings.InstanceCount`, memory) are worked out live.

use std::cell::RefCell;
use std::collections::BTreeMap;

use mlua::{
    AnyUserData, Lua, MetaMethod, Result, UserData, UserDataFields, UserDataMethods, Value,
    Variadic,
};

use crate::ctx::Ctx;
use crate::engine::signal::{self, LuaSignal, Owner, SignalKey};
use crate::lua_enum::LuaEnumItem;
use crate::plugin::api::Arg;
use crate::property::to_lua::variant_to_lua;

const DEFAULTS: &str = include_str!("settings.tsv");

/// `settings()`'s children in Studio's order: (class, name).
const GLOBAL: &[(&str, &str)] = &[
    ("GameSettings", "Game Options"),
    ("LuaSettings", "Lua"),
    ("DebugSettings", "Diagnostics"),
    ("PhysicsSettings", "Physics"),
    ("Studio", "Studio"),
    ("NetworkSettings", "Network"),
    ("RenderSettings", "Rendering"),
    ("TaskScheduler", "Task Scheduler"),
];

/// One settings object.
pub(crate) struct SettingsObject {
    pub(crate) class: &'static str,
    name: String,
    ctx: Ctx,
    values: RefCell<BTreeMap<String, Value>>,
    children: RefCell<Vec<AnyUserData>>,
    parent: Option<AnyUserData>,
}

/// The class names settings objects take, as `&'static str` for signal
/// owners.
fn static_class(class: &str) -> &'static str {
    const CLASSES: &[&str] = &[
        "GlobalSettings",
        "UserSettings",
        "UserGameSettings",
        "Studio",
        "StudioTheme",
        "DebugSettings",
        "PhysicsSettings",
        "RenderSettings",
        "NetworkSettings",
        "GameSettings",
        "LuaSettings",
        "TaskScheduler",
    ];
    CLASSES
        .iter()
        .find(|c| **c == class)
        .copied()
        .unwrap_or("Instance")
}

impl SettingsObject {
    fn make(
        lua: &Lua,
        ctx: &Ctx,
        class: &str,
        name: &str,
        parent: Option<AnyUserData>,
    ) -> Result<AnyUserData> {
        let class = static_class(class);
        let mut values = BTreeMap::new();
        let db = ctx.database();
        for (property, value) in db.default_values(class) {
            let Some(descriptor) = db.resolve_property(class, property) else {
                continue;
            };
            let value = variant_to_lua(lua, ctx, property, &descriptor.value_type, value)?;
            values.insert(property.to_owned(), value);
        }
        for line in DEFAULTS.lines() {
            let mut cells = line.splitn(4, '\t');
            let (Some(owner), Some(property), Some(kind), value) = (
                cells.next(),
                cells.next(),
                cells.next(),
                cells.next().unwrap_or(""),
            ) else {
                continue;
            };
            if owner != class {
                continue;
            }
            let source = match kind {
                "boolean" | "number" => value.to_owned(),
                "Color3" => format!("Color3.new({value})"),
                "EnumItem" => value.to_owned(),
                _ => format!("{value:?}"),
            };
            let value: Value = lua.load(format!("return {source}")).eval()?;
            values.insert(property.to_owned(), value);
        }
        lua.create_userdata(SettingsObject {
            class,
            name: name.to_owned(),
            ctx: ctx.clone(),
            values: RefCell::new(values),
            children: RefCell::default(),
            parent,
        })
    }

    fn full_name(&self) -> String {
        match &self.parent {
            Some(parent) => match parent.borrow::<SettingsObject>() {
                Ok(parent) => format!("{}.{}", parent.full_name(), self.name),
                Err(_) => self.name.clone(),
            },
            None => self.name.clone(),
        }
    }

    fn child(&self, name: &str) -> Option<AnyUserData> {
        self.children
            .borrow()
            .iter()
            .find(|c| {
                c.borrow::<SettingsObject>()
                    .is_ok_and(|c| c.name == name || c.class == name)
            })
            .cloned()
    }

    pub(crate) fn signal(&self, event: impl Into<String>) -> LuaSignal {
        LuaSignal(SignalKey::new(Owner::Service(self.class), event))
    }

    /// A value set without firing anything, while the tree is made.
    pub(crate) fn set_initial(&self, key: &str, value: Value) {
        self.values.borrow_mut().insert(key.to_owned(), value);
    }

    /// Live values, read on every access.
    fn live(&self, key: &str) -> Option<f64> {
        match (self.class, key) {
            ("DebugSettings", "InstanceCount") => Some(count_instances(&self.ctx) as f64),
            ("DebugSettings", "JobCount" | "PlayerCount") => Some(0.0),
            ("DebugSettings", "DataModel") => Some(1.0),
            ("NetworkSettings", "EmulatedTotalMemoryInMB") => meminfo("MemTotal:"),
            ("NetworkSettings", "FreeMemoryMBytes") => meminfo("MemAvailable:"),
            _ => None,
        }
    }
}

pub(crate) fn count_instances(ctx: &Ctx) -> usize {
    let dom = ctx.dom();
    let mut stack: Vec<_> = dom.root_refs().to_vec();
    let mut count = 0;
    while let Some(referent) = stack.pop() {
        count += 1;
        if let Some(instance) = dom.get(referent) {
            stack.extend_from_slice(instance.children());
        }
    }
    count
}

/// A `/proc/meminfo` line in megabytes (0 where there is none).
fn meminfo(field: &str) -> Option<f64> {
    let text = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let kb = text
        .lines()
        .find_map(|l| l.strip_prefix(field))
        .and_then(|rest| rest.split_whitespace().next()?.parse::<f64>().ok())
        .unwrap_or(0.0);
    Some(kb / 1024.0)
}

fn no_member(key: &str, this: &SettingsObject) -> mlua::Error {
    mlua::Error::runtime(format!(
        "{key} is not a valid member of {} \"{}\"",
        this.class, this.name
    ))
}

/// The type name Roblox's assignment errors use for a value.
fn type_name(lua: &Lua, value: &Value) -> Result<String> {
    let typeof_fn: mlua::Function = lua.globals().get("typeof")?;
    typeof_fn.call(value.clone())
}

impl UserData for SettingsObject {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Instance");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.name.clone()));
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: AnyUserData| {
            Ok(other
                .borrow::<SettingsObject>()
                .is_ok_and(|o| std::ptr::eq(&*o, this)))
        });
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            index(lua, this, &key)
        });
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |lua, this, (key, value): (String, Value)| assign(lua, this, &key, value),
        );
    }
}

fn function(
    lua: &Lua,
    f: impl Fn(&Lua, Variadic<Value>) -> Result<Value> + 'static,
) -> Result<Value> {
    Ok(Value::Function(lua.create_function(f)?))
}

fn this_object(args: &Variadic<Value>) -> Result<mlua::UserDataRef<SettingsObject>> {
    args.first()
        .and_then(|v| v.as_userdata())
        .and_then(|u| u.borrow::<SettingsObject>().ok())
        .ok_or_else(|| mlua::Error::runtime("Expected ':' not '.' calling member function"))
}

fn index(lua: &Lua, this: &SettingsObject, key: &str) -> Result<Value> {
    match key {
        "ClassName" => return lua.create_string(this.class).map(Value::String),
        "Name" => return lua.create_string(&this.name).map(Value::String),
        "Parent" => return Ok(this.parent.clone().map_or(Value::Nil, Value::UserData)),
        "Archivable" => return Ok(Value::Boolean(true)),
        "Changed" => {
            return lua
                .create_userdata(this.signal("Changed"))
                .map(Value::UserData)
        }
        "IsA" => {
            return function(lua, |_, args| {
                let this = this_object(&args)?;
                let name = args
                    .get(1)
                    .and_then(|v| v.as_string())
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default();
                Ok(Value::Boolean(
                    name == "Instance" || this.ctx.database().is_subclass_of(this.class, &name),
                ))
            })
        }
        "GetFullName" => {
            return function(lua, |lua, args| {
                lua.create_string(this_object(&args)?.full_name())
                    .map(Value::String)
            })
        }
        "GetPropertyChangedSignal" => {
            return function(lua, |lua, args| {
                let this = this_object(&args)?;
                let name = args
                    .get(1)
                    .and_then(|v| v.as_string())
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default();
                if !this.values.borrow().contains_key(&name) && this.live(&name).is_none() {
                    return Err(mlua::Error::runtime(format!(
                        "{name} is not a valid property name."
                    )));
                }
                lua.create_userdata(this.signal(format!("Changed:{name}")))
                    .map(Value::UserData)
            })
        }
        "GetChildren" => {
            return function(lua, |lua, args| {
                let this = this_object(&args)?;
                let list = this.children.borrow().clone();
                lua.create_sequence_from(list).map(Value::Table)
            })
        }
        "FindFirstChild" | "FindService" => {
            return function(lua, |_, args| {
                let this = this_object(&args)?;
                let name = args
                    .get(1)
                    .and_then(|v| v.as_string())
                    .map(|s| s.to_string_lossy());
                Ok(name
                    .and_then(|n| this.child(&n))
                    .map_or(Value::Nil, Value::UserData))
            })
        }
        "GetService" if matches!(this.class, "GlobalSettings" | "UserSettings") => {
            return function(lua, |_, args| {
                let this = this_object(&args)?;
                let name = args
                    .get(1)
                    .and_then(|v| v.as_string())
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default();
                this.child(&name).map(Value::UserData).ok_or_else(|| {
                    mlua::Error::runtime(format!("'{name}' is not a valid Service name"))
                })
            })
        }
        _ => {}
    }
    if let Some(value) = this.live(key) {
        return Ok(Value::Number(value));
    }
    if let Some(value) = this.values.borrow().get(key) {
        return Ok(value.clone());
    }
    if let Some(child) = this.child(key) {
        return Ok(Value::UserData(child));
    }
    if let Some(value) = super::theme::member(lua, this, key)? {
        return Ok(value);
    }
    if let Some(error) = UNREADABLE
        .iter()
        .find(|(c, k, _)| *c == this.class && *k == key)
    {
        return Err(mlua::Error::runtime(format!(
            "Unable to get property {key}, type {}",
            error.2
        )));
    }
    match (this.class, key) {
        ("RenderSettings", "GetMaxQualityLevel") => function(lua, |_, _| Ok(Value::Integer(22))),
        _ => Err(no_member(key, this)),
    }
}

/// Properties real Studio's Command Bar can't read either, and the Qt type
/// its error names.
const UNREADABLE: &[(&str, &str, &str)] = &[
    ("Studio", "Font", "QFont"),
    ("Studio", "Output Font", "QFont"),
    ("Studio", "PluginsDir", "QDir"),
];

fn assign(lua: &Lua, this: &SettingsObject, key: &str, value: Value) -> Result<()> {
    let old = match this.values.borrow().get(key) {
        Some(old) => old.clone(),
        None if this.live(key).is_some() => {
            return Err(mlua::Error::runtime(format!(
                "Unable to assign property {key}. Property is read only"
            )))
        }
        None => return Err(no_member(key, this)),
    };
    let read_only = this
        .ctx
        .database()
        .resolve_property(this.class, key)
        .is_some_and(|p| p.tags.iter().any(|t| t == "ReadOnly") || !p.script_writable);
    if read_only {
        return Err(mlua::Error::runtime(format!(
            "Unable to assign property {key}. Property is read only"
        )));
    }
    // Studio's coercions, as its Command Bar shows them: a boolean takes
    // any value's truth, a number reads a string (0 when it isn't one) and
    // turns anything else into 0, a string takes a number.
    let (want, got) = (type_name(lua, &old)?, type_name(lua, &value)?);
    let value = match (want.as_str(), &value) {
        ("boolean", v) => Value::Boolean(!matches!(v, Value::Nil | Value::Boolean(false))),
        ("number", Value::Number(_) | Value::Integer(_)) => value,
        ("number", Value::String(s)) => {
            Value::Number(s.to_str()?.trim().parse::<f64>().unwrap_or(0.0))
        }
        ("number", _) => Value::Number(0.0),
        ("string", Value::Number(_) | Value::Integer(_)) => {
            Value::String(lua.create_string(value.to_string()?)?)
        }
        ("EnumItem", _) => coerce_enum(lua, &this.ctx, key, &old, &value)?,
        (w, _) if w == got => value,
        _ => return Err(mismatch(key, &want, &got)),
    };
    let changed = !lua_equal(&old, &value);
    this.values.borrow_mut().insert(key.to_owned(), value);
    if changed {
        let key_owned = key.to_owned();
        signal::fire(lua, &this.signal(format!("Changed:{key_owned}")).0, &[])?;
        signal::fire(lua, &this.signal("Changed").0, &[Arg::String(key_owned)])?;
        if this.class == "Studio" && key == "Theme" {
            signal::fire(lua, &this.signal("ThemeChanged").0, &[])?;
        }
    }
    Ok(())
}

fn mismatch(key: &str, want: &str, got: &str) -> mlua::Error {
    mlua::Error::runtime(format!(
        "Unable to assign property {key}. {want} expected, got {got}"
    ))
}

fn lua_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::UserData(x), Value::UserData(y)) => x == y || x.to_pointer() == y.to_pointer(),
        _ => a == b,
    }
}

/// An `EnumItem` property takes an item of its enum, or that item's name or
/// value.
fn coerce_enum(lua: &Lua, ctx: &Ctx, key: &str, old: &Value, value: &Value) -> Result<Value> {
    let enum_name = old
        .as_userdata()
        .and_then(|u| u.borrow::<LuaEnumItem>().ok())
        .map(|item| item.enum_name().to_owned())
        .ok_or_else(|| mlua::Error::runtime("enum expected"))?;
    let db = ctx.database();
    let fail =
        |what: String| mlua::Error::runtime(format!("Unable to assign property {key}. {what}"));
    let (name, number) = match value {
        Value::UserData(u) => match u.borrow::<LuaEnumItem>() {
            Ok(item) if item.enum_name() == enum_name => {
                (item.item_name().to_owned(), item.value())
            }
            Ok(item) => {
                return Err(fail(format!(
                    "EnumItem of type {enum_name} expected, got an EnumItem of type {}",
                    item.enum_name()
                )))
            }
            Err(_) => {
                return Err(fail(format!(
                    "EnumItem, number, or string expected, got {}",
                    type_name(lua, value)?
                )))
            }
        },
        Value::String(s) => {
            let name = s.to_string_lossy().to_string();
            match db.enum_value(&enum_name, &name) {
                Some(v) => (name, v),
                None => {
                    return Err(fail(format!(
                        "Invalid value \"{name}\" for enum {enum_name}"
                    )))
                }
            }
        }
        Value::Number(_) | Value::Integer(_) => {
            let number = value
                .as_f64()
                .or(value.as_i64().map(|i| i as f64))
                .unwrap_or(-1.0);
            let shown = value.to_string()?;
            match db
                .enum_name(&enum_name, number as u32)
                .filter(|_| number >= 0.0)
            {
                Some(name) => (name.to_owned(), number as u32),
                None => return Err(fail(format!("Invalid value {shown} for enum {enum_name}"))),
            }
        }
        _ => {
            return Err(fail(format!(
                "EnumItem, number, or string expected, got {}",
                type_name(lua, value)?
            )))
        }
    };
    LuaEnumItem::new(enum_name, name, number, ctx.clone()).interned(lua)
}

/// Installs `settings()` and `UserSettings()` (made on first call, one
/// tree per VM).
pub(crate) fn install(lua: &Lua, ctx: &Ctx) -> Result<()> {
    let ctx_global = ctx.clone();
    let global = lua.create_function(move |lua, ()| {
        cached(lua, "rbxn.settings", || {
            let root =
                SettingsObject::make(lua, &ctx_global, "GlobalSettings", "Global Settings", None)?;
            for (class, name) in GLOBAL {
                let child =
                    SettingsObject::make(lua, &ctx_global, class, name, Some(root.clone()))?;
                root.borrow::<SettingsObject>()?
                    .children
                    .borrow_mut()
                    .push(child);
            }
            super::theme::attach(lua, &ctx_global, &root)?;
            Ok(root)
        })
    })?;
    let ctx_user = ctx.clone();
    let user = lua.create_function(move |lua, ()| {
        cached(lua, "rbxn.user_settings", || {
            let root =
                SettingsObject::make(lua, &ctx_user, "UserSettings", "GlobalBasicSettings", None)?;
            let game = SettingsObject::make(
                lua,
                &ctx_user,
                "UserGameSettings",
                "GameSettings",
                Some(root.clone()),
            )?;
            root.borrow::<SettingsObject>()?
                .children
                .borrow_mut()
                .push(game);
            Ok(root)
        })
    })?;
    lua.globals().set("settings", global)?;
    lua.globals().set("UserSettings", user)
}

fn cached(lua: &Lua, key: &str, make: impl FnOnce() -> Result<AnyUserData>) -> Result<AnyUserData> {
    if let Some(found) = lua.named_registry_value::<Option<AnyUserData>>(key)? {
        return Ok(found);
    }
    let made = make()?;
    lua.set_named_registry_value(key, made.clone())?;
    Ok(made)
}

/// Sets a settings value from the host (the editor's theme), firing the
/// same signals a script's write would.
pub(crate) fn set_value(lua: &Lua, object: &AnyUserData, key: &str, value: Value) -> Result<()> {
    let this = object.borrow::<SettingsObject>()?;
    this.set_initial(key, value);
    signal::fire(lua, &this.signal(format!("Changed:{key}")).0, &[])?;
    signal::fire(
        lua,
        &this.signal("Changed").0,
        &[Arg::String(key.to_owned())],
    )
}

/// A child settings object by class (`Studio`, `RenderSettings`, …).
pub(crate) fn child(object: &AnyUserData, class: &str) -> Option<AnyUserData> {
    object.borrow::<SettingsObject>().ok()?.child(class)
}

pub(crate) fn new_object(
    lua: &Lua,
    ctx: &Ctx,
    class: &str,
    name: &str,
    parent: Option<AnyUserData>,
) -> Result<AnyUserData> {
    SettingsObject::make(lua, ctx, class, name, parent)
}

pub(crate) fn name_of(object: &SettingsObject) -> &str {
    &object.name
}
