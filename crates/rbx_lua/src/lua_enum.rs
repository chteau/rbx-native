//! `Enum.<Name>.<Item>`, resolved lazily from the reflection database.

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};

use crate::ctx::Ctx;

/// The `Enum` global.
///
/// Userdata rather than a nested table tree: the dump holds thousands of enum
/// items across hundreds of enums, and a script touches a handful of them, so
/// each level resolves on access. Having no `__newindex` also makes the whole
/// tree read-only by construction.
pub(crate) struct LuaEnums {
    ctx: Ctx,
}

impl LuaEnums {
    pub(crate) fn new(ctx: Ctx) -> Self {
        LuaEnums { ctx }
    }
}

impl UserData for LuaEnums {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Enums");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |_, this, name: String| {
            if this.ctx.database().enum_items(&name).is_none() {
                return Err(mlua::Error::runtime(format!("{name} is not a valid Enum")));
            }
            Ok(LuaEnum {
                name,
                ctx: this.ctx.clone(),
            })
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("Enum"));
    }
}

pub(crate) struct LuaEnum {
    name: String,
    ctx: Ctx,
}

impl UserData for LuaEnum {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "Enum");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // `item.EnumType == Enum.Material`: one enum however it was reached.
        methods.add_meta_method(
            MetaMethod::Eq,
            |_, this, other: mlua::UserDataRef<LuaEnum>| Ok(this.name == other.name),
        );
        methods.add_method("FromName", |lua, this, name: String| {
            this.ctx
                .database()
                .enum_items(&this.name)
                .and_then(|items| items.iter().find(|(item, _)| *item == name))
                .map(|(item, value)| {
                    LuaEnumItem::new(
                        this.name.clone(),
                        item.to_string(),
                        *value,
                        this.ctx.clone(),
                    )
                    .interned(lua)
                })
                .unwrap_or(Ok(Value::Nil))
        });
        methods.add_method("FromValue", |lua, this, wanted: f64| {
            this.ctx
                .database()
                .enum_items(&this.name)
                .and_then(|items| items.iter().find(|(_, value)| f64::from(*value) == wanted))
                .map(|(item, value)| {
                    LuaEnumItem::new(
                        this.name.clone(),
                        item.to_string(),
                        *value,
                        this.ctx.clone(),
                    )
                    .interned(lua)
                })
                .unwrap_or(Ok(Value::Nil))
        });
        methods.add_method("GetEnumItems", |lua, this, ()| {
            let items = this
                .ctx
                .database()
                .enum_items(&this.name)
                .unwrap_or_default()
                .to_vec();
            let table = lua.create_table()?;
            for (index, (item, value)) in items.into_iter().enumerate() {
                table.set(
                    index + 1,
                    LuaEnumItem::new(this.name.clone(), item, value, this.ctx.clone())
                        .interned(lua)?,
                )?;
            }
            Ok(table)
        });
        methods.add_meta_method(MetaMethod::Index, |lua, this, item: String| {
            let value = this
                .ctx
                .database()
                .enum_items(&this.name)
                .and_then(|items| items.iter().find(|(name, _)| *name == item))
                .map(|(_, value)| *value)
                .ok_or_else(|| {
                    mlua::Error::runtime(format!(
                        "{item} is not a valid member of Enum.{}",
                        this.name
                    ))
                })?;
            LuaEnumItem::new(this.name.clone(), item, value, this.ctx.clone()).interned(lua)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Enum.{}", this.name))
        });
    }
}

pub(crate) struct LuaEnumItem {
    enum_name: String,
    name: String,
    value: u32,
    ctx: Ctx,
}

impl LuaEnumItem {
    pub(crate) fn new(enum_name: String, name: String, value: u32, ctx: Ctx) -> Self {
        LuaEnumItem {
            enum_name,
            name,
            value,
            ctx,
        }
    }

    /// The one userdata for this item in `lua`: Roblox enum items are
    /// unique objects, so scripts use them as table keys
    /// (`normals[Enum.NormalId.Top]`) and compare them with `rawequal`.
    pub(crate) fn interned(self, lua: &Lua) -> Result<Value> {
        const REGISTRY: &str = "rbxn.enum_items";
        let cache = match lua.named_registry_value::<Option<Table>>(REGISTRY)? {
            Some(cache) => cache,
            None => {
                let cache = lua.create_table()?;
                lua.set_named_registry_value(REGISTRY, cache.clone())?;
                cache
            }
        };
        let key = format!("{}.{}", self.enum_name, self.name);
        if let Some(found) = cache.get::<Option<Value>>(key.as_str())? {
            return Ok(found);
        }
        let value = Value::UserData(lua.create_userdata(self)?);
        cache.set(key, value.clone())?;
        Ok(value)
    }

    pub(crate) fn enum_name(&self) -> &str {
        &self.enum_name
    }

    pub(crate) fn value(&self) -> u32 {
        self.value
    }
}

impl UserData for LuaEnumItem {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "EnumItem");
        fields.add_field_method_get("Name", |_, this| Ok(this.name.clone()));
        fields.add_field_method_get("Value", |_, this| Ok(this.value));
        fields.add_field_method_get("EnumType", |_, this| {
            Ok(LuaEnum {
                name: this.enum_name.clone(),
                ctx: this.ctx.clone(),
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("IsA", |_, this, enum_name: String| {
            Ok(this.enum_name == enum_name)
        });
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: mlua::UserDataRef<Self>| {
            Ok(this.enum_name == other.enum_name && this.value == other.value)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Enum.{}.{}", this.enum_name, this.name))
        });
    }
}
