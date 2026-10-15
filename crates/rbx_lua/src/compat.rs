//! Spellings Roblox still answers to though they left its docs long ago:
//! lowercase method names (`part:findFirstChild`, `signal:connect`,
//! `children`), and an `Instance` method read off `game` as a plain value
//! (`local Clone = game.Clone`), which old plugins use to call it on other
//! instances.

use mlua::{Function, Lua, Result, Table, Value};

const CACHE: &str = "rbxn.method_aliases";

/// Instance methods a script may read off `game` and call on any instance.
const INSTANCE_METHODS: &[&str] = &[
    "ClearAllChildren",
    "Clone",
    "Destroy",
    "FindFirstAncestor",
    "FindFirstAncestorOfClass",
    "FindFirstAncestorWhichIsA",
    "FindFirstChild",
    "FindFirstChildOfClass",
    "FindFirstChildWhichIsA",
    "GetAttribute",
    "GetAttributes",
    "GetChildren",
    "GetDescendants",
    "GetFullName",
    "GetPropertyChangedSignal",
    "IsA",
    "IsAncestorOf",
    "IsDescendantOf",
    "SetAttribute",
    "WaitForChild",
];

/// A function that calls `self[name](self, ...)`, made once per name.
fn forwarder(lua: &Lua, name: &str) -> Result<Function> {
    let cache = match lua.named_registry_value::<Option<Table>>(CACHE)? {
        Some(cache) => cache,
        None => {
            let cache = lua.create_table()?;
            lua.set_named_registry_value(CACHE, cache.clone())?;
            cache
        }
    };
    if let Some(found) = cache.get::<Option<Function>>(name)? {
        return Ok(found);
    }
    let made: Function = lua
        .load(
            r#"
            local name = ...
            local this
            this = function(self, ...)
                local method = self[name]
                -- Resolving back to this very forwarder: the object has no
                -- such method (`game:Clone()`), not a reason to recurse.
                if method == this then
                    error(name .. " is not a valid member of " .. tostring(self), 2)
                end
                return method(self, ...)
            end
            return this
            "#,
        )
        .set_name("=alias")
        .call(name)?;
    cache.set(name, made.clone())?;
    Ok(made)
}

/// The current name for a deprecated lowercase one: `children` is
/// `GetChildren`, anything else is capitalised. `None` when `key` does not
/// start lowercase.
fn current_name(key: &str) -> Option<String> {
    if key == "children" {
        return Some("GetChildren".to_string());
    }
    let mut chars = key.chars();
    let first = chars.next()?;
    first
        .is_ascii_lowercase()
        .then(|| first.to_ascii_uppercase().to_string() + chars.as_str())
}

/// For a lowercase `key`, a function forwarding to the current method;
/// `None` otherwise.
pub(crate) fn lowercase_method(lua: &Lua, key: &str) -> Result<Option<Value>> {
    match current_name(key) {
        Some(name) => Ok(Some(Value::Function(forwarder(lua, &name)?))),
        None => Ok(None),
    }
}

/// `game.Clone` and the like: a forwarding function for an `Instance`
/// method, so it works on whatever instance it is called with.
pub(crate) fn instance_method(lua: &Lua, key: &str) -> Result<Option<Value>> {
    if INSTANCE_METHODS.contains(&key) {
        return Ok(Some(Value::Function(forwarder(lua, key)?)));
    }
    lowercase_method(lua, key)
}

#[cfg(test)]
mod tests {
    use super::current_name;

    #[test]
    fn lowercase_names_map_to_the_current_ones() {
        assert_eq!(
            current_name("findFirstChild").as_deref(),
            Some("FindFirstChild")
        );
        assert_eq!(current_name("connect").as_deref(), Some("Connect"));
        assert_eq!(current_name("children").as_deref(), Some("GetChildren"));
        assert_eq!(current_name("Name"), None);
    }
}
