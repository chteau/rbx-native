//! Where each dock sits, and how big its edge is.
//!
//! This is the data that used to be source order. `shell::workspace` built
//! Row D as three hardcoded `.child()` calls, so "Properties is on the
//! left" was not written down anywhere and there was nothing to change at
//! runtime; a panel's identity was its call site. Here a panel is a
//! [`Panel`] value, its home is an [`Edge`], and moving one is a function
//! on this struct rather than an edit to the render code.
//!
//! **Three edges around a fixed centre, not a tree of splits.** The
//! architecture note that flagged this work proposed a recursive
//! `Split { axis, fraction, before, after }`, which expresses arbitrary
//! nesting. This expresses what this editor has: one document in the
//! middle, and panels parked on its left, right or bottom, tabbed where
//! several share an edge. Every rearrangement the docks actually support —
//! any panel to any edge, several together, an edge emptied so the
//! viewport takes the room — is reachable, and the cases a tree adds
//! (a panel stacked *above* another on the same edge, a panel in a corner)
//! are not asked for by anything. The trade is recorded here rather than
//! hidden: growing into a tree later means replacing this file, not
//! unpicking it, because nothing outside it knows the shape.

use std::fmt;

/// One side of the document, and the only places a panel can live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Edge {
    Left,
    Right,
    Bottom,
}

impl Edge {
    /// Every edge, in the order they are drawn around the document.
    pub(crate) const ALL: [Edge; 3] = [Edge::Left, Edge::Right, Edge::Bottom];

    fn index(self) -> usize {
        match self {
            Edge::Left => 0,
            Edge::Right => 1,
            Edge::Bottom => 2,
        }
    }

    /// Whether this edge's size is a width rather than a height — which is
    /// also which axis a drag on its handle reads.
    pub(crate) fn is_vertical(self) -> bool {
        !matches!(self, Edge::Bottom)
    }

    /// How far this edge may be dragged. A side dock is also capped at a
    /// share of the window at render time (see `shell::workspace`), which
    /// this cannot know.
    pub(crate) fn range(self) -> (f32, f32) {
        match self {
            Edge::Bottom => (80., 400.),
            _ => (200., 560.),
        }
    }

    /// The word a menu entry uses for it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Edge::Left => "Left",
            Edge::Right => "Right",
            Edge::Bottom => "Bottom",
        }
    }

    /// The spelling persisted in the settings file.
    fn key(self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Right => "right",
            Edge::Bottom => "bottom",
        }
    }
}

/// A panel that can be moved. The identity a layout stores, in place of
/// the function call that used to *be* the identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Panel {
    Explorer,
    Properties,
    Output,
}

impl Panel {
    pub(crate) const ALL: [Panel; 3] = [Panel::Explorer, Panel::Properties, Panel::Output];

    /// Where this panel lives in a layout nobody has rearranged — also
    /// where it goes when a saved layout has lost track of it.
    fn home(self) -> Edge {
        match self {
            Panel::Explorer => Edge::Right,
            Panel::Properties => Edge::Left,
            Panel::Output => Edge::Bottom,
        }
    }

    /// The name persisted in the settings file. Deliberately the variant's
    /// own spelling, so a hand-edited file reads the way the menu does.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Panel::Explorer => "Explorer",
            Panel::Properties => "Properties",
            Panel::Output => "Output",
        }
    }

    fn from_key(key: &str) -> Option<Panel> {
        Panel::ALL.into_iter().find(|panel| panel.key() == key)
    }
}

impl fmt::Display for Panel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// One edge as the settings file holds it.
///
/// A struct rather than a tuple because `restore` has to be total over
/// nonsense — a name this version has never heard of, a panel named twice,
/// a tab index past the end — and reading that logic against
/// `(Edge, Vec<String>, f32, usize)` is how the wrong field gets used.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SavedEdge {
    pub(crate) edge: Edge,
    /// Panel names in tab order; anything unrecognised is dropped.
    pub(crate) panels: Vec<String>,
    /// The edge's size, or 0 for "never set" — which `Settings` writes for
    /// a field it has no value for.
    pub(crate) size: f32,
    /// Which tab was showing, clamped on the way back in.
    pub(crate) active: usize,
}

/// The default size of each edge, in the order [`Edge::index`] gives.
const DEFAULT_SIZES: [f32; 3] = [300., 300., 180.];

/// Which panels sit on which edge, in which order, and how big each edge
/// is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layout {
    /// Per edge, its panels in tab order. A panel appears in exactly one of
    /// the three, which every mutation here preserves.
    edges: [Vec<Panel>; 3],
    /// Per edge, which of its panels is showing.
    active: [usize; 3],
    /// Per edge, its size across its own axis.
    size: [f32; 3],
}

impl Default for Layout {
    fn default() -> Self {
        let mut edges: [Vec<Panel>; 3] = Default::default();
        for panel in Panel::ALL {
            edges[panel.home().index()].push(panel);
        }
        Self {
            edges,
            active: [0; 3],
            size: DEFAULT_SIZES,
        }
    }
}

