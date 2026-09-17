//! `StarterGui.ShowDevelopmentGui`, which "determines whether the contents of
//! `StarterGui` is visible in Studio" (the property's own page).
//!
//! The viewer draws a place the way Studio's edit view does — it never copies
//! `StarterGui` into a `PlayerGui` — so the flag is ours to honour: with it
//! off, nothing under that service is drawn, neither the `ScreenGui` overlays
//! nor the `BillboardGui`/`SurfaceGui` canvases the docs' "contents" covers.
//! `rbxview --show-development-gui` is the way back to what a player sees.

use rbx_dom::Instance;
use rbx_reflection::ReflectionDatabase;

use super::flag;

const STARTER_GUI_CLASS: &str = "StarterGui";

/// Whether `instance` is a `StarterGui` that is hiding its own contents.
///
/// Absent, the property is `true`: Studio shows the tree unless it was turned
/// off, and only a saved place carries the value at all.
pub(in crate::scene::gui) fn hides_contents(
    database: &ReflectionDatabase,
    instance: &Instance,
) -> bool {
    database.is_subclass_of(instance.class(), STARTER_GUI_CLASS)
        && !flag(instance.properties(), "ShowDevelopmentGui", true)
}
