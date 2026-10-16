//! `RaycastParams`, `OverlapParams` and `RaycastResult`.
//!
//! The parameter objects are plain mutable state; the spatial queries that
//! consume them read the `pub(crate)` fields directly.

use mlua::{
    AnyUserData, Lua, MetaMethod, ObjectLike, Result, Table, UserData, UserDataFields,
    UserDataMethods, Value,
};
use rbx_dom::{Ref, Vector3Data};

use crate::ctx::Ctx;
use crate::datatypes::vector3::LuaVector3;
use crate::instance::LuaInstance;
use crate::lua_enum::LuaEnumItem;

/// Resolves `Enum.<enum_name>.<item>` through the installed `Enum` global, so
/// the parameter constructors need no handle on the script context.
fn enum_item(lua: &Lua, enum_name: &str, item: &str) -> Result<AnyUserData> {
    let enums: AnyUserData = lua.globals().get("Enum")?;
    let kind: AnyUserData = enums.get(enum_name)?;
    kind.get(item)
}

/// The name of an `Enum.<enum_name>` item, rejecting items of other enums.
fn enum_item_name(value: &Value, enum_name: &str) -> Result<String> {
    if let Value::UserData(data) = value {
        let matches = data
            .borrow::<LuaEnumItem>()
            .is_ok_and(|item| item.enum_name() == enum_name);
        if matches {
            return data.get("Name");
        }
    }
    Err(mlua::Error::runtime(format!(
        "Enum.{enum_name} item expected, got {}",
        value.type_name()
    )))
}

fn instance_arg(value: &Value) -> Result<LuaInstance> {
    if let Value::UserData(data) = value {
        if let Ok(instance) = data.borrow::<LuaInstance>() {
            return Ok(instance.clone());
        }
    }
    Err(mlua::Error::runtime(format!(
        "Instance expected, got {}",
        value.type_name()
    )))
}

fn instances_from_table(table: &Table) -> Result<Vec<LuaInstance>> {
    table
        .sequence_values::<Value>()
        .map(|value| instance_arg(&value?))
        .collect()
}

fn instances_from_value(value: &Value) -> Result<Vec<LuaInstance>> {
    match value {
        Value::Table(table) => instances_from_table(table),
        other => Ok(vec![instance_arg(other)?]),
    }
}

fn is_include(name: &str) -> bool {
    matches!(name, "Include" | "Whitelist")
}

// `held` keeps the script-facing handles (they carry the context a bare `Ref`
// lacks) and `filter` mirrors their referents for the query code.
macro_rules! filter_fields {
    ($fields:ident) => {
        $fields.add_field_method_get("FilterDescendantsInstances", |lua, this| {
            lua.create_sequence_from(this.held.iter().cloned())
        });
        $fields.add_field_method_set("FilterDescendantsInstances", |_, this, table: Table| {
            this.held = instances_from_table(&table)?;
            this.filter = this.held.iter().map(LuaInstance::referent).collect();
            Ok(())
        });
        $fields.add_field_method_get("FilterType", |lua, this| {
            enum_item(
                lua,
                "RaycastFilterType",
                if this.include { "Include" } else { "Exclude" },
            )
        });
        $fields.add_field_method_set("FilterType", |_, this, value: Value| {
            this.include = is_include(&enum_item_name(&value, "RaycastFilterType")?);
            Ok(())
        });
        $fields.add_field_method_get("CollisionGroup", |_, this| Ok(this.collision_group.clone()));
        $fields.add_field_method_set("CollisionGroup", |_, this, group: String| {
            this.collision_group = group;
            Ok(())
        });
        $fields.add_field_method_get("RespectCanCollide", |_, this| Ok(this.respect_can_collide));
        $fields.add_field_method_set("RespectCanCollide", |_, this, on: bool| {
            this.respect_can_collide = on;
            Ok(())
        });
    };
}

