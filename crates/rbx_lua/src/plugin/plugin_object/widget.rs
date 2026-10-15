//! `CreateDockWidgetPluginGui`, `CreateFloatingWidget` and
//! `DockWidgetPluginGuiInfo`.

use mlua::{
    AnyUserData, Lua, MetaMethod, ObjectLike, Result, Table, UserData, UserDataMethods, Value,
};
use rbx_dom::Variant;

use crate::instance::LuaInstance;
use crate::plugin::api::{Arg, Widget, WidgetKind};
use crate::plugin::arg;
use crate::plugin::env::env;

/// `Enum.InitialDockState.Right`, Studio's default.
const DOCK_RIGHT: u8 = 3;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LuaWidgetInfo {
    pub(crate) initial: u8,
    pub(crate) enabled: bool,
    pub(crate) override_restore: bool,
    pub(crate) float: (f32, f32),
    pub(crate) min: (f32, f32),
}

impl Default for LuaWidgetInfo {
    fn default() -> Self {
        LuaWidgetInfo {
            initial: DOCK_RIGHT,
            enabled: false,
            override_restore: false,
            float: (0.0, 0.0),
            min: (0.0, 0.0),
        }
    }
}

/// An `Enum.InitialDockState` item or its number.
fn dock_state(value: &Value) -> Option<u8> {
    match value {
        Value::Integer(i) => Some(*i as u8),
        Value::Number(n) => Some(*n as u8),
        Value::UserData(ud) => ud.get::<f64>("Value").ok().map(|v| v as u8),
        _ => None,
    }
}

fn number(value: &Value) -> Option<f32> {
    match value {
        Value::Integer(i) => Some(*i as f32),
        Value::Number(n) => Some(*n as f32),
        _ => None,
    }
}

impl UserData for LuaWidgetInfo {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |_, this, key: String| {
            Ok(match key.as_str() {
                "InitialDockState" => Value::Number(this.initial as f64),
                "InitialEnabled" => Value::Boolean(this.enabled),
                "InitialEnabledShouldOverrideRestore" => Value::Boolean(this.override_restore),
                "FloatingXSize" => Value::Number(this.float.0 as f64),
                "FloatingYSize" => Value::Number(this.float.1 as f64),
                "MinWidth" => Value::Number(this.min.0 as f64),
                "MinHeight" => Value::Number(this.min.1 as f64),
                _ => {
                    return Err(mlua::Error::runtime(format!(
                        "{key} is not a valid member of DockWidgetPluginGuiInfo"
                    )))
                }
            })
        });
        methods.add_meta_method_mut(
            MetaMethod::NewIndex,
            |_, this, (key, value): (String, Value)| {
                let bad = || {
                    mlua::Error::runtime(format!("invalid value for DockWidgetPluginGuiInfo.{key}"))
                };
                match key.as_str() {
                    "InitialDockState" => this.initial = dock_state(&value).ok_or_else(bad)?,
                    "InitialEnabled" => this.enabled = value.as_boolean().ok_or_else(bad)?,
                    "InitialEnabledShouldOverrideRestore" => {
                        this.override_restore = value.as_boolean().ok_or_else(bad)?
                    }
                    "FloatingXSize" => this.float.0 = number(&value).ok_or_else(bad)?,
                    "FloatingYSize" => this.float.1 = number(&value).ok_or_else(bad)?,
                    "MinWidth" => this.min.0 = number(&value).ok_or_else(bad)?,
                    "MinHeight" => this.min.1 = number(&value).ok_or_else(bad)?,
                    _ => return Err(bad()),
                }
                Ok(())
            },
        );
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| {
            Ok("DockWidgetPluginGuiInfo")
        });
    }
}

/// The `DockWidgetPluginGuiInfo` global.
pub(super) fn info_global(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, args: mlua::Variadic<Value>| {
            let at = |i: usize| args.get(i).cloned().unwrap_or(Value::Nil);
            let mut info = LuaWidgetInfo::default();
            if let Some(state) = dock_state(&at(0)) {
                info.initial = state;
            }
            info.enabled = at(1).as_boolean().unwrap_or(false);
            info.override_restore = at(2).as_boolean().unwrap_or(false);
            info.float = (number(&at(3)).unwrap_or(0.0), number(&at(4)).unwrap_or(0.0));
            info.min = (number(&at(5)).unwrap_or(0.0), number(&at(6)).unwrap_or(0.0));
            Ok(info)
        })?,
    )?;
    Ok(table)
}

fn create(lua: &Lua, key: String, enabled: bool, kind: WidgetKind) -> LuaInstance {
    let env = env(lua);
    let existing = env
        .shared
        .borrow()
        .ui
        .widgets
        .iter()
        .find(|w| w.plugin == env.plugin && w.key == key)
        .map(|w| w.root);
    if let Some(root) = existing {
        return LuaInstance::new(root, env.ctx.clone());
    }
    let root = {
        let mut dom = env.ctx.dom_mut();
        let root = dom.new_instance("DockWidgetPluginGui", &key, Some(env.plugin_gui_service));
        // Freshly created, so the referent is valid and these cannot fail.
        let _ = dom.set_property(root, "Title", Variant::String(key.clone()));
        let _ = dom.set_property(root, "Enabled", Variant::Bool(enabled));
        root
    };
    let mut shared = env.shared.borrow_mut();
    let id = shared.next_id();
    shared.ui.widgets.push(Widget {
        id,
        plugin: env.plugin,
        title: key.clone(),
        key,
        enabled,
        root,
        kind,
        revision: 0,
    });
    shared.widget_roots.insert(root);
    shared.owners.insert(id, env.plugin);
    shared.ui_changed = true;
    LuaInstance::new(root, env.ctx.clone())
}

pub(super) fn create_dock(
    lua: &Lua,
    key: String,
    info: Option<AnyUserData>,
) -> Result<LuaInstance> {
    let info = match info {
        Some(ud) => ud.borrow::<LuaWidgetInfo>()?.clone(),
        None => LuaWidgetInfo::default(),
    };
    let kind = WidgetKind::Dock {
        initial: info.initial,
        float_size: info.float,
        min_size: info.min,
    };
    Ok(create(lua, key, info.enabled, kind))
}

pub(super) fn create_floating(
    lua: &Lua,
    key: String,
    options: Option<Table>,
) -> Result<LuaInstance> {
    let fields = match options.map(|t| arg::from_lua(&Value::Table(t))) {
        Some(Arg::Table(fields)) => fields,
        _ => Vec::new(),
    };
    let field = |name: &str| {
        fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };
    let size = match field("Size") {
        Some(Arg::Vector2([x, y])) => (x, y),
        _ => {
            return Err(mlua::Error::runtime(
                "CreateFloatingWidget needs a Size Vector2",
            ))
        }
    };
    let position = match field("Position") {
        Some(Arg::Vector2([x, y])) => Some((x, y)),
        _ => None,
    };
    let light_dismiss = matches!(field("LightDismiss"), Some(Arg::Bool(true)));
    let kind = WidgetKind::Floating {
        position,
        size,
        light_dismiss,
    };
    Ok(create(lua, key, true, kind))
}
