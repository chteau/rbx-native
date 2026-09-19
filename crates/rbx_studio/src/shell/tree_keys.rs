//! The Explorer's keyboard contract — WAI-ARIA's Tree View pattern, in
//! full.
//!
//! "In full" is the point. The APG says outright that a *partial*
//! implementation of the tree pattern is itself an accessibility problem:
//! someone who has learned that Left goes to the parent and finds that it
//! sometimes does nothing has no way to tell a missing feature from a
//! broken one, and falls back to the mouse.
//!
//! The toolkit's `TreeState` binds the four arrows already, but three of
//! its answers are the wrong ones for a tree:
//!
//! - **Up and Down wrap.** Down at the last row jumps to the first. That is
//!   a menu's behaviour, not a tree's — a tree is a *list of a place*, and
//!   teleporting to the other end of the Workspace loses the reader.
//! - **Left on a collapsed node does nothing**, where the pattern says to
//!   move to its parent — which is the whole reason Left is usable as an
//!   "out" key.
//! - **Right on an expanded node does nothing**, where the pattern says to
//!   move to its first child.
//!
//! And three keys aren't bound at all: Home, End, and type-ahead.
//!
//! So this module intercepts the tree's navigation at the *action* layer —
//! `capture_action` on `SelectUp`/`Down`/`Left`/`Right` — and hands back
//! only the two cases where the toolkit is already right (expanding a
//! collapsed node, collapsing an expanded one), because those are the two
//! that need `TreeState`'s private `toggle_expand`.
//!
//! The action layer, not `capture_key_down`, and that distinction cost a
//! round of testing: GPUI resolves a keystroke to an *action* through the
//! keymap and dispatches that separately from key listeners, so stopping
//! propagation on the keystroke does not stop the action. Arrow keys
//! appeared to work while the toolkit's own wrapping handler quietly ran
//! afterwards and overwrote the answer. Type-ahead stays on
//! `capture_key_down`, since plain letters resolve to no action at all.

use std::time::{Duration, Instant};

use gpui_kit::base::actions::{SelectDown, SelectLeft, SelectRight, SelectUp};
use gpui_kit::component::tree::TreeState;
use gpui_kit::{Context, Entity, Keystroke};

use super::Shell;

/// How long a type-ahead buffer survives without another keystroke.
///
/// The APG recommends type-ahead for any tree with more than about seven
/// root nodes, which every real place file has. This is the usual desktop
/// value: long enough to type "Spawn" without rushing, short enough that
/// coming back a moment later starts a fresh search.
const TYPEAHEAD_TIMEOUT: Duration = Duration::from_millis(1000);

/// What a keystroke means to the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Nav {
    Previous,
    Next,
    First,
    Last,
    /// Right on an expanded node: to its first child, which in a flattened
    /// visible list is always the row below.
    Into,
    /// Left on anything that isn't an expanded folder: out to its parent.
    OutToParent,
    /// Left on an expanded folder, or Right on a collapsed one — the two
    /// the toolkit already handles, which this module deliberately lets
    /// through rather than reimplementing against a private API.
    LetToolkitExpandOrCollapse,
    /// The key is the tree's, and its answer is "nothing" — Right on a leaf
    /// and Left at a root. Consumed rather than ignored, so it cannot reach
    /// the toolkit's wrapping bindings underneath.
    Nowhere,
}

/// Classifies a navigation keystroke against the focused row's own state.
///
/// Split out from the `Shell` method below so the whole contract is
/// testable without a window: `expanded` and `is_folder` describe the
/// focused row, and everything else follows from the key.
pub(super) fn nav_for(keystroke: &Keystroke, is_folder: bool, expanded: bool) -> Option<Nav> {
    if keystroke.modifiers.modified() {
        return None;
    }
    match keystroke.key.as_str() {
        "home" => Some(Nav::First),
        "end" => Some(Nav::Last),
        key => arrow(key, is_folder, expanded),
    }
}

