//! The tabbed Luau script editor: double-clicking a `Script`, `LocalScript`
//! or `ModuleScript` in the Explorer opens its `Source` here, highlighted,
//! in a tab of the dock's own Script Editor panel.
//!
//! The DOM stays the only copy of a script that counts. An editor's text is
//! written back to its instance's `Source` on a debounce (see
//! `shell::scripts`), and a tab whose `Source` moved underneath it — an undo,
//! a Command Bar script — is re-seeded from the DOM on the next render.
//! Nothing here holds a script for longer than that, which is what makes
//! closing and reopening a tab show what the DOM actually has.

pub(crate) mod find;
pub(crate) mod goto;
pub(crate) mod highlight;
pub(crate) mod luau;
pub(crate) mod outline;
pub(crate) mod source;
pub(crate) mod tabs;

use std::collections::HashMap;

use gpui_kit::component::input::{EditorState, InputState};
use gpui_kit::{Entity, Subscription};
use rbx_dom::Ref;

use tabs::Tabs;

/// One open script: its editor widget, and what the debounced DOM write needs
/// to know about it.
pub(crate) struct OpenScript {
    pub(crate) state: Entity<EditorState>,
    /// The text the DOM and this editor last agreed on. A `Source` that stops
    /// matching it changed from outside the editor, which is the whole signal
    /// [`crate::shell::Shell::resync_scripts`] re-seeds on.
    pub(crate) synced: String,
    /// Whether this editor holds typing that has yet to reach the DOM. While
    /// set, nothing re-seeds the editor — the pending text would be the thing
    /// overwritten.
    pub(crate) pending: bool,
    /// Bumped on every keystroke. A debounced write only lands if the
    /// generation it was scheduled for is still the latest, which is what
    /// collapses a burst of typing into one DOM write and one undo step.
    pub(crate) generation: u64,
    /// Kept only to stay subscribed to the editor's own change events.
    pub(crate) _subscription: Subscription,
}

/// Every open tab and the editor behind it. One field on `Shell`.
#[derive(Default)]
pub(crate) struct ScriptEditor {
    pub(crate) tabs: Tabs,
    pub(crate) open: HashMap<Ref, OpenScript>,
    /// The Alt+F / Ctrl+Shift+F overlay, while it is up.
    pub(crate) finder: Option<Finder>,
}

/// Which list the finder overlay shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinderMode {
    /// Studio's Script Function Filter: the active script's functions.
    Functions,
    /// Find All / Replace All, over every open tab.
    FindAll,
}

/// The finder overlay's own state; its hits are recomputed from the query on
/// every render rather than cached, so they can never go stale under typing.
pub(crate) struct Finder {
    pub(crate) mode: FinderMode,
    pub(crate) query: Entity<InputState>,
    pub(crate) replacement: Entity<InputState>,
    /// Index into the current hits; clamped on read, since the list shrinks
    /// under it as the query is typed.
    pub(crate) selected: usize,
    pub(crate) _subscription: Subscription,
}
