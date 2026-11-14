//! Every plugin `DockWidgetPluginGui` is a dock of its own
//! (`layout::Panel::Plugin`): its own tab, dragged to any edge, tabbed with
//! anything, floated, and remembered by plugin name and widget id in the
//! saved layout like Explorer is.
//!
//! A widget's place outlives the widget being open. Closing it (its tab's
//! ×, or the plugin setting `Enabled` false) leaves its panel where it was
//! and only hides it, so it comes back there when the plugin, its toolbar
//! button or the palette opens it again. A widget only gets a place once,
//! the first time it opens: from its `InitialDockState`, after which the
//! user's placement wins, as in Studio.

use gpui_kit::assets::IconName;
use gpui_kit::*;
use rbx_lua::plugin::{Input, UiId, WidgetKind};

use super::float::FLOAT;
use super::widgets::pixel_size;
use crate::shell::chrome;
use crate::shell::layout::{initial_edge, Edge, Home, Landing, PluginDock};
use crate::shell::menu::{self, MenuId};
use crate::shell::{Panel, Shell};
use crate::tokens;

impl Shell {
    /// The dock of every widget its plugin has made, open or not: what the
    /// palette can open.
    pub(in crate::shell) fn plugin_dock_panels(&self) -> Vec<Panel> {
        self.plugins
            .widgets
            .docks
            .values()
            .map(|dock| Panel::Plugin(*dock))
            .collect()
    }

    fn dock_widget(&self, dock: PluginDock) -> Option<UiId> {
        let docks = &self.plugins.widgets.docks;
        docks.iter().find(|(_, d)| **d == dock).map(|(id, _)| *id)
    }

    fn widget_open(&self, id: UiId) -> bool {
        self.plugins
            .ui
            .widgets
            .iter()
            .any(|w| w.id == id && w.enabled)
    }

    /// Plugin docks with a place in the layout but no open widget: left out
    /// of the docks the way `Shell::hidden_panels` leaves a set-aside panel
    /// out, so nothing in the layout moves while they are away.
    pub(in crate::shell) fn shut_plugin_docks(&self) -> Vec<Panel> {
        let open = |dock: PluginDock| {
            self.dock_widget(dock)
                .is_some_and(|id| self.widget_open(id))
        };
        Edge::ALL
            .into_iter()
            .flat_map(|edge| self.layout.groups(edge))
            .flat_map(|group| group.panels())
            .chain(self.layout.floating())
            .filter(|panel| matches!(panel, Panel::Plugin(dock) if !open(*dock)))
            .copied()
            .collect()
    }

    /// After `ui_changed`: names each dock widget's panel, gives a newly
    /// opened one a place if it has none, and brings it forward.
    pub(super) fn sync_plugin_docks(&mut self, opened: &[UiId], cx: &mut Context<Self>) {
        let host = self.plugins.host.as_ref();
        let docks = self
            .plugins
            .ui
            .widgets
            .iter()
            .filter_map(|w| match w.kind {
                WidgetKind::Dock { initial, .. } => {
                    let (plugin, key) = host?.widget_owner(w.id)?;
                    let dock = PluginDock::of(&plugin, &key);
                    dock.describe(&w.title, initial_edge(initial));
                    Some((w.id, dock))
                }
                _ => None,
            })
            .collect();
        self.plugins.widgets.docks = docks;
        let shown: Vec<PluginDock> = opened
            .iter()
            .filter_map(|id| self.plugins.widgets.docks.get(id).copied())
            .collect();
        if shown.is_empty() {
            return;
        }
        self.seat_plugin_docks();
        for dock in shown {
            self.layout.activate(Panel::Plugin(dock));
        }
        self.save_settings();
        cx.notify();
    }

