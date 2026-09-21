//! Where each dock is: which edge, which tab, or a window of its own.
//!
//! This is the data that used to be source order. `shell::workspace` built
//! Row D as three hardcoded `.child()` calls, so "Properties is on the
//! left" was not written down anywhere and there was nothing to change at
//! runtime; a panel's identity was its call site. Here a panel is a
//! [`Panel`] value, its home is a [`Home`], and rearranging is a function
//! on this struct rather than an edit to the render code.
//!
//! **Three edges around a fixed centre, not a tree of splits.** The
//! architecture note that flagged this work proposed a recursive
//! `Split { axis, fraction, before, after }`, which expresses arbitrary
//! nesting. This expresses what the docks actually do: one document in the
//! middle, panels parked on its left, right or bottom as tabs, and any of
//! them torn out into its own window. The case a tree adds on top — two
//! panels splitting one edge unevenly, a panel wedged into a corner — is
//! not something a tab strip can express anyway. Growing into a tree later
//! means replacing this file rather than unpicking it, because nothing
//! outside it knows the shape.

use std::fmt;

use crate::tokens;

/// One side of the document.
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

    /// How far this edge may be dragged. Past either end the panel on it
    /// stops being usable rather than merely small. A side dock is also
    /// capped at a share of the window at render time (see
    /// `shell::workspace`), which this cannot know.
    pub(crate) fn range(self) -> (f32, f32) {
        match self {
            Edge::Bottom => (80., 400.),
            _ => (200., 560.),
        }
    }

    /// What this edge starts at, and what Reset Layout puts it back to.
    ///
    /// Read through `tokens` rather than held as a constant: the frame
    /// fixes a dock at 228px against a 9px label, this shell sets text at
    /// 14px, and at 2x the UI scale a dock that stayed 300px would hold
    /// 600px rows.
    pub(crate) fn default_size(self) -> f32 {
        match self {
            Edge::Bottom => 180.,
            _ => tokens::dock_width(),
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

/// Where a panel lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Home {
    /// A tab on one of the window's three edges.
    Docked(Edge),
    /// Torn out into a window of its own.
    Floating,
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
    /// where it goes when a saved layout has lost track of it, and where
    /// closing its torn-out window puts it back.
    pub(crate) fn home(self) -> Edge {
        match self {
            Panel::Explorer => Edge::Right,
            Panel::Properties => Edge::Left,
            Panel::Output => Edge::Bottom,
        }
    }

    /// The name persisted in the settings file, which is also what its tab
    /// and its torn-out window's title bar read. Deliberately the
    /// variant's own spelling, so a hand-edited file reads the way the
    /// editor does.
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

/// A whole layout as the settings file holds it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SavedLayout {
    pub(crate) edges: Vec<SavedEdge>,
    /// Panels that were in windows of their own.
    pub(crate) floating: Vec<String>,
}

/// Which panels sit where.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layout {
    /// Per edge, its panels in tab order. A panel appears in exactly one
    /// edge or in [`Self::floating`] — never both, never twice. Every
    /// mutation here preserves that, and every reader assumes it.
    edges: [Vec<Panel>; 3],
    /// Per edge, which of its tabs is showing.
    active: [usize; 3],
    /// Panels torn out into windows of their own.
    floating: Vec<Panel>,
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
            floating: Vec::new(),
            size: Edge::ALL.map(Edge::default_size),
        }
    }
}

impl Layout {
    /// The panels on one edge, in tab order. Empty for an edge nobody has
    /// put anything on, which is what lets the document take its room.
    pub(crate) fn panels(&self, edge: Edge) -> &[Panel] {
        &self.edges[edge.index()]
    }

    /// Which tab that edge is showing, if it holds any.
    pub(crate) fn active(&self, edge: Edge) -> Option<Panel> {
        self.panels(edge).get(self.active[edge.index()]).copied()
    }

    /// The panels in windows of their own.
    pub(crate) fn floating(&self) -> &[Panel] {
        &self.floating
    }

