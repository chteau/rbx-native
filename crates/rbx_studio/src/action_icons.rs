//! This project's own icon for a ribbon *action* — Select, Play, Lock, and
//! the rest of `shell::ribbon`'s tile set — that isn't a Roblox class and so
//! has no entry in `class_icons`' `ClassName`-keyed kit. Same flat,
//! two-tone SVG style (`assets/icons/README.md`'s design system), same
//! `dark`/`light` variants picked by the same [`IconPack`] setting, same
//! rasterizer (`class_icons::rasterize`) — a sibling kit keyed by action
//! name instead of `ClassName`, not a different mechanism.
//!
//! `ribbon`'s own doc comment explains why these exist at all instead of
//! Lucide glyphs: this editor's own visual identity, not the toolkit's
//! generic one, for the handful of actions a user looks at constantly.

use std::sync::Arc;

use gpui_kit::RenderImage;

use crate::class_icons::{rasterize, IconPack};

#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../assets/icons/actions/dark"]
struct DarkActionIcons;

#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../assets/icons/actions/light"]
struct LightActionIcons;

/// The rasterized icon for `name` (e.g. `"select"`, `"play"` — a bare
/// filename stem under `assets/icons/actions/dark`, listed in
/// `shell::ribbon`'s own tile calls) from the requested `pack`, or `None`
/// if `name` has no file — a build-time invariant this module's own tests
/// check, not something a caller needs to guard against at runtime.
pub(crate) fn action_icon(name: &str, pack: IconPack) -> Option<Arc<RenderImage>> {
    let file = match pack {
        IconPack::Dark => DarkActionIcons::get(&format!("{name}.svg")),
        IconPack::Light => LightActionIcons::get(&format!("{name}.svg")),
    }?;
    rasterize(&file.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every action name `shell::ribbon` actually calls `action_icon` with
    /// — kept here rather than shared with that module, since this test's
    /// only job is catching a typo'd filename or invalid SVG at test time
    /// rather than as a blank ribbon tile at runtime; a name added to one
    /// list without the other fails loudly either way (a blank tile in a
    /// screenshot, or this test), never silently.
    const NAMES: &[&str] = &[
        "select",
        "move",
        "scale",
        "rotate",
        "copy",
        "paste",
        "cut",
        "duplicate",
        "play",
        "run",
        "resume",
        "stop",
        "team",
        "exit",
        "lock",
        "anchor",
        "material",
        "color",
        "toolbox",
        "import",
        "game-settings",
        "device",
        "show-ui",
    ];

    #[test]
    fn every_ribbon_action_rasterizes_in_both_packs() {
        for name in NAMES {
            assert!(
                action_icon(name, IconPack::Dark).is_some(),
                "no dark-pack action icon for {name:?}"
            );
            assert!(
                action_icon(name, IconPack::Light).is_some(),
                "no light-pack action icon for {name:?}"
            );
        }
    }
}