    /// Gives every open dock widget without a place in the layout its first
    /// one, which a Reset Layout or a saved layout from before the plugin
    /// also asks for: a floating widget floats, a docked one opens as a dock
    /// of its own on a side edge and as a tab across the bottom, where a
    /// new dock would halve Output's width.
    pub(in crate::shell) fn seat_plugin_docks(&mut self) {
        let unseated: Vec<(PluginDock, u8)> = self
            .plugins
            .ui
            .widgets
            .iter()
            .filter(|w| w.enabled)
            .filter_map(|w| match w.kind {
                WidgetKind::Dock { initial, .. } => {
                    Some((*self.plugins.widgets.docks.get(&w.id)?, initial))
                }
                _ => None,
            })
            .filter(|(dock, _)| !self.layout.contains(Panel::Plugin(*dock)))
            .collect();
        for (dock, initial) in unseated {
            let edge = initial_edge(initial);
            let groups = self.layout.groups(edge).len();
            let landing = match (initial, edge) {
                (FLOAT, _) => Landing::Float,
                (_, Edge::Bottom) if groups > 0 => Landing::Tab {
                    edge,
                    group: 0,
                    tab: usize::MAX,
                },
                _ => Landing::NewGroup {
                    edge,
                    group: groups,
                },
            };
            self.layout.apply(Panel::Plugin(dock), landing);
        }
    }

    /// A plugin dock's ×, its menu's Close, or a View toggle: the widget's
    /// `Enabled` goes false (its `BindToClose` runs) and its place stays.
    pub(in crate::shell) fn close_plugin_dock(&mut self, dock: PluginDock, cx: &mut Context<Self>) {
        if let Some(id) = self.dock_widget(dock) {
            self.plugin_input(Input::WidgetClosed(id), cx);
        }
    }

    /// Opened by name (the palette): the widget comes back where it was.
    pub(in crate::shell) fn open_plugin_dock(&mut self, dock: PluginDock, cx: &mut Context<Self>) {
        let Some(id) = self.dock_widget(dock) else {
            return;
        };
        if self.widget_open(id) {
            self.layout.activate(Panel::Plugin(dock));
            cx.notify();
        } else {
            self.plugin_input(Input::WidgetOpened(id), cx);
        }
    }

    /// Whether a plugin dock is floating over the editor.
    pub(super) fn plugin_dock_floats(&self, id: UiId) -> bool {
        self.plugins
            .widgets
            .docks
            .get(&id)
            .is_some_and(|dock| self.layout.home_of(Panel::Plugin(*dock)) == Home::Floating)
    }

    /// The overflow menu a plugin dock wears, docked or floating: the same
    /// Move to / Float / Close every dock has.
    pub(super) fn plugin_dock_menu(&self, id: UiId, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dock = *self.plugins.widgets.docks.get(&id)?;
        let trigger = chrome::icon_trigger(
            ElementId::Name(format!("plugin-dock-overflow-{id}").into()),
            IconName::Ellipsis,
            "Widget settings",
        );
        let items = self.move_items(Panel::Plugin(dock));
        Some(
            menu::dropdown(self, MenuId::PluginDockOverflow(id), trigger, items, cx)
                .into_any_element(),
        )
    }

    /// One plugin dock's trailing menu and content, for
    /// `workspace::panel_parts`.
    pub(in crate::shell) fn plugin_dock(
        &mut self,
        dock: PluginDock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let Some(id) = self.dock_widget(dock) else {
            return (None, None);
        };
        let revision = self
            .plugins
            .ui
            .widgets
            .iter()
            .find(|w| w.id == id)
            .map_or(0, |w| w.revision);
        let measured = self.plugins.widgets.areas.get(&id).and_then(|a| a.get());
        if let Some(bounds) = measured {
            self.request_widget_render(id, revision, pixel_size(bounds), cx);
        }
        let surface = self.widget_surface(id, window, cx);
        let content = div()
            .flex_1()
            .min_h_0()
            .w_full()
            .bg(tokens::dock())
            .child(surface);
        (
            self.plugin_dock_menu(id, cx),
            Some(content.into_any_element()),
        )
    }
}
