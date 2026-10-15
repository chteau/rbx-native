//! [`Arg`] ⇄ Luau values.

use mlua::{IntoLua, Lua, ObjectLike, Result, Value};

use super::api::Arg;
use crate::ctx::Ctx;
use crate::datatypes::color3::LuaColor3;
use crate::datatypes::misc::{LuaBrickColor, LuaVector2};
use crate::datatypes::vector3::LuaVector3;
use crate::instance::LuaInstance;
use crate::lua_enum::LuaEnumItem;

const MAX_DEPTH: usize = 16;

/// `Arg::Instance` → instance userdata on `ctx`; `Enum` → the enum item
/// (resolved through the reflection database; an unknown item is an
/// error); `Table`/`List` → fresh tables; `Vector2`/`Vector3`/`Color3`/
/// `BrickColor` → their datatype userdata.
pub(crate) fn to_lua(lua: &Lua, ctx: &Ctx, arg: &Arg) -> Result<Value> {
    match arg {
        Arg::Nil => Ok(Value::Nil),
        Arg::Bool(b) => Ok(Value::Boolean(*b)),
        Arg::Number(n) => Ok(Value::Number(*n)),
        Arg::String(s) => Ok(Value::String(lua.create_string(s)?)),
        Arg::Instance(r) => LuaInstance::new(*r, ctx.clone()).into_lua(lua),
        Arg::Instances(refs) => {
            let table = lua.create_table()?;
            for (i, r) in refs.iter().enumerate() {
                table.set(i + 1, LuaInstance::new(*r, ctx.clone()))?;
            }
            Ok(Value::Table(table))
        }
        Arg::Vector2([x, y]) => lua
            .create_userdata(LuaVector2(rbx_dom::Vector2Data { x: *x, y: *y }))
            .map(Value::UserData),
        Arg::Vector3([x, y, z]) => lua
            .create_userdata(LuaVector3::new(*x, *y, *z))
            .map(Value::UserData),
        Arg::Color3([r, g, b]) => lua
            .create_userdata(LuaColor3::new(*r, *g, *b))
            .map(Value::UserData),
        Arg::BrickColor(n) => lua
            .create_userdata(LuaBrickColor::from_number(*n))
            .map(Value::UserData),
        Arg::Enum { enum_name, item } => {
            let value = ctx
                .database()
                .enum_items(enum_name)
                .and_then(|items| {
                    items
                        .iter()
                        .find(|(name, _)| *name == item.as_str())
                        .map(|(_, value)| *value)
                })
                .ok_or_else(|| {
                    mlua::Error::runtime(format!(
                        "{item} is not a valid member of Enum.{enum_name}"
                    ))
                })?;
            LuaEnumItem::new(enum_name.clone(), item.clone(), value, ctx.clone()).interned(lua)
        }
        Arg::Table(entries) => {
            let table = lua.create_table()?;
            for (key, value) in entries {
                table.set(key.as_str(), to_lua(lua, ctx, value)?)?;
            }
            Ok(Value::Table(table))
        }
        Arg::List(items) => {
            let table = lua.create_table()?;
            for (i, value) in items.iter().enumerate() {
                table.set(i + 1, to_lua(lua, ctx, value)?)?;
            }
            Ok(Value::Table(table))
        }
    }
}

/// The inverse, best effort: instances, datatypes, enum items, numbers,
/// strings, booleans; a table with only 1..n keys is a `List`, any other
/// table a `Table` (keys converted with `tostring`); functions, threads
/// and unknown userdata become `Nil`. Recursion stops at depth 16.
pub(crate) fn from_lua(value: &Value) -> Arg {
    convert(value, 0)
}

fn convert(value: &Value, depth: usize) -> Arg {
    match value {
        Value::Boolean(b) => Arg::Bool(*b),
        Value::Integer(i) => Arg::Number(*i as f64),
        Value::Number(n) => Arg::Number(*n),
        Value::String(s) => Arg::String(s.to_string_lossy().to_string()),
        Value::Table(table) => {
            if depth >= MAX_DEPTH {
                return Arg::Nil;
            }
            let pairs: Vec<(Value, Value)> = table
                .pairs::<Value, Value>()
                .filter_map(|p| p.ok())
                .collect();
            let len = pairs.len();
            let is_list = pairs.iter().all(|(k, _)| match k {
                Value::Integer(i) => *i >= 1 && (*i as usize) <= len,
                Value::Number(n) => n.fract() == 0.0 && *n >= 1.0 && *n <= len as f64,
                _ => false,
            });
            if is_list {
                let mut items = vec![Arg::Nil; len];
                for (k, v) in &pairs {
                    let index = match k {
                        Value::Integer(i) => *i as usize,
                        Value::Number(n) => *n as usize,
                        _ => continue,
                    };
                    items[index - 1] = convert(v, depth + 1);
                }
                Arg::List(items)
            } else {
                Arg::Table(
                    pairs
                        .iter()
                        .map(|(k, v)| (key_string(k), convert(v, depth + 1)))
                        .collect(),
                )
            }
        }
        Value::UserData(ud) => {
            if let Ok(i) = ud.borrow::<LuaInstance>() {
                Arg::Instance(i.referent())
            } else if let Ok(v) = ud.borrow::<LuaVector3>() {
                Arg::Vector3([v.0.x, v.0.y, v.0.z])
            } else if let Ok(v) = ud.borrow::<LuaVector2>() {
                Arg::Vector2([v.0.x, v.0.y])
            } else if let Ok(c) = ud.borrow::<LuaColor3>() {
                Arg::Color3([c.0.r, c.0.g, c.0.b])
            } else if let Ok(b) = ud.borrow::<LuaBrickColor>() {
                Arg::BrickColor(b.number())
            } else if let Ok(e) = ud.borrow::<LuaEnumItem>() {
                let enum_name = e.enum_name().to_string();
                drop(e);
                // The item keeps its name private; its `Name` field is the
                // one public way back to it without a database at hand.
                let item = ud.get::<String>("Name").unwrap_or_default();
                Arg::Enum { enum_name, item }
            } else {
                Arg::Nil
            }
        }
        _ => Arg::Nil,
    }
}

