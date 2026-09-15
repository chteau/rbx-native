//! `PhysicalProperties` datatype.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use rbx_dom::PhysicalProperties;

use super::from_userdata;

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
        lua.create_function(
            |_,
             (density, friction, elasticity, friction_weight, elasticity_weight): (
                f32,
                f32,
                f32,
                Option<f32>,
                Option<f32>,
            )| {
                Ok(LuaPhysicalProperties(PhysicalProperties::Custom {
                    density,
                    friction,
                    elasticity,
                    friction_weight: friction_weight.unwrap_or(1.0),
                    elasticity_weight: elasticity_weight.unwrap_or(1.0),
                }))
            },
        )?,
    )?;
    Ok(table)
}