    /// Where a panel is. Every panel is always exactly one of these.
    pub(crate) fn home_of(&self, panel: Panel) -> Home {
        if self.floating.contains(&panel) {
            return Home::Floating;
        }
        Edge::ALL
            .into_iter()
            .find(|edge| self.panels(*edge).contains(&panel))
            .map_or(Home::Docked(panel.home()), Home::Docked)
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

    /// Re-derives every edge's size from the current UI scale, which is
    /// also what Reset Layout does.
    ///
    /// Sizes live in this struct rather than in tokens, so a scale change
    /// has to reach them explicitly or a 2x scale leaves a 300px dock
    /// holding 600px rows.
    pub(crate) fn reset_sizes(&mut self) {
        self.size = Edge::ALL.map(Edge::default_size);
    }

    /// Takes a panel off whatever holds it, leaving the layout consistent.
    ///
    /// The edge it left keeps showing something: a tab index past the end
    /// of a shortened list renders nothing at all, so it walks back to the
    /// last tab rather than leaving the dock blank.
    fn detach(&mut self, panel: Panel) {
        self.floating.retain(|held| *held != panel);
        for edge in Edge::ALL {
            let index = edge.index();
            self.edges[index].retain(|held| *held != panel);
            let last = self.edges[index].len().saturating_sub(1);
            self.active[index] = self.active[index].min(last);
        }
    }

    /// Docks a panel on an edge and shows it — a panel you just dropped is
    /// the one you want to look at.
    ///
    /// `before` is the tab position to insert at, which is what lets a drop
    /// between two tabs reorder a strip rather than only append to it.
    /// `None` appends.
    pub(crate) fn dock(&mut self, panel: Panel, to: Edge, before: Option<usize>) {
        if before.is_none() && self.home_of(panel) == Home::Docked(to) {
            return;
        }
        self.detach(panel);

        let index = to.index();
        let at = before.unwrap_or(usize::MAX).min(self.edges[index].len());
        self.edges[index].insert(at, panel);
        self.active[index] = at;
    }

    /// Tears a panel out into a window of its own.
    pub(crate) fn float(&mut self, panel: Panel) {
        if self.home_of(panel) == Home::Floating {
            return;
        }
        self.detach(panel);
        self.floating.push(panel);
    }

    /// Shows one of an edge's tabs. Ignores a panel that is not docked,
    /// rather than moving it — a tab click is not a rearrangement.
    pub(crate) fn activate(&mut self, panel: Panel) {
        let Home::Docked(edge) = self.home_of(panel) else {
            return;
        };
        if let Some(index) = self.panels(edge).iter().position(|held| *held == panel) {
            self.active[edge.index()] = index;
        }
    }

    /// Rebuilds a layout from what the settings file had, and is the only
    /// way a `Layout` is ever made from outside its own default.
    ///
    /// Total by construction: a name this version does not know is
    /// dropped, a panel the file never mentions is put back on its own
    /// default edge, a panel named twice keeps its first mention, a tab
    /// index past the end walks back, and a size outside its range is
    /// clamped. A layout from a future version therefore opens the editor
    /// rather than stopping it, which is how `Settings::load` already
    /// treats every other field.
    pub(crate) fn restore(saved: &SavedLayout) -> Self {
        let mut layout = Layout {
            edges: Default::default(),
            floating: Vec::new(),
            ..Layout::default()
        };

        for entry in &saved.edges {
            let index = entry.edge.index();
            if entry.size > 0. {
                layout.resize(entry.edge, entry.size);
            }
            for name in &entry.panels {
                if let Some(panel) = layout.unclaimed(name) {
                    layout.edges[index].push(panel);
                }
            }
            layout.active[index] = entry.active;
        }

        for name in &saved.floating {
            if let Some(panel) = layout.unclaimed(name) {
                layout.floating.push(panel);
            }
        }

        for panel in Panel::ALL {
            if layout.unclaimed(panel.key()).is_some() {
                layout.edges[panel.home().index()].push(panel);
            }
        }

        // Only now that every edge holds its final list: an index saved
        // against a longer one, or against panels this version dropped,
        // would otherwise leave a dock showing nothing.
        for edge in Edge::ALL {
            let last = layout.panels(edge).len().saturating_sub(1);
            layout.active[edge.index()] = layout.active[edge.index()].min(last);
        }

        layout
    }

    /// The panel `name` refers to, if this version knows it and nothing
    /// has taken it yet — the guard that keeps a file naming one panel
    /// twice from putting it in two places at once.
    fn unclaimed(&self, name: &str) -> Option<Panel> {
        Panel::from_key(name).filter(|panel| {
            !self.floating.contains(panel) && !self.edges.iter().any(|held| held.contains(panel))
        })
    }

    /// This layout as the settings file stores it.
    pub(crate) fn saved(&self) -> SavedLayout {
        SavedLayout {
            edges: Edge::ALL
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
                .collect(),
            floating: self
                .floating
                .iter()
                .map(|panel| panel.key().to_owned())
                .collect(),
        }
    }
}

/// The settings file's own spelling of an edge, and the way back.
///
/// Free functions rather than `Edge` methods so `settings` can round-trip
/// the layout without the enum's internals leaking into it.
pub(crate) fn edge_key(edge: Edge) -> &'static str {
    edge.key()
}

pub(crate) fn edge_from_key(key: &str) -> Option<Edge> {
    Edge::ALL.into_iter().find(|edge| edge.key() == key)
}

#[cfg(test)]
#[path = "layout/tests.rs"]
mod tests;
