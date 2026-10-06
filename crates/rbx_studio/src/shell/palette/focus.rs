//! Where "View: Focus <dock>" and "View: Focus <document>" put the caret:
//! keyboard-driven panel management, the other half of the reference
//! guidance's "recognition rather than recall" item.

use gpui_kit::component::input::InputState;
use gpui_kit::*;

use crate::shell::chrome::Document;
use crate::shell::{Panel, Shell};

/// A dock's keyboard entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Entry {
    Tree,
    PropertiesFilter,
    OutputSearch,
    QualitySelect,
    ArgonAddress,
    WallySearch,
    WatchExpression,
}

/// The one control in `panel` that every state of it shows, or `None`
/// where there is nothing to focus: Script Analysis and Call Stack are
/// lists of clickable rows with no focusable element at all, and the
/// Terrain Editor's fields change with its tool, none of them shared by
/// every tool (its tool grid itself is not focusable either).
pub(super) fn entry(panel: Panel) -> Option<Entry> {
    match panel {
        Panel::Explorer => Some(Entry::Tree),
        Panel::Properties => Some(Entry::PropertiesFilter),
        Panel::Output => Some(Entry::OutputSearch),
        Panel::Viewport => Some(Entry::QualitySelect),
        Panel::Argon => Some(Entry::ArgonAddress),
        Panel::Wally => Some(Entry::WallySearch),
        Panel::Watch => Some(Entry::WatchExpression),
        Panel::ScriptAnalysis | Panel::CallStack | Panel::TerrainEditor => None,
    }
}

fn focus_input(input: &Entity<InputState>, window: &mut Window, cx: &mut App) {
    input.update(cx, |state, cx| state.focus(window, cx));
}

impl Shell {
    /// Opens `panel` (or brings its tab forward) and moves focus into it.
    /// The Terrain Editor opens the way its ribbon tile does, on its last
    /// tool, since a terrain panel with no tool armed edits nothing.
    pub(super) fn focus_dock(&mut self, panel: Panel, window: &mut Window, cx: &mut Context<Self>) {
        match panel {
            Panel::TerrainEditor => self.toggle_terrain_editor(true, cx),
            _ => self.set_panel_open(panel, true, cx),
        }
        match entry(panel) {
            Some(Entry::Tree) => window.focus(&self.tree_focus_handle, cx),
            Some(Entry::PropertiesFilter) => focus_input(&self.filter, window, cx),
            Some(Entry::OutputSearch) => focus_input(&self.output_search, window, cx),
            Some(Entry::QualitySelect) => {
                let handle = self.quality.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }
            Some(Entry::ArgonAddress) => {
                let host = self.argon_ui.host().clone();
                focus_input(&host, window, cx);
            }
            Some(Entry::WallySearch) => focus_input(&self.wally_query, window, cx),
            Some(Entry::WatchExpression) => self.focus_watch_input(window, cx),
            None => {}
        }
    }

    /// Brings `document` to the front with the caret in it: the 3D view,
    /// the active script tab, or the UI canvas.
    pub(super) fn focus_document(
        &mut self,
        document: Document,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_document(document, cx);
        match document {
            Document::Viewport => {
                let handle = self.viewport.read(cx).focus_handle();
                window.focus(&handle, cx);
            }
            Document::Scripts => {
                if let Some(active) = self.scripts.tabs.active() {
                    self.focus_script(active, window, cx);
                }
            }
            Document::UiEditor => self.focus_ui_canvas(window, cx),
        }
    }
}