fn key_string(key: &Value) -> String {
    match key {
        Value::String(s) => s.to_string_lossy().to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

/// Several values at once, for `Signal:Fire` and friends.
pub(crate) fn to_lua_multi(lua: &Lua, ctx: &Ctx, args: &[Arg]) -> Result<mlua::MultiValue> {
    args.iter().map(|arg| to_lua(lua, ctx, arg)).collect()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use rbx_dom::WeakDom;
    use rbx_reflection::ReflectionDatabase;

    use super::*;

    fn setup() -> (Lua, Ctx, rbx_dom::Ref) {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let ctx = Ctx::new(
            Rc::new(RefCell::new(dom)),
            Rc::new(ReflectionDatabase::embedded()),
            Rc::new(RefCell::new(Vec::new())),
            Rc::new(RefCell::new(Vec::new())),
        );
        (Lua::new(), ctx, workspace)
    }

    fn round(arg: Arg) -> Arg {
        let (lua, ctx, _) = setup();
        from_lua(&to_lua(&lua, &ctx, &arg).unwrap())
    }

    #[test]
    fn scalars_round_trip() {
        for arg in [
            Arg::Bool(true),
            Arg::Number(2.5),
            Arg::String("hi".into()),
            Arg::Vector2([1.0, 2.0]),
            Arg::Vector3([1.0, 2.0, 3.0]),
            Arg::Color3([0.25, 0.5, 1.0]),
        ] {
            assert_eq!(round(arg.clone()), arg);
        }
        assert_eq!(round(Arg::Nil), Arg::Nil);
    }

    #[test]
    fn brick_color_round_trips() {
        assert_eq!(round(Arg::BrickColor(194)), Arg::BrickColor(194));
    }

    #[test]
    fn instance_and_instances() {
        let (lua, ctx, ws) = setup();
        let v = to_lua(&lua, &ctx, &Arg::Instance(ws)).unwrap();
        assert_eq!(from_lua(&v), Arg::Instance(ws));
        let v = to_lua(&lua, &ctx, &Arg::Instances(vec![ws, ws])).unwrap();
        assert_eq!(
            from_lua(&v),
            Arg::List(vec![Arg::Instance(ws), Arg::Instance(ws)])
        );
    }

    #[test]
    fn enum_round_trips_and_unknown_errors() {
        let arg = Arg::Enum {
            enum_name: "Material".into(),
            item: "Plastic".into(),
        };
        assert_eq!(round(arg.clone()), arg);
        let (lua, ctx, _) = setup();
        let bad = Arg::Enum {
            enum_name: "Material".into(),
            item: "Nope".into(),
        };
        assert!(to_lua(&lua, &ctx, &bad).is_err());
    }

    #[test]
    fn table_and_list_round_trip() {
        let list = Arg::List(vec![Arg::Number(1.0), Arg::String("a".into())]);
        assert_eq!(round(list.clone()), list);
        let table = Arg::Table(vec![("k".into(), Arg::Bool(false))]);
        assert_eq!(round(table.clone()), table);
    }

    #[test]
    fn functions_become_nil_and_depth_is_capped() {
        let (lua, _, _) = setup();
        let f = lua.load("return function() end").eval::<Value>().unwrap();
        assert_eq!(from_lua(&f), Arg::Nil);
        let deep = lua
            .load("local t = {} local c = t for i = 1, 40 do local n = {} c[1] = n c = n end return t")
            .eval::<Value>()
            .unwrap();
        let mut arg = from_lua(&deep);
        let mut levels = 0;
        while let Arg::List(items) = arg {
            levels += 1;
            arg = items.into_iter().next().unwrap_or(Arg::Nil);
        }
        assert!(levels <= MAX_DEPTH + 1);
    }

    #[test]
    fn multi_converts_each() {
        let (lua, ctx, _) = setup();
        let m = to_lua_multi(&lua, &ctx, &[Arg::Number(1.0), Arg::Bool(true)]).unwrap();
        assert_eq!(m.len(), 2);
    }
}
