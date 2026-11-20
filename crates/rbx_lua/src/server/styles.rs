//! UI styling at run time: `GetStyled`, the styled-change signals, and the
//! `StyleRule`/`StyleSheet` methods. The rules themselves are
//! `rbx_dom::style`'s, the ones the renderer applies; this keeps what only
//! a running game knows (StyleRuntime golden): a property a script set wins
//! over every rule, and reading the property gives the unstyled value while
//! `GetStyled` gives the styled one.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use mlua::{Function, IntoLua, Lua, Result, Table, Value};
use rbx_dom::{Ref, Variant};

use crate::ctx::Ctx;
use crate::engine::engine;
use crate::engine::events::instance_key;
use crate::engine::signal::{self, LuaSignal};
use crate::instance::LuaInstance;
use crate::lua_enum::LuaEnumItem;
use crate::property::{self, from_lua::lua_to_variant, to_lua::variant_to_lua};

const PROPERTIES: &str = "PropertiesSerialize";

#[derive(Default)]
pub(super) struct Styles {
    /// Properties a script set: they win over the sheets.
    overrides: HashSet<(Ref, String)>,
    /// Instances whose styled-change signals were asked for, with their
    /// styled values when last looked at.
    watched: HashMap<Ref, BTreeMap<String, Variant>>,
}

pub(super) type Shared = Rc<RefCell<Styles>>;

/// A script wrote `key` on `referent`.
pub(super) fn wrote(styles: &Shared, ctx: &Ctx, referent: Ref, class: &str, key: &str) {
    if ctx.database().is_subclass_of(class, "GuiBase2d") {
        styles
            .borrow_mut()
            .overrides
            .insert((referent, key.to_owned()));
    }
}

/// What the sheets give `referent`, less what a script set.
fn effective(ctx: &Ctx, styles: &Styles, referent: Ref) -> BTreeMap<String, Variant> {
    let mut map = rbx_dom::style::styled(&ctx.dom(), referent);
    map.retain(|name, _| !styles.overrides.contains(&(referent, name.clone())));
    map
}

fn watch(ctx: &Ctx, styles: &Shared, referent: Ref) {
    if styles.borrow().watched.contains_key(&referent) {
        return;
    }
    let now = effective(ctx, &styles.borrow(), referent);
    styles.borrow_mut().watched.insert(referent, now);
}

/// Once a frame: the styled values that changed fire their signals.
pub(super) fn poll(lua: &Lua, styles: &Shared) -> Result<()> {
    if styles.borrow().watched.is_empty() {
        return Ok(());
    }
    let ctx = engine(lua).ctx.clone();
    let mut fire = Vec::new();
    {
        let mut all = styles.borrow_mut();
        let referents: Vec<Ref> = all.watched.keys().copied().collect();
        for referent in referents {
            if ctx.dom().get(referent).is_none() {
                all.watched.remove(&referent);
                continue;
            }
            let now = effective(&ctx, &all, referent);
            let was = all
                .watched
                .insert(referent, now.clone())
                .unwrap_or_default();
            for name in was.keys().chain(now.keys()).collect::<HashSet<_>>() {
                if was.get(name) != now.get(name) {
                    fire.push((referent, name.clone()));
                }
            }
        }
    }
    fire.sort_by(|a, b| a.1.cmp(&b.1));
    for (referent, name) in fire {
        signal::fire(
            lua,
            &instance_key(referent, format!("StyledChanged:{name}")),
            &[],
        )?;
        signal::fire(lua, &instance_key(referent, "StyledPropertiesChanged"), &[])?;
    }
    Ok(())
}

fn class_of(ctx: &Ctx, referent: Ref) -> String {
    ctx.dom()
        .get(referent)
        .map(|i| i.class().to_owned())
        .unwrap_or_default()
}

fn get_styled(lua: &Lua, ctx: &Ctx, styles: &Shared, referent: Ref, name: &str) -> Result<Value> {
    let class = class_of(ctx, referent);
    let Some(value_type) = ctx
        .database()
        .resolve_property(&class, name)
        .map(|d| d.value_type.clone())
    else {
        return Err(mlua::Error::runtime(format!(
            "GetStyled(): No such property named `{name}`"
        )));
    };
    let styled = effective(ctx, &styles.borrow(), referent).remove(name);
    match styled {
        Some(variant) => variant_to_lua(lua, ctx, name, &value_type, &variant),
        None => Ok(property::get(lua, ctx, referent, name)?.unwrap_or(Value::Nil)),
    }
}

