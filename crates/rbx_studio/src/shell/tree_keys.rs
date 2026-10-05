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
//! The Explorer multi-selects, so it also takes the APG's *alternative*
//! multi-select model, the one that does not need modifier-free toggling:
//! a plain arrow moves focus and selects, `Shift` extends, `Ctrl`+Up/Down
//! moves focus alone and `Ctrl`+Space toggles the focused row. Roblox
//! Studio's own Explorer documents no keyboard focus apart from its
//! selection (creator-docs `studio/explorer.md` lists only Left/Right), so
//! the APG is the reference here. The cursor is the toolkit tree's
//! `focused_index`; the selection is `Shell::selection`.
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

use std::time::Instant;

use gpui_kit::base::actions::{SelectDown, SelectLeft, SelectRight, SelectUp};
use gpui_kit::component::tree::TreeState;
use gpui_kit::{Context, Entity, Keystroke};

use super::Shell;

mod typeahead;
pub(super) use typeahead::Typeahead;
use typeahead::{typeahead_char, typeahead_target};

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

/// `Shift` with Up, Down, Home or End: the move that, instead of selecting
/// the row it lands on, selects the range from the anchor to it (see
/// [`Shell::extend_tree_range`]). Any other modifier with it is not the
/// tree's.
pub(super) fn range_nav_for(keystroke: &Keystroke) -> Option<Nav> {
    let modifiers = keystroke.modifiers;
    if !modifiers.shift || modifiers.control || modifiers.alt || modifiers.platform {
        return None;
    }
    match keystroke.key.as_str() {
        "up" => Some(Nav::Previous),
        "down" => Some(Nav::Next),
        "home" => Some(Nav::First),
        "end" => Some(Nav::Last),
        _ => None,
    }
}

/// What `Ctrl` (or `Cmd`) does with a key in the tree, by the APG's
/// alternative multi-select model: Up/Down move the cursor without touching
/// the selection, Space toggles the row under it. `Shift` with it is a
/// different command (Ctrl+Shift+click adds a range), not this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FocusKey {
    Move(Nav),
    Toggle,
}

pub(super) fn focus_key_for(keystroke: &Keystroke) -> Option<FocusKey> {
    let modifiers = keystroke.modifiers;
    if !(modifiers.control || modifiers.platform) || modifiers.shift || modifiers.alt {
        return None;
    }
    match keystroke.key.as_str() {
        "up" => Some(FocusKey::Move(Nav::Previous)),
        "down" => Some(FocusKey::Move(Nav::Next)),
        "space" => Some(FocusKey::Toggle),
        _ => None,
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
        // See `Shell::renaming_in_place`: the open name box owns the arrows.
        if self.renaming_in_place() {
            return false;
        }
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
        let focused = tree.focused_index().unwrap_or(0);
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
            Nav::Previous | Nav::Next | Nav::First | Nav::Last | Nav::Into => {
                step(nav, len, focused)
            }
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
        // See `Shell::renaming_in_place`: without this the type-ahead below
        // swallows every letter typed into an open name box.
        if self.renaming_in_place() {
            return false;
        }
        if super::explorer_edit::menu::opens_row_menu(keystroke) {
            return self.open_row_menu_from_keyboard(cx);
        }
        let Some((len, focused, is_folder, expanded)) = self.tree_focus(cx) else {
            return false;
        };

        match focus_key_for(keystroke) {
            Some(FocusKey::Move(nav)) => {
                let target = step(nav, len, focused);
                self.tree.update(cx, |tree, cx| {
                    tree.set_focused_index(Some(target), cx);
                    tree.scroll_to_item(target, gpui_kit::ScrollStrategy::Center);
                });
                return true;
            }
            Some(FocusKey::Toggle) => {
                if let Some(&reference) = self.visible_rows(cx).get(focused) {
                    self.extend_selection(reference, cx);
                    self.range_anchor = Some(reference);
                }
                return true;
            }
            None => {}
        }
        if let Some(nav) = range_nav_for(keystroke) {
            self.extend_tree_range(nav, len, focused, cx);
            return true;
        }
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

    /// `Shift`+Up/Down/Home/End: moves the tree's cursor, then selects every
    /// visible row from the range anchor to it, as a `Shift`-click there
    /// would (see `selection::range`). The anchor stays put and stays the
    /// tree's selected row; Properties and the gizmo stay on it.
    fn extend_tree_range(&mut self, nav: Nav, len: usize, focused: usize, cx: &mut Context<Self>) {
        let target = step(nav, len, focused);
        let visible = self.visible_rows(cx);
        let (Some(&from), Some(&to)) = (visible.get(focused), visible.get(target)) else {
            return;
        };
        let anchor = super::selection::range_anchor(
            &visible,
            [self.range_anchor, self.selection.get()],
            from,
        );
        let range = super::selection::range(&visible, anchor, to);
        let first = range.first().and_then(|&r| self.explorer.item(r));
        let changed = self.selection.replace(range);
        self.tree.update(cx, |tree, cx| {
            tree.set_selected_item(first.as_ref(), cx);
            tree.set_focused_index(Some(target), cx);
            tree.scroll_to_item(target, gpui_kit::ScrollStrategy::Center);
        });
        if changed {
            self.selection_changed(cx);
        }
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

/// Where a vertical move lands, never wrapping (see `apply_tree_nav`).
fn step(nav: Nav, len: usize, focused: usize) -> usize {
    match nav {
        Nav::Previous => focused.saturating_sub(1),
        Nav::First => 0,
        Nav::Last => len - 1,
        _ => (focused + 1).min(len - 1),
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
