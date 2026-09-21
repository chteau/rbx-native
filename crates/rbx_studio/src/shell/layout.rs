//! Where each dock is: which edge, stacked where on it, which tab, or a
//! window of its own.
//!
//! This is the data that used to be source order. `shell::workspace` built
//! Row D as three hardcoded `.child()` calls, so "Properties is on the
//! left" was not written down anywhere and there was nothing to change at
//! runtime; a panel's identity was its call site. Here a panel is a
//! [`Panel`] value, its home is a [`Home`], and every rearrangement is one
//! call to [`Layout::apply`] with the [`Landing`] a drop worked out.
//!
//! **Two levels, not a tree of arbitrary splits.** An edge holds a stack
//! of [`Group`]s and a group holds a stack of tabs; that is exactly what a
//! drop can express — land on a dock's tab strip and you are a tab of it,
//! land on its top or bottom half and you are a new dock above or below
//! it. The recursive `Split { axis, fraction, before, after }` the
//! architecture note sketched can nest further than that, but nothing in
//! the gesture can *ask* for deeper nesting, so the extra level would be
//! structure nobody can reach. Growing into a tree later means replacing
//! this file rather than unpicking it, because nothing outside it knows
//! the shape.

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
    /// and its torn-out window's title bar read.
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

/// One dock: the panels tabbed together in it, and which tab is showing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Group {
    panels: Vec<Panel>,
    active: usize,
}

impl Group {
    fn new(panel: Panel) -> Self {
        Group {
            panels: vec![panel],
            active: 0,
        }
    }

    pub(crate) fn panels(&self) -> &[Panel] {
        &self.panels
    }

    /// The tab currently showing. Never `None` for a group that exists —
    /// [`Layout`] drops a group the moment its last tab leaves.
    pub(crate) fn active(&self) -> Option<Panel> {
        self.panels.get(self.active).copied()
    }
}

/// Where a panel lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Home {
    /// A tab of the dock at `group` on `edge`.
    Docked { edge: Edge, group: usize },
    /// Torn out into a window of its own.
    Floating,
    /// Shut, and reachable only from the View menu or the ribbon's Home
    /// tab. A closed panel keeps nothing — reopening puts it back on its
    /// own default edge, because a layout that remembered where a panel
    /// was before it was closed would have to keep a slot open for it.
    Closed,
}

/// Where a drop would put the panel it is carrying.
///
/// Worked out by the hit test in `shell::dock_drag` and handed to
/// [`Layout::apply`], so that what the overlay promises and what the drop
/// does are the same value rather than two pieces of logic that agree
/// until one of them is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Landing {
    /// As a tab of an existing dock, at that position in its strip.
    Tab {
        edge: Edge,
        group: usize,
        tab: usize,
    },
    /// As a new dock on `edge`, stacked at that position among its docks.
    NewGroup { edge: Edge, group: usize },
    /// In a window of its own.
    Float,
}

/// Which panels sit where.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layout {
    /// Per edge, the docks stacked along it. A panel appears in exactly
    /// one group or in [`Self::floating`] — never both, never twice — and
    /// no group is ever empty. Every mutation here preserves both, and
    /// every reader assumes them.
    edges: [Vec<Group>; 3],
    /// Panels torn out into windows of their own.
    floating: Vec<Panel>,
    /// Panels that have been shut. Held rather than simply absent so that
    /// `restore` can tell "this file predates the panel" — put it back —
    /// from "the user closed it" — leave it shut.
    closed: Vec<Panel>,
    /// Per edge, its size across its own axis.
    size: [f32; 3],
}

impl Default for Layout {
    fn default() -> Self {
        let mut edges: [Vec<Group>; 3] = Default::default();
        for panel in Panel::ALL {
            edges[panel.home().index()].push(Group::new(panel));
        }
        Self {
            edges,
            floating: Vec::new(),
            closed: Vec::new(),
            size: Edge::ALL.map(Edge::default_size),
        }
    }
}

impl Layout {
    /// The docks stacked on one edge. Empty for an edge nobody has put
    /// anything on, which is what lets the document take its room.
    pub(crate) fn groups(&self, edge: Edge) -> &[Group] {
        &self.edges[edge.index()]
    }

    /// The panels in windows of their own.
    pub(crate) fn floating(&self) -> &[Panel] {
        &self.floating
    }