/// A rule property value as the rule stores it, with its enum's name.
fn rule_value(
    lua: &Lua,
    ctx: &Ctx,
    name: &str,
    value: &Value,
) -> Result<(Variant, Option<String>)> {
    Ok(match value {
        Value::Boolean(b) => (Variant::Bool(*b), None),
        Value::Integer(n) => (Variant::Float64(*n as f64), None),
        Value::Number(n) => (Variant::Float64(*n), None),
        Value::String(s) => (Variant::String(s.to_str()?.to_string()), None),
        Value::UserData(data) if data.is::<LuaEnumItem>() => {
            let item = data.borrow::<LuaEnumItem>()?;
            (
                Variant::Enum(item.value()),
                Some(item.enum_name().to_owned()),
            )
        }
        _ => {
            let kind: String = lua
                .globals()
                .get::<Function>("typeof")?
                .call(value.clone())?;
            (lua_to_variant(ctx, name, &kind, value)?, None)
        }
    })
}

fn rule_properties(ctx: &Ctx, rule: Ref) -> (BTreeMap<String, Variant>, BTreeMap<String, String>) {
    let dom = ctx.dom();
    let blob = dom.get(rule).and_then(|i| i.properties().get(PROPERTIES));
    (
        rbx_dom::attributes::decode(blob),
        rbx_dom::attributes::enum_types(blob),
    )
}

fn rule_get(lua: &Lua, ctx: &Ctx, rule: Ref, name: &str) -> Result<Value> {
    let (values, enums) = rule_properties(ctx, rule);
    match values.get(name) {
        Some(variant) => {
            let kind = enums.get(name).map_or("", String::as_str);
            variant_to_lua(lua, ctx, name, kind, variant)
        }
        None => Ok(Value::Nil),
    }
}

fn rule_set(lua: &Lua, ctx: &Ctx, rule: Ref, name: &str, value: &Value) -> Result<()> {
    ctx.check_write(rule)?;
    let new = match value {
        Value::Nil => None,
        other => Some(rule_value(lua, ctx, name, other)?),
    };
    let old = ctx
        .dom()
        .get(rule)
        .and_then(|i| i.properties().get(PROPERTIES).cloned());
    let (variant, enum_type) = new.unzip();
    let blob = rbx_dom::attributes::set_attribute(
        old.as_ref(),
        name,
        variant.as_ref(),
        enum_type.flatten().as_deref(),
    )
    .ok_or_else(|| mlua::Error::runtime(format!("SetProperty(): cannot store `{name}`")))?;
    ctx.dom_mut()
        .set_property(rule, PROPERTIES, blob)
        .map_err(|e| mlua::Error::runtime(e.to_string()))?;
    Ok(())
}

fn children_of(ctx: &Ctx, referent: Ref, class: &str) -> Vec<Ref> {
    let dom = ctx.dom();
    dom.get(referent)
        .map(|i| {
            i.children()
                .iter()
                .copied()
                .filter(|c| dom.get(*c).is_some_and(|c| c.class() == class))
                .collect()
        })
        .unwrap_or_default()
}

fn instances(lua: &Lua, ctx: &Ctx, list: Vec<Ref>) -> Result<Vec<Value>> {
    list.into_iter()
        .map(|r| LuaInstance::new(r, ctx.clone()).into_lua(lua))
        .collect()
}

fn instance_arg(value: &Value) -> Result<Ref> {
    value
        .as_userdata()
        .and_then(|u| u.borrow::<LuaInstance>().ok())
        .map(|i| i.referent())
        .ok_or_else(|| mlua::Error::runtime("Instance expected"))
}

fn set_parent(ctx: &Ctx, child: Ref, parent: Ref) -> Result<()> {
    ctx.check_write(child)?;
    ctx.dom_mut().set_parent(child, Some(parent));
    Ok(())
}

