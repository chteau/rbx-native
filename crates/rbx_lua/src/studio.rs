//! Studio's own API, for plugins and the Command Bar alike: the settings
//! objects (`settings()`), the theme, and the Studio-only members of
//! services and instances.

mod devices;
mod diagnostics;
mod palette;
mod services;
mod settings;
mod theme;

use mlua::{Lua, Result};

use crate::ctx::Ctx;

pub(crate) use diagnostics::member as diagnostics;
pub(crate) use services::{game_member, member};
pub(crate) use theme::set_theme;

/// The Studio globals a plugin or Command Bar VM gets: `settings()`,
/// `UserSettings()`.
pub(crate) fn install(lua: &Lua, ctx: &Ctx) -> Result<()> {
    settings::install(lua, ctx)
}
