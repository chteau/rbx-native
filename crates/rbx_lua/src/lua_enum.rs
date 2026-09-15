//! `Enum.<Name>.<Item>`, resolved lazily from the reflection database.

use mlua::{MetaMethod, UserData, UserDataFields, UserDataMethods};

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
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
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
                    LuaEnumItem::new(this.name.clone(), item, value, this.ctx.clone()),
                )?;
            }
            Ok(table)
        });
        methods.add_meta_method(MetaMethod::Index, |_, this, item: String| {
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
            Ok(LuaEnumItem::new(
                this.name.clone(),
                item,
                value,
                this.ctx.clone(),
            ))
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

    pub(crate) fn enum_name(&self) -> &str {
        &self.enum_name
    }

    pub(crate) fn value(&self) -> u32 {
        self.value
    }
}

impl UserData for LuaEnumItem {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
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
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: mlua::UserDataRef<Self>| {
            Ok(this.enum_name == other.enum_name && this.value == other.value)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Enum.{}.{}", this.enum_name, this.name))
        });
    }
}