/// `instance[key]` for the styling members, `None` for the rest.
pub(super) fn member(
    lua: &Lua,
    ctx: &Ctx,
    styles: &Shared,
    referent: Ref,
    class: &str,
    key: &str,
) -> Result<Option<Value>> {
    let ctx = ctx.clone();
    let styles = styles.clone();
    let signal = |name: String| -> Result<Option<Value>> {
        lua.create_userdata(LuaSignal(instance_key(referent, name)))
            .map(|u| Some(Value::UserData(u)))
    };
    let f = match key {
        "GetStyled" => lua.create_function(move |lua, (_this, name): (Value, String)| {
            get_styled(lua, &ctx, &styles, referent, &name)
        })?,
        "GetStyledPropertyChangedSignal" => {
            lua.create_function(move |_, (_this, name): (Value, String)| {
                watch(&ctx, &styles, referent);
                Ok(LuaSignal(instance_key(
                    referent,
                    format!("StyledChanged:{name}"),
                )))
            })?
        }
        "StyledPropertiesChanged" => {
            watch(&ctx, &styles, referent);
            return signal(key.to_owned());
        }
        "StyleRulesChanged" if matches!(class, "StyleRule" | "StyleSheet") => {
            return signal(key.to_owned())
        }
        "GetStyleRules" if matches!(class, "StyleRule" | "StyleSheet") => {
            lua.create_function(move |lua, _this: Value| {
                instances(lua, &ctx, children_of(&ctx, referent, "StyleRule"))
            })?
        }
        "InsertStyleRule" if matches!(class, "StyleRule" | "StyleSheet") => {
            lua.create_function(move |_, (_this, rule): (Value, Value)| {
                set_parent(&ctx, instance_arg(&rule)?, referent)
            })?
        }
        "SetStyleRules" if matches!(class, "StyleRule" | "StyleSheet") => {
            lua.create_function(move |_, (_this, rules): (Value, Vec<Value>)| {
                for old in children_of(&ctx, referent, "StyleRule") {
                    ctx.check_write(old)?;
                    ctx.dom_mut().set_parent(old, None);
                }
                for rule in &rules {
                    set_parent(&ctx, instance_arg(rule)?, referent)?;
                }
                Ok(())
            })?
        }
        "GetDerives" if class == "StyleSheet" => {
            lua.create_function(move |lua, _this: Value| {
                let sheets: Vec<Ref> = {
                    let dom = ctx.dom();
                    children_of(&ctx, referent, "StyleDerive")
                        .into_iter()
                        .filter_map(|d| dom.get(d).and_then(rbx_dom::style::cascade::reference))
                        .filter(|s| dom.get(*s).is_some())
                        .collect()
                };
                instances(lua, &ctx, sheets)
            })?
        }
        "SetDerives" if class == "StyleSheet" => {
            lua.create_function(move |_, (_this, sheets): (Value, Vec<Value>)| {
                for old in children_of(&ctx, referent, "StyleDerive") {
                    ctx.check_write(old)?;
                    ctx.dom_mut().set_parent(old, None);
                }
                for sheet in &sheets {
                    let sheet = instance_arg(sheet)?;
                    let mut dom = ctx.dom_mut();
                    let derive = dom.new_instance("StyleDerive", "StyleDerive", Some(referent));
                    let _ = dom.set_property(derive, "StyleSheet", Variant::Ref(sheet));
                }
                Ok(())
            })?
        }
        "GetProperty" if class == "StyleRule" => {
            lua.create_function(move |lua, (_this, name): (Value, String)| {
                rule_get(lua, &ctx, referent, &name)
            })?
        }
        "GetProperties" if class == "StyleRule" => {
            lua.create_function(move |lua, _this: Value| {
                let table = lua.create_table()?;
                for name in rule_properties(&ctx, referent).0.keys() {
                    table.set(name.as_str(), rule_get(lua, &ctx, referent, name)?)?;
                }
                Ok(table)
            })?
        }
        "SetProperty" if class == "StyleRule" => {
            lua.create_function(move |lua, (_this, name, value): (Value, String, Value)| {
                rule_set(lua, &ctx, referent, &name, &value)
            })?
        }
        "SetProperties" if class == "StyleRule" => {
            lua.create_function(move |lua, (_this, values): (Value, Table)| {
                for (name, _) in rule_properties(&ctx, referent).0 {
                    rule_set(lua, &ctx, referent, &name, &Value::Nil)?;
                }
                for pair in values.pairs::<String, Value>() {
                    let (name, value) = pair?;
                    rule_set(lua, &ctx, referent, &name, &value)?;
                }
                Ok(())
            })?
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Function(f)))
}
