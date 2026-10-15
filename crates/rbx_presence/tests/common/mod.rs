//! The plugin's Luau modules in plain Luau (mlua): a stub `script` whose
//! `Parent.<Name>` evaluates the sibling module, and `require` as the
//! identity, which is all the pure modules need from Studio.

use std::path::PathBuf;

use mlua::{Lua, Table, Value};

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn src_dir() -> PathBuf {
    root().join("plugins/team-presence/src")
}

/// Evaluates `<name>.luau` once and caches its returned table.
pub fn load_module(lua: &Lua, name: &str) -> mlua::Result<Table> {
    let cache: Table = lua.globals().get("__modules")?;
    if let Ok(found) = cache.get::<Table>(name) {
        return Ok(found);
    }
    let path = src_dir().join(format!("{name}.luau"));
    let src = std::fs::read_to_string(&path)
        .map_err(|e| mlua::Error::runtime(format!("{}: {e}", path.display())))?;
    let module: Table = lua.load(src).set_name(format!("={name}")).eval()?;
    cache.set(name, module.clone())?;
    Ok(module)
}

pub fn new_lua() -> Lua {
    let lua = Lua::new();
    let globals = lua.globals();
    globals
        .set("__modules", lua.create_table().unwrap())
        .unwrap();

    let parent = lua.create_table().unwrap();
    let meta = lua.create_table().unwrap();
    meta.set(
        "__index",
        lua.create_function(|lua, (_this, name): (Table, String)| load_module(lua, &name))
            .unwrap(),
    )
    .unwrap();
    parent.set_metatable(Some(meta)).unwrap();

    let script = lua.create_table().unwrap();
    script.set("Parent", parent).unwrap();
    globals.set("script", script).unwrap();
    globals
        .set(
            "require",
            lua.create_function(|_, value: Value| Ok(value)).unwrap(),
        )
        .unwrap();
    drop(globals);
    lua
}