/// The four arrows, classified against the focused row's own state. Split
/// from [`nav_for`] because the arrows arrive as actions and Home/End as
/// plain keystrokes — the same contract, two delivery paths.
pub(super) fn arrow(key: &str, is_folder: bool, expanded: bool) -> Option<Nav> {
    match key {
        "up" => Some(Nav::Previous),
        "down" => Some(Nav::Next),
        "right" if is_folder && !expanded => Some(Nav::LetToolkitExpandOrCollapse),
        "right" if is_folder && expanded => Some(Nav::Into),
        // Right on a leaf does nothing at all — but it is still consumed,
        // because letting it fall through hands it to the toolkit, which
        // would wrap the selection somewhere unrelated.
        "right" => Some(Nav::Nowhere),
        "left" if is_folder && expanded => Some(Nav::LetToolkitExpandOrCollapse),
        "left" => Some(Nav::OutToParent),
        _ => None,
    }
}

/// A printable character that should extend the type-ahead buffer.
///
/// One character, no modifiers: anything else is a command, and a tree that
/// swallowed Ctrl+C to search for "c" would be worse than one with no
/// type-ahead at all.
fn typeahead_char(keystroke: &Keystroke) -> Option<char> {
    if keystroke.modifiers.control || keystroke.modifiers.alt || keystroke.modifiers.platform {
        return None;
    }
    let text = keystroke.key_char.as_deref().unwrap_or(&keystroke.key);
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_control() && c != ' ' => Some(c),
        _ => None,
    }
}

/// The type-ahead buffer: what has been typed, and when.
#[derive(Debug, Default)]
pub(super) struct Typeahead {
    query: String,
    last: Option<Instant>,
}

impl Typeahead {
    /// Adds `c`, starting a fresh query if the last keystroke has gone
    /// stale, and returns the string to match rows against.
    pub(super) fn push(&mut self, c: char, now: Instant) -> &str {
        let stale = self
            .last
            .is_none_or(|last| now.duration_since(last) > TYPEAHEAD_TIMEOUT);
        if stale {
            self.query.clear();
        }
        self.last = Some(now);
        self.query.extend(c.to_lowercase());
        &self.query
    }
}

/// Where a type-ahead search lands, searching forward from `from` and
/// wrapping once.
///
/// Wrapping is right here where it is wrong for the arrows: type-ahead is a
/// search, and a search that stops at the bottom of the list has simply
/// failed to find something that is sitting above it.
pub(super) fn typeahead_target(labels: &[String], from: usize, query: &str) -> Option<usize> {
    if query.is_empty() || labels.is_empty() {
        return None;
    }

    // A repeated single character cycles through the matches rather than
    // sticking on the first one, which is what makes "p p p" walk the Parts.
    let start = if query.chars().count() == 1 {
        from + 1
    } else {
        from
    };

    (0..labels.len())
        .map(|offset| (start + offset) % labels.len())
        .find(|&index| labels[index].to_lowercase().starts_with(query))
}

/// The row a `Left` should move to: the nearest row above that is shallower
/// than this one. `None` at a root, where the pattern says to do nothing.
pub(super) fn parent_of(depths: &[usize], index: usize) -> Option<usize> {
    let depth = *depths.get(index)?;
    if depth == 0 {
        return None;
    }
    depths[..index]
        .iter()
        .rposition(|&candidate| candidate < depth)
}

impl Shell {
    /// One arrow, arriving as the action the toolkit's own keymap produced.
    /// Returns whether this module answered it; `false` means the toolkit's
    /// handler should run (and expand or collapse the focused node).
    pub(super) fn handle_tree_arrow(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some((len, focused, is_folder, expanded)) = self.tree_focus(cx) else {
            return false;
        };
        let Some(nav) = arrow(key, is_folder, expanded) else {
            return false;
        };
        self.apply_tree_nav(nav, len, focused, cx)
    }

    /// The focused row's index and shape, plus how many rows are visible.
    fn tree_focus(&self, cx: &Context<Self>) -> Option<(usize, usize, bool, bool)> {
        let tree = self.tree.read(cx);
        let len = visible_len(tree);
        if len == 0 {
            return None;
        }
        let focused = tree.selected_index().unwrap_or(0);
        let entry = tree.entry(focused);
        Some((
            len,
            focused,
            entry.is_some_and(|entry| entry.is_folder()),
            entry.is_some_and(|entry| entry.is_expanded()),
        ))
    }