impl Layout {
    /// The panels on one edge, in tab order. Empty for an edge nobody has
    /// put anything on, which is what lets the document take its room.
    pub(crate) fn panels(&self, edge: Edge) -> &[Panel] {
        &self.edges[edge.index()]
    }

    /// Which panel that edge is currently showing, if it holds any.
    pub(crate) fn active(&self, edge: Edge) -> Option<Panel> {
        let panels = self.panels(edge);
        panels.get(self.active[edge.index()]).copied()
    }

    /// Which edge a panel is on. Every panel is always on exactly one.
    pub(crate) fn edge_of(&self, panel: Panel) -> Edge {
        Edge::ALL
            .into_iter()
            .find(|edge| self.panels(*edge).contains(&panel))
            .unwrap_or_else(|| panel.home())
    }

    pub(crate) fn size(&self, edge: Edge) -> f32 {
        self.size[edge.index()]
    }

    /// Sets one edge's size, clamped to what that edge allows.
    pub(crate) fn resize(&mut self, edge: Edge, size: f32) {
        let (low, high) = edge.range();
        self.size[edge.index()] = size.clamp(low, high);
    }

    /// Caps an edge without recording it as the user's choice.
    ///
    /// The render pass shrinks a side dock that has outgrown its share of
    /// the window (see `shell::workspace`), and that has to be a *display*
    /// cap: writing it back would mean a window narrowed once and widened
    /// again lost the size the user had actually picked.
    pub(crate) fn capped(&self, edge: Edge, limit: f32) -> f32 {
        self.size(edge).min(limit)
    }

    /// Moves a panel to an edge, as the tab after whatever is already
    /// there. A no-op when it is already on that edge, so a menu entry for
    /// where a panel already lives cannot reorder its own tabs.
    ///
    /// The edge it left keeps showing something: a tab index past the end
    /// of a shortened list would render nothing at all, so it walks back
    /// to the last tab rather than leaving the dock blank.
    pub(crate) fn move_panel(&mut self, panel: Panel, to: Edge) {
        let from = self.edge_of(panel);
        if from == to {
            return;
        }

        let leaving = &mut self.edges[from.index()];
        leaving.retain(|held| *held != panel);
        let remaining = leaving.len();
        self.active[from.index()] = self.active[from.index()].min(remaining.saturating_sub(1));

        let arriving = &mut self.edges[to.index()];
        arriving.push(panel);
        // A panel that just moved is the one you want to look at.
        self.active[to.index()] = arriving.len() - 1;
    }

    /// Shows one of an edge's tabs. Ignores a panel that is not on it,
    /// rather than moving it — a tab click is not a rearrangement.
    pub(crate) fn activate(&mut self, panel: Panel) {
        let edge = self.edge_of(panel);
        if let Some(index) = self.panels(edge).iter().position(|held| *held == panel) {
            self.active[edge.index()] = index;
        }
    }

    /// Rebuilds a layout from what the settings file had, and is the only
    /// way a `Layout` is ever made from outside.
    ///
    /// Total by construction: a name this version does not know is
    /// dropped, a panel the file never mentions is put back on its own
    /// default edge, and a size outside its range is clamped. A layout
    /// file from a future version therefore opens the editor rather than
    /// stopping it, which is how `Settings::load` already treats every
    /// other field.
    pub(crate) fn restore(saved: &[SavedEdge]) -> Self {
        let mut layout = Layout {
            edges: Default::default(),
            active: [0; 3],
            size: DEFAULT_SIZES,
        };

        for entry in saved {
            let index = entry.edge.index();
            if entry.size > 0. {
                layout.resize(entry.edge, entry.size);
            }
            for name in &entry.panels {
                let Some(panel) = Panel::from_key(name) else {
                    continue;
                };
                // A file naming the same panel twice would otherwise put it
                // on two edges at once, which every method here assumes
                // cannot happen.
                if layout.edges.iter().any(|held| held.contains(&panel)) {
                    continue;
                }
                layout.edges[index].push(panel);
            }
            layout.active[index] = entry.active;
        }

        for panel in Panel::ALL {
            if !layout.edges.iter().any(|held| held.contains(&panel)) {
                layout.edges[panel.home().index()].push(panel);
            }
        }

        // Only now that every edge holds its final list: an index saved
        // against a longer list, or against panels this version dropped,
        // would otherwise leave a dock showing nothing.
        for edge in Edge::ALL {
            let last = layout.panels(edge).len().saturating_sub(1);
            layout.active[edge.index()] = layout.active[edge.index()].min(last);
        }

        layout
    }

    /// This layout as the settings file stores it: per edge, its panel
    /// names in tab order and its size.
    pub(crate) fn saved(&self) -> Vec<SavedEdge> {
        Edge::ALL
            .into_iter()
            .map(|edge| SavedEdge {
                edge,
                panels: self
                    .panels(edge)
                    .iter()
                    .map(|panel| panel.key().to_owned())
                    .collect(),
                size: self.size(edge),
                active: self.active[edge.index()],
            })
            .collect()
    }

    /// The settings file's own key for an edge.
    pub(crate) fn key_of(edge: Edge) -> &'static str {
        edge.key()
    }
}

#[cfg(test)]
#[path = "layout/tests.rs"]
mod tests;
