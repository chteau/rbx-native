//! `settings().Studio.Theme`: the `StudioTheme` objects, `GetColor`,
//! `GetAvailableThemes` and `ThemeChanged`.

use mlua::{AnyUserData, Lua, Result, Value, Variadic};

use super::palette;
use super::settings::{self, SettingsObject};
use crate::ctx::Ctx;
use crate::lua_enum::LuaEnumItem;

/// Studio's themes, in the order `GetAvailableThemes` lists them.
const THEMES: [&str; 2] = ["Light", "Dark"];

/// Makes the theme objects under `settings().Studio` and points `Theme` at
/// the host's (see [`set_theme`]; Dark until told).
pub(super) fn attach(lua: &Lua, ctx: &Ctx, root: &AnyUserData) -> Result<()> {
    let Some(studio) = settings::child(root, "Studio") else {
        return Ok(());
    };
    let mut themes = Vec::new();
    for name in THEMES {
        themes.push(settings::new_object(lua, ctx, "StudioTheme", name, None)?);
    }
    lua.set_named_registry_value(
        "rbxn.studio_themes",
        lua.create_sequence_from(themes.clone())?,
    )?;
    let current = lua
        .named_registry_value::<Option<String>>("rbxn.studio_theme")?
        .unwrap_or_else(|| "Dark".to_owned());
    let theme = themes
        .iter()
        .find(|t| {
            t.borrow::<SettingsObject>()
                .is_ok_and(|t| settings::name_of(&t) == current)
        })
        .cloned()
        .unwrap_or_else(|| themes[1].clone());
    studio
        .borrow::<SettingsObject>()?
        .set_initial("Theme", Value::UserData(theme));
    Ok(())
}

/// The editor's theme ("Light" or "Dark"): `settings().Studio.Theme`
/// follows it, firing `ThemeChanged` when it moves.
pub(crate) fn set_theme(lua: &Lua, name: &str) -> Result<()> {
    let name = if name == "Light" { "Light" } else { "Dark" };
    let previous = lua.named_registry_value::<Option<String>>("rbxn.studio_theme")?;
    if previous.as_deref() == Some(name) {
        return Ok(());
    }
    lua.set_named_registry_value("rbxn.studio_theme", name)?;
    let Some(root) = lua.named_registry_value::<Option<AnyUserData>>("rbxn.settings")? else {
        return Ok(());
    };
    let (Some(studio), Some(themes)) = (
        settings::child(&root, "Studio"),
        lua.named_registry_value::<Option<mlua::Table>>("rbxn.studio_themes")?,
    ) else {
        return Ok(());
    };
    for theme in themes.sequence_values::<AnyUserData>() {
        let theme = theme?;
        let matches = theme
            .borrow::<SettingsObject>()
            .is_ok_and(|t| settings::name_of(&t) == name);
        if matches {
            settings::set_value(lua, &studio, "Theme", Value::UserData(theme))?;
            let signal = studio.borrow::<SettingsObject>()?.signal("ThemeChanged");
            crate::engine::signal::fire(lua, &signal.0, &[])?;
        }
    }
    Ok(())
}

/// `Studio.GetAvailableThemes`, `Studio.ThemeChanged` and
/// `StudioTheme.GetColor`; `None` for anything else.
pub(super) fn member(lua: &Lua, this: &SettingsObject, key: &str) -> Result<Option<Value>> {
    let value = match (this.class, key) {
        ("Studio", "ThemeChanged") => {
            Value::UserData(lua.create_userdata(this.signal("ThemeChanged"))?)
        }
        // A fresh list per call, as in Studio.
        ("Studio", "GetAvailableThemes") => {
            Value::Function(lua.create_function(|lua, _: Variadic<Value>| {
                let themes =
                    lua.named_registry_value::<Option<mlua::Table>>("rbxn.studio_themes")?;
                let list: Vec<Value> = match themes {
                    Some(t) => t.sequence_values::<Value>().collect::<Result<_>>()?,
                    None => Vec::new(),
                };
                lua.create_sequence_from(list)
            })?)
        }
        ("StudioTheme", "GetColor") => Value::Function(lua.create_function(
            |lua, (this, color, modifier): (AnyUserData, Value, Option<Value>)| {
                let theme = settings::name_of(&*this.borrow::<SettingsObject>()?).to_owned();
                let [r, g, b] = color_of(&theme, &color, modifier.as_ref())?;
                lua.create_userdata(crate::datatypes::color3::LuaColor3::new(r, g, b))
            },
        )?),
        _ => return Ok(None),
    };
    Ok(Some(value))
}

/// The theme's colour for an `Enum.StudioStyleGuideColor` (or its name)
/// under an optional `Enum.StudioStyleGuideModifier`; an unknown name is an
/// error, as in Studio.
fn color_of(theme: &str, color: &Value, modifier: Option<&Value>) -> Result<[f32; 3]> {
    let name = |value: &Value| -> Result<String> {
        if let Some(item) = value
            .as_userdata()
            .and_then(|u| u.borrow::<LuaEnumItem>().ok())
        {
            return Ok(item.name().to_owned());
        }
        Ok(match value {
            Value::String(s) => s.to_string_lossy().to_string(),
            other => other.to_string()?,
        })
    };
    let modifier = match modifier {
        Some(Value::Nil) | None => String::new(),
        Some(value) => name(value)?,
    };
    let color = name(color)?;
    let rgb = palette::color(theme, &color, &modifier)
        .ok_or_else(|| mlua::Error::runtime("Unable to cast string to StudioStyleGuideColor"))?;
    Ok(rgb.map(|channel| f32::from(channel) / 255.0))
}
