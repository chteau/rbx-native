//! The command palette and Quick Open, one overlay as in Studio: Ctrl+P
//! searches the place's instances, and a leading `>` — or Ctrl+Alt+P,
//! which opens with it typed — searches commands instead ("Quickest Open
//! in the West!", DevForum, 2020). Commands are every one the editor
//! already has (see [`commands`]), each with its shortcut, so a command can
//! be found by name instead of remembered by key.
//!
//! Keyboard and semantics follow the APG combobox-with-listbox pattern:
//! focus stays in the field, Up/Down/Home/End move the highlighted option
//! (wrapping, as the menus do), Enter runs it, Escape or a click outside
//! closes, and focus goes back to wherever it was. The field's own
//! bindings are taken in the capture phase, as `shell::script_finder`
//! does, because a focused input runs its key bindings before any key
//! listener sees the key. See `view` for how the highlighted option reaches
//! a screen reader.
//!
//! Both shortcuts are keymap bindings for the View menu's items
//! (`menu_bar::install_key_bindings`), so each key and its menu item are
//! one action.

mod commands;
mod focus;
mod instances;
mod view;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use crate::script_editor::source;

use super::roving::Move;
use super::Shell;
use commands::{Command, Run};

/// `RBX_STUDIO_A11Y_DUMP=<path>`: while a screen reader (or any AT-SPI
/// client) has the window's accessibility tree switched on, each frame
/// writes GPUI's own debug dump of it to `path` — the only way to see from
/// outside what the palette reports as focused, since no test window ever
/// activates accessibility.
const A11Y_DUMP_VARIABLE: &str = "RBX_STUDIO_A11Y_DUMP";

#[derive(Default)]
pub(super) struct Palette {
    open: Option<Open>,
    /// Labels of the rows run this session, most recent first.
    recent: Vec<SharedString>,
    /// Asked for by a key or the menu, opened by the next render: `true`
    /// for commands, `false` for Quick Open's instances.
    requested: Option<bool>,
}

struct Open {
    query: Entity<InputState>,
    commands: Vec<Command>,
    instances: Vec<Command>,
    /// Whether the query asks for commands (see `commands::wants_actions`).
    actions: bool,
    /// What the query keeps, best first, as indices into whichever list
    /// `actions` picks; recomputed only when the query changes.
    rows: Vec<usize>,
    /// Index into `rows`.
    highlighted: usize,
    /// What held focus before the palette took it.
    restore: Option<FocusHandle>,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

impl Open {
    fn list(&self) -> &[Command] {
        match self.actions {
            true => &self.commands,
            false => &self.instances,
        }
    }
}

impl Shell {
    /// The menu actions' handlers are global and get no `Window` — and
    /// `App::active_window` is `None` under a window manager that never
    /// activates anything — so the opening waits for `Render`, which has one.
    pub(crate) fn request_palette(&mut self, actions: bool, cx: &mut Context<Self>) {
        self.palette.requested = Some(actions);
        cx.notify();
    }

    /// Called from `Render for Shell`, before the tree is built, so the
    /// field it focuses is in the very frame that shows it. Asked again
    /// while open, it switches mode in place.
    pub(super) fn open_requested_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok(path) = std::env::var(A11Y_DUMP_VARIABLE) {
            if let Some(tree) = window.debug_a11y_tree_json() {
                let _ = std::fs::write(path, tree);
            }
        }
        let Some(actions) = self.palette.requested.take() else {
            return;
        };
        let seed = if actions { ">" } else { "" };
        match &self.palette.open {
            Some(open) => {
                let query = open.query.clone();
                query.update(cx, |state, cx| {
                    state.set_value(seed, window, cx);
                    state.set_selected_range(seed.len()..seed.len(), cx);
                });
                self.refresh_palette_rows(cx);
            }
            None => self.open_palette(seed, window, cx),
        }
    }

    fn open_palette(&mut self, seed: &str, window: &mut Window, cx: &mut Context<Self>) {
        let panels = {
            let hidden = self.document_hides();
            super::Panel::ALL
                .into_iter()
                .filter(move |p| !hidden.contains(p))
        };
        let menus = crate::menu_bar::menus(self.script_templates.extras());
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search the place, or type > for commands")
                .default_value(seed.to_owned())
        });
        let subscription = cx.subscribe(&query, |shell, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                shell.refresh_palette_rows(cx);
            }
        });
        let restore = window.focused(cx);
        query.update(cx, |state, cx| {
            state.set_selected_range(seed.len()..seed.len(), cx);
            state.focus(window, cx);
        });
        self.palette.open = Some(Open {
            query,
            commands: commands::registry(&menus, panels),
            instances: instances::instances(&self.dom),
            actions: true,
            rows: Vec::new(),
            highlighted: 0,
            restore,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        });
        self.refresh_palette_rows(cx);
    }

    fn refresh_palette_rows(&mut self, cx: &mut Context<Self>) {
        let palette = &mut self.palette;
        let Some(open) = palette.open.as_mut() else {
            return;
        };
        let query = open.query.read(cx).value().to_string();
        open.actions = commands::wants_actions(&query);
        let mut rows = commands::filter(open.list(), &query, &palette.recent);
        if !open.actions {
            rows.truncate(instances::SHOWN);
        }
        open.rows = rows;
        open.highlighted = 0;
        open.scroll.scroll_to_item(0);
        cx.notify();
    }

    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Open> {
        let open = self.palette.open.take()?;
        if let Some(handle) = &open.restore {
            window.focus(handle, cx);
        }
        cx.notify();
        Some(open)
    }

    fn move_palette_highlight(&mut self, movement: Move, cx: &mut Context<Self>) {
        if let Some(open) = self.palette.open.as_mut() {
            let count = open.rows.len();
            if count > 0 {
                open.highlighted = movement.apply(open.highlighted.min(count - 1), count);
                open.scroll.scroll_to_item(open.highlighted);
            }
            cx.notify();
        }
    }

    /// Closes first, so the row runs against the focus it would have had
    /// if its shortcut had been typed instead.
    fn run_palette_row(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self
            .palette
            .open
            .as_ref()
            .and_then(|open| open.rows.get(row).copied())
        else {
            return;
        };
        let Some(mut open) = self.close_palette(window, cx) else {
            return;
        };
        let command = match open.actions {
            true => open.commands.swap_remove(index),
            false => open.instances.swap_remove(index),
        };
        self.palette.recent.retain(|label| *label != command.label);
        self.palette.recent.insert(0, command.label);
        match command.run {
            // Deferred by GPUI, which matters: every menu handler updates
            // `Shell`, which is mid-update right here.
            Run::Action(action) => window.dispatch_action(action, cx),
            Run::Tool(tool) => self.transform_action(crate::transform::Action::Use(tool), cx),
            Run::Ribbon(command) => command.run(self, cx),
            Run::Focus(panel) => self.focus_dock(panel, window, cx),
            Run::Document(document) => self.focus_document(document, window, cx),
            Run::Instance(reference) => {
                if source::is_script(&self.dom, &self.database, reference) {
                    self.open_script(reference, window, cx);
                } else {
                    self.select(reference, cx);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "palette/tests.rs"]
mod tests;
