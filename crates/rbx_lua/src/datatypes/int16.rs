//! `Vector2int16` and `Vector3int16`: integer vectors, wrapping at 16 bits
//! as Roblox's do.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaVector2int16(pub(crate) [i16; 2]);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaVector3int16(pub(crate) [i16; 3]);

fn wrap(n: f64) -> i16 {
    (n as i64) as i16
}

macro_rules! int_vector {
    ($ty:ident, $name:literal, $n:literal, [$($axis:literal => $i:literal),*]) => {
        impl UserData for $ty {
            fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
                fields.add_meta_field(MetaMethod::Type, $name);
                $(fields.add_field_method_get($axis, |_, this| Ok(this.0[$i]));)*
            }

            fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
                let op = |f: fn(i16, i16) -> i16| {
                    move |_: &Lua, this: &$ty, rhs: Value| -> Result<$ty> {
                        let other = match &rhs {
                            Value::UserData(u) => u.borrow::<$ty>()?.0,
                            Value::Integer(n) => [*n as i16; $n],
                            Value::Number(n) => [wrap(*n); $n],
                            _ => return Err(mlua::Error::runtime(concat!($name, " expected"))),
                        };
                        let mut out = this.0;
                        for (o, b) in out.iter_mut().zip(other) {
                            *o = f(*o, b);
                        }
                        Ok($ty(out))
                    }
                };
                methods.add_meta_method(MetaMethod::Add, op(i16::wrapping_add));
                methods.add_meta_method(MetaMethod::Sub, op(i16::wrapping_sub));
                methods.add_meta_method(MetaMethod::Mul, op(i16::wrapping_mul));
                methods.add_meta_method(MetaMethod::Div, op(|a, b| if b == 0 { 0 } else { a.wrapping_div(b) }));
                methods.add_meta_method(MetaMethod::Eq, |_, this, other: mlua::UserDataRef<$ty>| {
                    Ok(this.0 == other.0)
                });
                methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
                    Ok(this.0.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", "))
                });
            }
        }
    };
}

int_vector!(LuaVector2int16, "Vector2int16", 2, ["X" => 0, "Y" => 1]);
int_vector!(LuaVector3int16, "Vector3int16", 3, ["X" => 0, "Y" => 1, "Z" => 2]);

pub(crate) fn register(lua: &Lua) -> Result<()> {
    let two: Table = lua.create_table()?;
    two.set(
        "new",
        lua.create_function(|_, (x, y): (Option<f64>, Option<f64>)| {
            Ok(LuaVector2int16([
                wrap(x.unwrap_or(0.0)),
                wrap(y.unwrap_or(0.0)),
            ]))
        })?,
    )?;
    let three: Table = lua.create_table()?;
    three.set(
        "new",
        lua.create_function(|_, (x, y, z): (Option<f64>, Option<f64>, Option<f64>)| {
            Ok(LuaVector3int16([
                wrap(x.unwrap_or(0.0)),
                wrap(y.unwrap_or(0.0)),
                wrap(z.unwrap_or(0.0)),
            ]))
        })?,
    )?;
    lua.globals().set("Vector2int16", two)?;
    lua.globals().set("Vector3int16", three)?;
    Ok(())
}