macro_rules! filter_methods {
    ($methods:ident) => {
        $methods.add_method_mut("AddToFilter", |_, this, value: Value| {
            let added = instances_from_value(&value)?;
            this.filter.extend(added.iter().map(LuaInstance::referent));
            this.held.extend(added);
            Ok(())
        });
    };
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct LuaRaycastParams {
    pub(crate) filter: Vec<Ref>,
    held: Vec<LuaInstance>,
    pub(crate) include: bool,
    pub(crate) ignore_water: bool,
    pub(crate) collision_group: String,
    pub(crate) respect_can_collide: bool,
    pub(crate) brute_force: bool,
}

impl LuaRaycastParams {
    fn new() -> Self {
        LuaRaycastParams {
            filter: Vec::new(),
            held: Vec::new(),
            include: false,
            ignore_water: false,
            collision_group: "Default".to_string(),
            respect_can_collide: false,
            brute_force: false,
        }
    }
}

impl UserData for LuaRaycastParams {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "RaycastParams");
        filter_fields!(fields);
        fields.add_field_method_get("IgnoreWater", |_, this| Ok(this.ignore_water));
        fields.add_field_method_set("IgnoreWater", |_, this, on: bool| {
            this.ignore_water = on;
            Ok(())
        });
        fields.add_field_method_get("BruteForceAllSlow", |_, this| Ok(this.brute_force));
        fields.add_field_method_set("BruteForceAllSlow", |_, this, on: bool| {
            this.brute_force = on;
            Ok(())
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        filter_methods!(methods);
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct LuaOverlapParams {
    pub(crate) filter: Vec<Ref>,
    held: Vec<LuaInstance>,
    pub(crate) include: bool,
    pub(crate) collision_group: String,
    pub(crate) respect_can_collide: bool,
    pub(crate) max_parts: u32,
}

impl LuaOverlapParams {
    fn new() -> Self {
        LuaOverlapParams {
            filter: Vec::new(),
            held: Vec::new(),
            include: false,
            collision_group: "Default".to_string(),
            respect_can_collide: false,
            max_parts: 0,
        }
    }
}

impl UserData for LuaOverlapParams {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "OverlapParams");
        filter_fields!(fields);
        fields.add_field_method_get("MaxParts", |_, this| Ok(this.max_parts));
        fields.add_field_method_set("MaxParts", |_, this, count: u32| {
            this.max_parts = count;
            Ok(())
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        filter_methods!(methods);
    }
}

/// What a raycast hit: the part, where, the surface normal there, how far along
/// the ray, and the part's material.
#[allow(dead_code)]
pub(crate) struct LuaRaycastResult {
    instance: Ref,
    position: Vector3Data,
    normal: Vector3Data,
    distance: f32,
    material: String,
    ctx: Ctx,
}

impl LuaRaycastResult {
    #[allow(dead_code)]
    pub(crate) fn new(
        instance: Ref,
        position: Vector3Data,
        normal: Vector3Data,
        distance: f32,
        material: String,
        ctx: Ctx,
    ) -> Self {
        LuaRaycastResult {
            instance,
            position,
            normal,
            distance,
            material,
            ctx,
        }
    }
}

impl UserData for LuaRaycastResult {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "RaycastResult");
        fields.add_field_method_get("Instance", |_, this| {
            Ok(LuaInstance::new(this.instance, this.ctx.clone()))
        });
        fields.add_field_method_get("Position", |_, this| Ok(LuaVector3(this.position)));
        fields.add_field_method_get("Normal", |_, this| Ok(LuaVector3(this.normal)));
        fields.add_field_method_get("Distance", |_, this| Ok(this.distance));
        fields.add_field_method_get("Material", |lua, this| {
            let value = this
                .ctx
                .database()
                .enum_items("Material")
                .and_then(|items| items.iter().find(|(name, _)| *name == this.material))
                .map(|(_, value)| *value)
                .ok_or_else(|| {
                    mlua::Error::runtime(format!(
                        "{} is not a valid member of Enum.Material",
                        this.material
                    ))
                })?;
            LuaEnumItem::new(
                "Material".to_string(),
                this.material.clone(),
                value,
                this.ctx.clone(),
            )
            .interned(lua)
        });
    }
}

pub(crate) fn register(lua: &Lua) -> Result<()> {
    let globals = lua.globals();

    let raycast = lua.create_table()?;
    raycast.set(
        "new",
        lua.create_function(|_, ()| Ok(LuaRaycastParams::new()))?,
    )?;
    globals.set("RaycastParams", raycast)?;

    let overlap = lua.create_table()?;
    overlap.set(
        "new",
        lua.create_function(|_, ()| Ok(LuaOverlapParams::new()))?,
    )?;
    globals.set("OverlapParams", overlap)?;
    Ok(())
}
