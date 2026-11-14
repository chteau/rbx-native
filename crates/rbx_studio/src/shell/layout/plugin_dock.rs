//! A plugin's dock widget as a [`Panel`](super::Panel) of its own.
//!
//! The native panels are a closed set known at compile time; a plugin's
//! `DockWidgetPluginGui` is not, yet it has to be a `Copy` value the layout
//! can hold, save by name and compare like any other panel. So each widget
//! identity (plugin name and widget id, the pair the host also keys its
//! settings by) is interned here once and the panel holds its index.
//! Interned names are leaked to give them the `&'static str` every panel's
//! key is: one per widget the user ever had, so the leak is bounded by the
//! plugins installed, not by time.

use std::sync::{Mutex, PoisonError};

use super::Edge;

/// What a saved layout's name for a plugin dock starts with. No native
/// panel's key does, so the two can never be confused.
const PREFIX: &str = "plugin:";

/// One plugin dock widget, as a layout holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PluginDock(u32);

struct Entry {
    key: &'static str,
    /// The widget's `Title`, or its id until the plugin has run.
    title: String,
    /// The edge it docks to when nothing says otherwise.
    home: Edge,
}

static DOCKS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

fn with_docks<T>(f: impl FnOnce(&mut Vec<Entry>) -> T) -> T {
    f(&mut DOCKS.lock().unwrap_or_else(PoisonError::into_inner))
}

/// The edge a widget's `Enum.InitialDockState` (Top 0, Bottom 1, Left 2,
/// Right 3, Float 4) docks to. The editor has no top edge: Top lands on the
/// right, where a side widget's narrow column fits, and so does Float when
/// it is docked from its window.
pub(crate) fn initial_edge(initial: u8) -> Edge {
    match initial {
        1 => Edge::Bottom,
        2 => Edge::Left,
        _ => Edge::Right,
    }
}

impl PluginDock {
    /// The dock for widget `widget` of plugin `plugin`.
    pub(crate) fn of(plugin: &str, widget: &str) -> Self {
        Self::intern(&format!("{PREFIX}{plugin}/{widget}"))
    }

    /// The dock a saved layout's name refers to, if it names one.
    pub(super) fn from_key(key: &str) -> Option<Self> {
        key.starts_with(PREFIX).then(|| Self::intern(key))
    }

    fn intern(key: &str) -> Self {
        with_docks(|docks| {
            let index = docks
                .iter()
                .position(|entry| entry.key == key)
                .unwrap_or_else(|| {
                    docks.push(Entry {
                        key: Box::leak(key.into()),
                        title: key.trim_start_matches(PREFIX).to_owned(),
                        home: Edge::Right,
                    });
                    docks.len() - 1
                });
            PluginDock(index as u32)
        })
    }

    /// Records what the running plugin says about its widget.
    pub(crate) fn describe(self, title: &str, home: Edge) {
        with_docks(|docks| {
            let entry = &mut docks[self.0 as usize];
            if entry.title != title {
                entry.title = title.to_owned();
            }
            entry.home = home;
        });
    }

    pub(super) fn key(self) -> &'static str {
        with_docks(|docks| docks[self.0 as usize].key)
    }

    pub(super) fn title(self) -> String {
        with_docks(|docks| docks[self.0 as usize].title.clone())
    }

    pub(super) fn home(self) -> Edge {
        with_docks(|docks| docks[self.0 as usize].home)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Home, Landing, Layout, Panel};
    use super::*;

    #[test]
    fn one_widget_is_one_dock_whatever_asks_for_it() {
        let dock = PluginDock::of("Archimedes", "Main");
        assert_eq!(dock, PluginDock::of("Archimedes", "Main"));
        assert_ne!(dock, PluginDock::of("Archimedes", "Settings"));
        assert_eq!(PluginDock::from_key(dock.key()), Some(dock));
        assert_eq!(PluginDock::from_key("Explorer"), None);
    }

    #[test]
    fn a_widget_reads_its_title_once_its_plugin_describes_it() {
        let dock = PluginDock::of("Titles", "w");
        assert_eq!(Panel::Plugin(dock).title(), "Titles/w");
        dock.describe("Collaborators", Edge::Left);
        assert_eq!(Panel::Plugin(dock).title(), "Collaborators");
        assert_eq!(Panel::Plugin(dock).home(), Edge::Left);
    }

    #[test]
    fn the_initial_dock_state_picks_the_edge() {
        assert_eq!(initial_edge(0), Edge::Right);
        assert_eq!(initial_edge(1), Edge::Bottom);
        assert_eq!(initial_edge(2), Edge::Left);
        assert_eq!(initial_edge(3), Edge::Right);
        assert_eq!(initial_edge(4), Edge::Right);
    }

    #[test]
    fn plugin_docks_keep_their_own_places_across_a_restart() {
        let a = Panel::Plugin(PluginDock::of("F3X", "Tools"));
        let b = Panel::Plugin(PluginDock::of("Archimedes", "Main"));
        let mut layout = Layout::default();
        assert!(!layout.contains(a));
        layout.apply(
            a,
            Landing::NewGroup {
                edge: Edge::Left,
                group: 1,
            },
        );
        layout.apply(
            b,
            Landing::NewGroup {
                edge: Edge::Right,
                group: 0,
            },
        );

        let restored = Layout::restore(&layout.saved());
        assert_eq!(restored, layout);
        assert_eq!(
            restored.home_of(a),
            Home::Docked {
                edge: Edge::Left,
                group: 1
            }
        );
        assert_eq!(
            restored.home_of(b),
            Home::Docked {
                edge: Edge::Right,
                group: 0
            }
        );
    }

    #[test]
    fn a_floating_plugin_dock_floats_again_after_a_restart() {
        let panel = Panel::Plugin(PluginDock::of("Floaty", "w"));
        let mut layout = Layout::default();
        layout.apply(panel, Landing::Float);
        assert_eq!(
            Layout::restore(&layout.saved()).home_of(panel),
            Home::Floating
        );
    }

    #[test]
    fn the_old_plugin_widgets_dock_is_dropped_from_a_saved_layout() {
        let mut legacy = Layout::default().saved();
        let right = legacy.edges.iter_mut().find(|e| e.edge == Edge::Right);
        right.unwrap().groups[0]
            .panels
            .push("Plugin Widgets".to_owned());
        legacy.closed.push("Plugin Widgets".to_owned());

        let restored = Layout::restore(&legacy);
        assert_eq!(restored, Layout::default());
        let names = restored.saved();
        let every = names
            .edges
            .iter()
            .flat_map(|edge| &edge.groups)
            .flat_map(|group| &group.panels)
            .chain(&names.floating)
            .chain(&names.closed);
        assert!(every.into_iter().all(|name| name != "Plugin Widgets"));
    }
}
