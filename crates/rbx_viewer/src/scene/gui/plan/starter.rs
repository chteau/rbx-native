//! `StarterGui.ShowDevelopmentGui`, which "determines whether the contents of
//! `StarterGui` is visible in Studio" (the property's own page).
//!
//! The viewer draws a place the way Studio's edit view does, it never copies
//! `StarterGui` into a `PlayerGui`: so the flag is ours to honour: with it
//! off, nothing under that service is drawn, neither the `ScreenGui` overlays
//! nor the `BillboardGui`/`SurfaceGui` canvases the docs' "contents" covers.
//! `rbxview --show-development-gui` is the way back to what a player sees.

use std::sync::atomic::{AtomicU32, Ordering};

use rbx_dom::{Instance, Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::flag;

const STARTER_GUI_CLASS: &str = "StarterGui";

/// No player's GUI shown: the edit view.
const NO_PLAYER: u32 = u32::MAX;

/// The `PlayerGui` drawn instead of `StarterGui` while a game runs (Play
/// (local) shows the viewed player's own screens, as their client draws
/// them), [`NO_PLAYER`] in the edit view.
static PLAYER_GUI: AtomicU32 = AtomicU32::new(NO_PLAYER);

/// Draws the screens of `player_gui` (a `PlayerGui`) and nothing of
/// `StarterGui`, as a player sees a running game; `None` goes back to the
/// edit view. The GUI must be re-planned after.
pub fn set_player_gui(player_gui: Option<Ref>) {
    PLAYER_GUI.store(
        player_gui.map_or(NO_PLAYER, |r| r.value()),
        Ordering::Relaxed,
    );
}

/// The `PlayerGui` [`set_player_gui`] last named.
pub fn player_gui() -> Option<Ref> {
    let value = PLAYER_GUI.load(Ordering::Relaxed);
    (value != NO_PLAYER).then(|| Ref::new(value))
}

/// Whether `instance` is a `StarterGui` that is hiding its own contents.
///
/// Absent, the property is `true`: Studio shows the tree unless it was turned
/// off, and only a saved place carries the value at all.
pub(in crate::scene::gui) fn hides_contents(
    database: &ReflectionDatabase,
    instance: &Instance,
) -> bool {
    database.is_subclass_of(instance.class(), STARTER_GUI_CLASS)
        && (player_gui().is_some() || !flag(instance.properties(), "ShowDevelopmentGui", true))
}

/// Whether a `ScreenGui` at `referent` is one Studio's edit view puts on
/// screen: only those under `StarterGui` are. One kept anywhere else in a
/// place, `ServerScriptService`, `ReplicatedStorage`, `Workspace`, is a
/// template a script clones at run time, never drawn. The test is "no
/// service but `StarterGui` above it" rather than "`StarterGui` above it" so
/// a model file, which has no services at all, still shows its screens.
pub fn shows_screen(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> bool {
    if let Some(root) = player_gui() {
        let mut up = dom.parent(referent);
        while let Some(ancestor) = up {
            if ancestor == root {
                return true;
            }
            up = dom.parent(ancestor);
        }
        return false;
    }
    let mut up = dom.parent(referent);
    while let Some(instance) = up.and_then(|ancestor| dom.get(ancestor)) {
        if database.is_service(instance.class()) {
            return database.is_subclass_of(instance.class(), STARTER_GUI_CLASS);
        }
        up = dom.parent(instance.referent());
    }
    true
}