    /// Whether a panel is showing anywhere at all — what a View menu or a
    /// ribbon button ticks.
    pub(crate) fn is_open(&self, panel: Panel) -> bool {
        self.home_of(panel) != Home::Closed
    }

    /// Where a panel is. Every panel is always exactly one of these.
    pub(crate) fn home_of(&self, panel: Panel) -> Home {
        if self.closed.contains(&panel) {
            return Home::Closed;
        }
        if self.floating.contains(&panel) {
            return Home::Floating;
        }
        for edge in Edge::ALL {
            for (index, group) in self.groups(edge).iter().enumerate() {
                if group.panels.contains(&panel) {
                    return Home::Docked { edge, group: index };
                }
            }
        }
        Home::Docked {
            edge: panel.home(),
            group: 0,
        }
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
    pub(crate) fn reset_sizes(&mut self) {
        self.size = Edge::ALL.map(Edge::default_size);
    }

    /// Takes a panel off whatever holds it, dropping a dock that has just
    /// lost its last tab and walking back any tab index the removal left
    /// dangling.
    fn detach(&mut self, panel: Panel) {
        self.floating.retain(|held| *held != panel);
        self.closed.retain(|held| *held != panel);
        for edge in Edge::ALL {
            let groups = &mut self.edges[edge.index()];
            for group in groups.iter_mut() {
                group.panels.retain(|held| *held != panel);
                group.active = group.active.min(group.panels.len().saturating_sub(1));
            }
            groups.retain(|group| !group.panels.is_empty());
        }
    }

    /// Puts a panel wherever a drop said it should go.
    ///
    /// The single mutation every gesture funnels through — the drag, the
    /// menu's "Move to" and its "Float" alike — so no two of them can
    /// disagree about what a move means.
    pub(crate) fn apply(&mut self, panel: Panel, landing: Landing) {
        // Read before detaching: the indices a landing carries were
        // computed against the layout as it stands, and taking the panel
        // out first can shift them.
        let from = self.home_of(panel);
        match landing {
            Landing::Float => {
                if self.home_of(panel) == Home::Floating {
                    return;
                }
                self.detach(panel);
                self.floating.push(panel);
            }
            Landing::Tab { edge, group, tab } => {
                self.detach(panel);
                let index = self.shifted(edge, group, from);
                let groups = &mut self.edges[edge.index()];
                match groups.get_mut(index) {
                    Some(target) => {
                        let at = tab.min(target.panels.len());
                        target.panels.insert(at, panel);
                        target.active = at;
                    }
                    // The dock this was aimed at was the panel's own, and
                    // taking it out emptied it: it becomes a dock again
                    // where that one was.
                    None => groups.insert(index.min(groups.len()), Group::new(panel)),
                }
            }
            Landing::NewGroup { edge, group } => {
                self.detach(panel);
                let index = self.shifted(edge, group, from);
                let groups = &mut self.edges[edge.index()];
                let at = index.min(groups.len());
                groups.insert(at, Group::new(panel));
            }
        }
    }

    /// A group index, corrected for the group that `detach` may have just
    /// removed from the same edge above it.
    fn shifted(&self, edge: Edge, group: usize, from: Home) -> usize {
        match from {
            Home::Docked {
                edge: was,
                group: index,
            } if was == edge && index < group && self.edges[edge.index()].len() < group + 1 => {
                group - 1
            }
            _ => group,
        }
    }

    /// Tears a panel out into a window of its own. A thin wrapper over
    /// [`Self::apply`] so that every gesture goes through one door.
    pub(crate) fn float(&mut self, panel: Panel) {
        self.apply(panel, Landing::Float);
    }

    /// Shuts a panel. Its dock goes with it if it was the last tab.
    pub(crate) fn close(&mut self, panel: Panel) {
        if self.home_of(panel) == Home::Closed {
            return;
        }
        self.detach(panel);
        self.closed.push(panel);
    }

    /// Reopens a shut panel on its own default edge.
    pub(crate) fn open(&mut self, panel: Panel) {
        if self.is_open(panel) {
            return;
        }
        self.detach(panel);
        self.edges[panel.home().index()].push(Group::new(panel));
    }

    /// Shows one of a dock's tabs. Ignores a panel that is not docked,
    /// rather than moving it — a tab click is not a rearrangement.
    pub(crate) fn activate(&mut self, panel: Panel) {
        let Home::Docked { edge, group } = self.home_of(panel) else {
            return;
        };
        let Some(group) = self.edges[edge.index()].get_mut(group) else {
            return;
        };
        if let Some(index) = group.panels.iter().position(|held| *held == panel) {
            group.active = index;
        }
    }

    /// Rebuilds a layout from what the settings file had, and is the only
    /// way a `Layout` is ever made from outside its own default.
    ///
    /// Total by construction: a name this version does not know is
    /// dropped, a panel the file never mentions is put back on its own
    /// default edge, a panel named twice keeps its first mention, an empty
    /// dock is dropped, a tab index past the end walks back, and a size
    /// outside its range is clamped. A layout from a future version
    /// therefore opens the editor rather than stopping it, which is how
    /// `Settings::load` already treats every other field.
    pub(crate) fn restore(saved: &SavedLayout) -> Self {
        let mut layout = Layout {
            edges: Default::default(),
            floating: Vec::new(),
            ..Layout::default()
        };

        for entry in &saved.edges {
            if entry.size > 0. {
                layout.resize(entry.edge, entry.size);
            }
            for group in &entry.groups {
                let panels: Vec<Panel> = group
                    .panels
                    .iter()
                    .filter_map(|name| layout.unclaimed(name))
                    .fold(Vec::new(), |mut kept, panel| {
                        // Folded rather than collected so each name is
                        // checked against the ones before it in its own
                        // group as well as against the other edges.
                        if !kept.contains(&panel) {
                            kept.push(panel);
                        }
                        kept
                    });
                if panels.is_empty() {
                    continue;
                }
                let active = group.active.min(panels.len() - 1);
                layout.edges[entry.edge.index()].push(Group { panels, active });
            }
        }

        for name in &saved.floating {
            if let Some(panel) = layout.unclaimed(name) {
                layout.floating.push(panel);
            }
        }

        for name in &saved.closed {
            if let Some(panel) = layout.unclaimed(name) {
                layout.closed.push(panel);
            }
        }

        for panel in Panel::ALL {
            if layout.unclaimed(panel.key()).is_some() {
                layout.edges[panel.home().index()].push(Group::new(panel));
            }
        }

        layout
    }

    /// The panel `name` refers to, if this version knows it and nothing
    /// has taken it yet — the guard that keeps a file naming one panel
    /// twice from putting it in two places at once.
    fn unclaimed(&self, name: &str) -> Option<Panel> {
        Panel::from_key(name).filter(|panel| {
            !self.floating.contains(panel)
                && !self.closed.contains(panel)
                && !self
                    .edges
                    .iter()
                    .flatten()
                    .any(|group| group.panels.contains(panel))
        })
    }

    /// This layout as the settings file stores it.
    pub(crate) fn saved(&self) -> SavedLayout {
        SavedLayout {
            edges: Edge::ALL
                .into_iter()
                .map(|edge| SavedEdge {
                    edge,
                    groups: self
                        .groups(edge)
                        .iter()
                        .map(|group| SavedGroup {
                            panels: group.panels.iter().map(|p| p.key().to_owned()).collect(),
                            active: group.active,
                        })
                        .collect(),
                    size: self.size(edge),
                })
                .collect(),
            floating: self.floating.iter().map(|p| p.key().to_owned()).collect(),
            closed: self.closed.iter().map(|p| p.key().to_owned()).collect(),
        }
    }
}

/// One dock as the settings file holds it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SavedGroup {
    pub(crate) panels: Vec<String>,
    pub(crate) active: usize,
}

/// One edge as the settings file holds it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SavedEdge {
    pub(crate) edge: Edge,
    pub(crate) groups: Vec<SavedGroup>,
    /// The edge's size, or 0 for "never set" — which `Settings` writes for
    /// a field it has no value for.
    pub(crate) size: f32,
}

/// A whole layout as the settings file holds it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SavedLayout {
    pub(crate) edges: Vec<SavedEdge>,
    /// Panels that were in windows of their own.
    pub(crate) floating: Vec<String>,
    /// Panels that were shut.
    pub(crate) closed: Vec<String>,
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