    fn apply_tree_nav(
        &mut self,
        nav: Nav,
        len: usize,
        focused: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let target = match nav {
            Nav::LetToolkitExpandOrCollapse => return false,
            Nav::Nowhere => return true,
            // No wrapping, in either direction: a tree is a list of a
            // place, and teleporting from the last row to the first loses
            // the reader. (The toolkit's own handlers wrap, which is a
            // menu's behaviour.)
            Nav::Previous => focused.saturating_sub(1),
            Nav::Next => (focused + 1).min(len - 1),
            Nav::First => 0,
            Nav::Last => len - 1,
            Nav::Into => (focused + 1).min(len - 1),
            Nav::OutToParent => match parent_of(&self.tree_depths(cx), focused) {
                Some(parent) => parent,
                None => return true,
            },
        };
        let tree = self.tree.clone();
        self.focus_tree_row(&tree, target, cx);
        true
    }

    /// Runs the tree's keyboard contract for the keys that arrive as plain
    /// keystrokes rather than as actions: Home, End and type-ahead.
    pub(super) fn handle_tree_key(
        &mut self,
        keystroke: &Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((len, focused, is_folder, expanded)) = self.tree_focus(cx) else {
            return false;
        };

        if let Some(nav) = nav_for(keystroke, is_folder, expanded) {
            return self.apply_tree_nav(nav, len, focused, cx);
        }

        if let Some(c) = typeahead_char(keystroke) {
            let query = self.typeahead.push(c, Instant::now()).to_owned();
            let labels = self.tree_labels(cx);
            if let Some(target) = typeahead_target(&labels, focused, &query) {
                let tree = self.tree.clone();
                self.focus_tree_row(&tree, target, cx);
            }
            // Consumed either way: a search that found nothing has still
            // been typed *at the tree*, and letting it fall through would
            // hand a stray letter to whatever is behind it.
            return true;
        }

        false
    }

    fn focus_tree_row(&mut self, tree: &Entity<TreeState>, index: usize, cx: &mut Context<Self>) {
        tree.update(cx, |tree, cx| {
            tree.set_selected_index(Some(index), cx);
            tree.scroll_to_item(index, gpui_kit::ScrollStrategy::Center);
        });
        cx.notify();
    }

    fn tree_depths(&self, cx: &Context<Self>) -> Vec<usize> {
        let tree = self.tree.read(cx);
        (0..)
            .map_while(|index| tree.entry(index).map(|entry| entry.depth()))
            .collect()
    }

    fn tree_labels(&self, cx: &Context<Self>) -> Vec<String> {
        let tree = self.tree.read(cx);
        (0..)
            .map_while(|index| {
                tree.entry(index)
                    .map(|entry| entry.item().label.to_string())
            })
            .collect()
    }
}

fn visible_len(tree: &TreeState) -> usize {
    (0..)
        .take_while(|&index| tree.entry(index).is_some())
        .count()
}

#[cfg(test)]
#[path = "tree_keys/tests.rs"]
mod tests;

/// The four arrow actions the toolkit's keymap produces inside its `Tree`
/// context, wired so this module sees each one *before* the tree's own
/// handler does.
pub(super) fn intercept_arrows<E: gpui_kit::InteractiveElement>(
    element: E,
    cx: &mut Context<Shell>,
) -> E {
    element
        .capture_action(cx.listener(|shell, _: &SelectUp, _, cx| {
            if shell.handle_tree_arrow("up", cx) {
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(|shell, _: &SelectDown, _, cx| {
            if shell.handle_tree_arrow("down", cx) {
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(|shell, _: &SelectLeft, _, cx| {
            if shell.handle_tree_arrow("left", cx) {
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(|shell, _: &SelectRight, _, cx| {
            if shell.handle_tree_arrow("right", cx) {
                cx.stop_propagation();
            }
        }))
}
