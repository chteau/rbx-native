//! `Region3`, `Faces` and `Axes`.

use mlua::{
    Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value, Variadic,
};
use rbx_dom::{CFrameData, Vector3Data};

use crate::datatypes::cframe::LuaCFrame;
use crate::datatypes::from_userdata;
use crate::datatypes::vector3::LuaVector3;
use crate::lua_enum::LuaEnumItem;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaRegion3 {
    min: Vector3Data,
    max: Vector3Data,
}

impl UserData for LuaRegion3 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("CFrame", |_, this| {
            Ok(LuaCFrame(CFrameData {
                position: Vector3Data {
                    x: (this.min.x + this.max.x) / 2.0,
                    y: (this.min.y + this.max.y) / 2.0,
                    z: (this.min.z + this.max.z) / 2.0,
                },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }))
        });
        fields.add_field_method_get("Size", |_, this| {
            Ok(LuaVector3::new(
                this.max.x - this.min.x,
                this.max.y - this.min.y,
                this.max.z - this.min.z,
            ))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Snaps outward so the grid-aligned region always contains the original.
        methods.add_method("ExpandToGrid", |_, this, resolution: f32| {
            if resolution <= 0.0 {
                return Err(mlua::Error::runtime(
                    "ExpandToGrid resolution must be positive",
                ));
            }
            let down = |v: f32| (v / resolution).floor() * resolution;
            let up = |v: f32| (v / resolution).ceil() * resolution;
            Ok(LuaRegion3 {
                min: Vector3Data {
                    x: down(this.min.x),
                    y: down(this.min.y),
                    z: down(this.min.z),
                },
                max: Vector3Data {
                    x: up(this.max.x),
                    y: up(this.max.y),
                    z: up(this.max.z),
                },
            })
        });
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaRegion3| {
            Ok(*this == other)
        });
    }
}

impl mlua::FromLua for LuaRegion3 {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "Region3")
    }
}

/// `Enum.NormalId` order: Right, Top, Back, Left, Bottom, Front.
const FACE_NAMES: [&str; 6] = ["Right", "Top", "Back", "Left", "Bottom", "Front"];

fn enum_item_value(value: &Value, enum_name: &str) -> Option<u32> {
    if let Value::UserData(data) = value {
        if let Ok(item) = data.borrow::<LuaEnumItem>() {
            if item.enum_name() == enum_name {
                return Some(item.value());
            }
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct LuaFaces {
    flags: [bool; 6],
}

impl UserData for LuaFaces {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        for (index, name) in FACE_NAMES.iter().enumerate() {
            fields.add_field_method_get(*name, move |_, this| Ok(this.flags[index]));
            fields.add_field_method_set(*name, move |_, this, on: bool| {
                this.flags[index] = on;
                Ok(())
            });
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            MetaMethod::Eq,
            |_, this, other: LuaFaces| Ok(*this == other),
        );
    }
}

impl mlua::FromLua for LuaFaces {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "Faces")
    }
}

/// X, Y, Z.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct LuaAxes {
    flags: [bool; 3],
}

/// The axis each `Enum.NormalId` face lies on, indexed like [`FACE_NAMES`].
const FACE_AXIS: [usize; 6] = [0, 1, 2, 0, 1, 2];

impl UserData for LuaAxes {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        for (index, name) in ["X", "Y", "Z"].iter().enumerate() {
            fields.add_field_method_get(*name, move |_, this| Ok(this.flags[index]));
            fields.add_field_method_set(*name, move |_, this, on: bool| {
                this.flags[index] = on;
                Ok(())
            });
        }
        for (index, name) in FACE_NAMES.iter().enumerate() {
            let axis = FACE_AXIS[index];
            fields.add_field_method_get(*name, move |_, this| Ok(this.flags[axis]));
            fields.add_field_method_set(*name, move |_, this, on: bool| {
                this.flags[axis] = on;
                Ok(())
            });
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaAxes| Ok(*this == other));
    }
}

impl mlua::FromLua for LuaAxes {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "Axes")
    }
}

pub(crate) fn register(lua: &Lua) -> Result<()> {
    let globals = lua.globals();

    let region = lua.create_table()?;
    region.set(
        "new",
        lua.create_function(|_, (min, max): (LuaVector3, LuaVector3)| {
            Ok(LuaRegion3 {
                min: min.0,
                max: max.0,
            })
        })?,
    )?;
    globals.set("Region3", region)?;

    globals.set("Faces", faces_constructors(lua)?)?;
    globals.set("Axes", axes_constructors(lua)?)?;
    Ok(())
}

fn faces_constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, items: Variadic<Value>| {
            let mut faces = LuaFaces::default();
            for item in items.iter() {
                let value = enum_item_value(item, "NormalId")
                    .ok_or_else(|| mlua::Error::runtime("Faces.new expects Enum.NormalId items"))?;
                faces.flags[(value as usize).min(5)] = true;
            }
            Ok(faces)
        })?,
    )?;
    Ok(table)
}

fn axes_constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, items: Variadic<Value>| {
            let mut axes = LuaAxes::default();
            for item in items.iter() {
                if let Some(value) = enum_item_value(item, "Axis") {
                    axes.flags[(value as usize).min(2)] = true;
                } else if let Some(value) = enum_item_value(item, "NormalId") {
                    axes.flags[FACE_AXIS[(value as usize).min(5)]] = true;
                } else {
                    return Err(mlua::Error::runtime(
                        "Axes.new expects Enum.Axis or Enum.NormalId items",
                    ));
                }
            }
            Ok(axes)
        })?,
    )?;
    Ok(table)
}
