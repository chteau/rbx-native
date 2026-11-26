//! `PhysicalProperties` datatype.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::PhysicalProperties;

use super::from_userdata;
use crate::lua_enum::LuaEnumItem;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LuaPhysicalProperties(pub(crate) PhysicalProperties);

impl LuaPhysicalProperties {
    fn fields(&self) -> (f32, f32, f32, f32, f32) {
        match self.0 {
            PhysicalProperties::Custom {
                density,
                friction,
                elasticity,
                friction_weight,
                elasticity_weight,
            } => (
                density,
                friction,
                elasticity,
                friction_weight,
                elasticity_weight,
            ),
            // `.new` always builds `Custom`, and `to_lua` maps `Default` to `nil`
            // before it can reach a script, but this keeps the accessor total.
            PhysicalProperties::Default => (0.0, 0.0, 0.0, 0.0, 0.0),
        }
    }
}

impl UserData for LuaPhysicalProperties {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "PhysicalProperties");
        fields.add_field_method_get("Density", |_, this| Ok(this.fields().0));
        fields.add_field_method_get("Friction", |_, this| Ok(this.fields().1));
        fields.add_field_method_get("Elasticity", |_, this| Ok(this.fields().2));
        fields.add_field_method_get("FrictionWeight", |_, this| Ok(this.fields().3));
        fields.add_field_method_get("ElasticityWeight", |_, this| Ok(this.fields().4));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: LuaPhysicalProperties| {
            Ok(*this == other)
        });
    }
}

impl mlua::FromLua for LuaPhysicalProperties {
    fn from_lua(value: Value, _: &Lua) -> Result<Self> {
        from_userdata(&value, "PhysicalProperties")
    }
}

pub(crate) fn constructors(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|_, args: mlua::MultiValue| {
            // `PhysicalProperties.new(material)`: that material's values.
            if let Some(Value::UserData(data)) = args.front() {
                let item = data.borrow::<LuaEnumItem>().map_err(|_| {
                    mlua::Error::runtime("bad argument #1 to 'new' (Material expected)")
                })?;
                let [density, friction, elasticity, friction_weight, elasticity_weight] =
                    rbx_materials::physics::PHYSICS
                        .iter()
                        .find(|(name, _)| *name == item.name())
                        .map_or([0.7, 0.3, 0.5, 1.0, 1.0], |(_, values)| *values);
                return Ok(LuaPhysicalProperties(PhysicalProperties::Custom {
                    density,
                    friction,
                    elasticity,
                    friction_weight,
                    elasticity_weight,
                }));
            }
            let number = |i: usize| -> Result<Option<f32>> {
                match args.get(i) {
                    None | Some(Value::Nil) => Ok(None),
                    Some(Value::Integer(n)) => Ok(Some(*n as f32)),
                    Some(Value::Number(n)) => Ok(Some(*n as f32)),
                    Some(_) => Err(mlua::Error::runtime(format!(
                        "bad argument #{} to 'new' (number expected)",
                        i + 1
                    ))),
                }
            };
            let required = |i: usize| {
                number(i)?.ok_or_else(|| {
                    mlua::Error::runtime(format!(
                        "bad argument #{} to 'new' (number expected)",
                        i + 1
                    ))
                })
            };
            Ok(LuaPhysicalProperties(PhysicalProperties::Custom {
                density: required(0)?,
                friction: required(1)?,
                elasticity: required(2)?,
                friction_weight: number(3)?.unwrap_or(1.0),
                elasticity_weight: number(4)?.unwrap_or(1.0),
            }))
        })?,
    )?;
    Ok(table)
}
