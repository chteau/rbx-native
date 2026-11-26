//! `CatalogSearchParams`, what `AvatarEditorService:SearchCatalogAsync`
//! takes: a bag of filter fields with the creator docs' defaults; a field
//! it doesn't have is an error, as a live server words it.

use std::collections::BTreeMap;

use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value};

use super::tween_info::enum_item;

#[derive(Clone, Debug)]
pub(crate) struct LuaCatalogParams {
    fields: BTreeMap<&'static str, Value>,
}

const FIELDS: &[&str] = &[
    "SearchKeyword",
    "MinPrice",
    "MaxPrice",
    "SortType",
    "SortAggregation",
    "CategoryFilter",
    "SalesTypeFilter",
    "BundleTypes",
    "AssetTypes",
    "IncludeOffSale",
    "CreatorName",
    "CreatorType",
    "CreatorId",
    "Limit",
];

fn defaults(lua: &Lua) -> Result<BTreeMap<&'static str, Value>> {
    let mut f = BTreeMap::new();
    f.insert("SearchKeyword", Value::String(lua.create_string("")?));
    f.insert("MinPrice", Value::Integer(0));
    f.insert("MaxPrice", Value::Integer(2_147_483_647));
    f.insert("SortType", enum_item(lua, "CatalogSortType", "Relevance")?);
    f.insert(
        "SortAggregation",
        enum_item(lua, "CatalogSortAggregation", "AllTime")?,
    );
    f.insert(
        "CategoryFilter",
        enum_item(lua, "CatalogCategoryFilter", "None")?,
    );
    f.insert("SalesTypeFilter", enum_item(lua, "SalesTypeFilter", "All")?);
    f.insert("BundleTypes", Value::Table(lua.create_table()?));
    f.insert("AssetTypes", Value::Table(lua.create_table()?));
    f.insert("IncludeOffSale", Value::Boolean(false));
    f.insert("CreatorName", Value::String(lua.create_string("")?));
    f.insert("CreatorType", enum_item(lua, "CreatorTypeFilter", "All")?);
    f.insert("CreatorId", Value::Integer(0));
    f.insert("Limit", Value::Integer(30));
    Ok(f)
}

fn field(key: &str) -> Result<&'static str> {
    FIELDS.iter().copied().find(|f| *f == key).ok_or_else(|| {
        mlua::Error::runtime(format!(
            "{key} is not a valid member of CatalogSearchParams"
        ))
    })
}

impl UserData for LuaCatalogParams {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field(MetaMethod::Type, "CatalogSearchParams");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |_, this, key: String| {
            Ok(this.fields.get(field(&key)?).cloned().unwrap_or(Value::Nil))
        });
        methods.add_meta_method_mut(
            MetaMethod::NewIndex,
            |_, this, (key, value): (String, Value)| {
                this.fields.insert(field(&key)?, value);
                Ok(())
            },
        );
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("CatalogSearchParams"));
    }
}

pub(super) fn register(lua: &Lua) -> Result<()> {
    let table: Table = lua.create_table()?;
    table.set(
        "new",
        lua.create_function(|lua, ()| {
            Ok(LuaCatalogParams {
                fields: defaults(lua)?,
            })
        })?,
    )?;
    lua.globals().set("CatalogSearchParams", table)
}
