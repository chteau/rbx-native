//! Roving tabindex: the keyboard model WAI-ARIA's Toolbar and Tabs patterns
//! ask for, and the one thing a dense editor cannot be usable without.
//!
//! The rule is that a composite widget — the ribbon, a tab strip — is
//! **one** stop in the window's Tab order, not one stop per button. Tab
//! moves between regions; arrows move inside one. Without it, reaching the
//! Explorer from the menu bar means pressing Tab past every ribbon button
//! on the current page, which is the kind of thing that makes people stop
//! using the keyboard entirely.
//!
//! GPUI has no helper for this (`tab_group` only nests the *ordering*;
//! every leaf inside a group is still its own stop, and `tab_index` is a
//! plain sort key with no `-1` escape). What it does have is exactly the
//! three primitives needed:
//!
//! - `tab_stop(false)` genuinely removes a focusable element from the Tab
//!   sequence while leaving it focusable programmatically;
//! - `focus_visible` styles an element only when focus arrived by keyboard,
//!   so the ring needs no modality tracking of its own;
//! - a focused element already turns Enter and Space into a real
//!   `on_click`, which *is* the APG's "manual activation" — no separate
//!   activation path to write, and no way for the two to drift apart.
//!
//! So: every item owns a focus handle, exactly one of them (the current
//! one) carries the group's `tab_index`, and the container translates
//! arrows into `Window::focus`. Re-entering with Tab lands on whichever
//! item was last current, because that is the one holding the index.

use std::cell::{Cell, RefCell};

use gpui_kit::{App, FocusHandle, InteractiveElement, Keystroke, Window};

gpui_kit::actions!(rbx_shell, [FocusNext, FocusPrev]);

/// The key context this window puts on its root, so the bindings below
/// out-rank the toolkit's own Tab binding.
pub(crate) const CONTEXT: &str = "RbxShell";

/// Takes Tab and Shift+Tab off the toolkit and gives them to
/// [`TabOrder::step`].
///
/// A binding, not a key listener, because the toolkit's `Root` binds Tab as
/// an **action** — and GPUI resolves a keystroke to an action *before* it
/// runs any key listener, so a `capture_key_down` on this window's root
/// never saw the key at all. Matching on a deeper key context is the way to
/// win that, and it is why the root element carries [`CONTEXT`].
pub(crate) fn install(cx: &mut gpui_kit::App) {
    cx.bind_keys([
        gpui_kit::KeyBinding::new("tab", FocusNext, Some(CONTEXT)),
        gpui_kit::KeyBinding::new("shift-tab", FocusPrev, Some(CONTEXT)),
    ]);
}

/// The window's Tab order, as one running counter.
///
/// Two GPUI limitations force this shape, both established by driving the
/// real window rather than by reading the code:
///
/// - **`tab_group` swallows its children.** An element inside a group
///   registers no tab stop at all, so a dock wrapped in one is simply
///   unreachable. The whole region-band idea went with it.
/// - **Two stops with the same `tab_index` do not advance.** Tab reaches
///   the first of them and stays there — a keyboard trap, and the reason
///   the three window buttons could swallow focus entirely.
///
/// So every stop gets its own number, handed out in paint order, which is
/// reading order, which is the order WCAG 2.4.3 asks for anyway. `Shell`
/// resets the counter at the top of each render.
#[derive(Default)]
pub(crate) struct TabOrder {
    /// The next index to hand out, for the toolkit widgets that take one.
    next: Cell<isize>,
    /// Stable handles, by position in paint order, for the controls this
    /// module focuses directly.
    pool: RefCell<Vec<FocusHandle>>,
    cursor: Cell<usize>,
    /// This render's stops, in paint order. Rebuilt every frame, because
    /// what exists depends on the open ribbon page and the selection.
    order: RefCell<Vec<FocusHandle>>,
    /// Where the walk last left off.
    ///
    /// Some stops hand focus straight on to something else — the Explorer's
    /// door focuses the tree inside it, a field's wrapper focuses the
    /// field — so by the next keystroke nothing in `order` holds focus any
    /// more. Without this the walk would restart from the top every time it
    /// passed one of those, and the Explorer would be a wall.
    last: Cell<usize>,
}

impl TabOrder {
    /// Called once at the top of each render.
    pub(crate) fn restart(&self) {
        self.next.set(1);
        self.cursor.set(0);
        self.order.borrow_mut().clear();
    }

    /// An index for a toolkit widget that exposes `tab_index` but not its
    /// handle. Numbered from 1: 0 is what every widget that never asked
    /// for one already carries.
    pub(crate) fn next(&self) -> isize {
        let index = self.next.get();
        self.next.set(index + 1);
        index
    }

    /// A handle for one of this app's own controls, placed at this point in
    /// the order. Stable across renders by position, so focus survives a
    /// repaint.
    pub(crate) fn claim(&self, cx: &mut App) -> FocusHandle {
        let position = self.cursor.get();
        self.cursor.set(position + 1);

        let mut pool = self.pool.borrow_mut();
        while pool.len() <= position {
            pool.push(cx.focus_handle());
        }
        let handle = pool[position].clone();
        self.order.borrow_mut().push(handle.clone());
        handle
    }

    /// Records a handle that belongs to somebody else (a roving group's
    /// current item) at this point in the order.
    pub(crate) fn register(&self, handle: &FocusHandle) {
        self.order.borrow_mut().push(handle.clone());
    }

    /// Moves focus one stop, wrapping. Returns whether there was anywhere
    /// to go.
    ///
    /// This walks **this** list rather than calling `Window::focus_next`,
    /// and the reason is not preference. GPUI's tab order cannot escape a
    /// group of stops that share an index — and every toolkit control that
    /// never asked for one sits at 0, so focus that wanders in stays there
    /// (measured: 49 consecutive presses, both directions). Nor can that be
    /// papered over by calling `focus_next` in a loop and checking where it
    /// landed: GPUI *defers* a focus change to the end of the frame, so the
    /// check reads the previous value every time.
    pub(crate) fn step(&self, backwards: bool, window: &mut Window, cx: &mut App) -> bool {
        let order = self.order.borrow();
        if order.is_empty() {
            return false;
        }

        let at = order
            .iter()
            .position(|handle| handle.is_focused(window))
            .or_else(|| {
                let last = self.last.get();
                (last < order.len()).then_some(last)
            });
        let target = match (at, backwards) {
            (Some(0), true) => order.len() - 1,
            (Some(at), true) => at - 1,
            (Some(at), false) => (at + 1) % order.len(),
            // Nothing this app placed has held focus yet: enter from the
            // top going forward, from the bottom going back.
            (None, true) => order.len() - 1,
            (None, false) => 0,
        };
        self.last.set(target);
        order[target].clone().focus(window, cx);
        true
    }
}

/// What a keystroke asked a roving group to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Move {
    Previous,
    Next,
    First,
    Last,
}

impl Move {
    fn of(keystroke: &Keystroke, vertical: bool) -> Option<Move> {
        if keystroke.modifiers.modified() {
            return None;
        }
        match keystroke.key.as_str() {
            "left" if !vertical => Some(Move::Previous),
            "right" if !vertical => Some(Move::Next),
            "up" if vertical => Some(Move::Previous),
            "down" if vertical => Some(Move::Next),
            "home" => Some(Move::First),
            "end" => Some(Move::Last),
            _ => None,
        }
    }

    /// Where `current` lands, given `len` items.
    ///
    /// Previous/Next **wrap**, which the APG marks optional for toolbars
    /// and tab lists — taken here because these strips are short and
    /// circular, so running off the end and stopping reads as the key
    /// having failed rather than as a boundary.
    fn apply(self, current: usize, len: usize) -> usize {
        let last = len.saturating_sub(1);
        match self {
            Move::Previous if current == 0 => last,
            Move::Previous => current - 1,
            Move::Next if current >= last => 0,
            Move::Next => current + 1,
            Move::First => 0,
            Move::Last => last,
        }
    }
}

/// One composite widget's keyboard state.
///
/// The interior mutability is deliberate. Handles have to be created while
/// the element tree is being built, and the builders that create them run
/// behind `&Shell` — a render that took `&mut Shell` could not also hand
/// `&Shell` to `menu::dropdown` in the same expression. Nothing here
/// escapes a single render, so the `RefCell` is never held across a call
/// that could re-enter it.
pub(crate) struct Roving {
    handles: RefCell<Vec<FocusHandle>>,
    current: Cell<usize>,
    /// How many items the last render actually built, so an arrow key can
    /// wrap against the real count rather than against however many handles
    /// have ever been created (a ribbon page with fewer buttons than the
    /// one before it would otherwise let focus walk off the end).
    len: Cell<usize>,
    /// Hands out indices in build order during a render.
    cursor: Cell<usize>,
    /// The single index this whole group occupies, taken from the
    /// window's [`TabOrder`] once per render.
    band: Cell<isize>,
    vertical: bool,
}

impl Roving {
    pub(crate) fn horizontal() -> Self {
        Roving::new(false)
    }

    /// A stacked group — a property list — where Up/Down move and the
    /// horizontal arrows are left to whatever owns them.
    pub(crate) fn vertical() -> Self {
        Roving::new(true)
    }

    fn new(vertical: bool) -> Self {
        Roving {
            handles: RefCell::new(Vec::new()),
            current: Cell::new(0),
            len: Cell::new(0),
            cursor: Cell::new(0),
            band: Cell::new(0),
            vertical,
        }
    }

    /// Call once per render, before building the items. A group whose size
    /// is known up front passes it; the ribbon, whose control count depends
    /// on the open page, passes `None` and calls [`Roving::finish`] once the
    /// page is built.
    pub(crate) fn begin(&self, order: &TabOrder, len: Option<usize>, cx: &mut App) {
        self.band.set(order.next());
        if let Some(len) = len {
            self.set_len(len);
        }
        // The group is one stop in the window's order: whichever item is
        // current stands for the whole of it.
        order.register(&self.handle(self.current.get(), cx));
        self.cursor.set(0);
    }

    /// Closes a `begin(None)` render: however many items asked for an index
    /// is how many there are.
    pub(crate) fn finish(&self) {
        self.set_len(self.cursor.get());
    }

    fn set_len(&self, len: usize) {
        self.len.set(len);
        if self.current.get() >= len {
            self.current.set(len.saturating_sub(1));
        }
    }

    /// The next index in build order — for a group whose items are built
    /// across several functions and can't conveniently be enumerated.
    pub(crate) fn claim<E: InteractiveElement>(&self, element: E, cx: &mut App) -> E {
        let index = self.cursor.get();
        self.cursor.set(index + 1);
        self.item(index, element, cx)
    }

    fn handle(&self, index: usize, cx: &mut App) -> FocusHandle {
        let mut handles = self.handles.borrow_mut();
        while handles.len() <= index {
            handles.push(cx.focus_handle());
        }
        handles[index].clone()
    }

    /// Wires one item into the group: focusable, but a Tab stop only when
    /// it is the current one.
    ///
    /// The index and the stop flag go on the **focus handle**, not on the
    /// element. `InteractiveElement::tab_index` and `tab_stop` only reach a
    /// handle that GPUI creates for itself, and an element with
    /// `track_focus` already has one — so setting them on the element is
    /// silently ignored, and every item ends up a stop at index 0. (That is
    /// not obvious from either method's documentation; it is
    /// `Interactivity::layout`'s `tracked_focus_handle.is_none()` guard.)
    ///
    /// `tab_stop(false)` is also the *only* way to take an item out of the
    /// sequence: GPUI has no `tabindex="-1"`, and a negative index just
    /// sorts earlier.
    ///
    /// The handle's builder methods return a modified clone that keeps the
    /// same `FocusId`, so focusing the stored handle and reading
    /// `is_focused` still refer to the same element.
    pub(crate) fn item<E: InteractiveElement>(&self, index: usize, element: E, cx: &mut App) -> E {
        let current = index == self.current.get();
        let handle = self
            .handle(index, cx)
            .tab_index(self.band.get())
            .tab_stop(current);

        element.track_focus(&handle)
    }

    /// Handles an arrow/Home/End keystroke, moving focus within the group.
    /// Returns whether it consumed the key.
    ///
    /// Enter and Space are deliberately **not** here: GPUI already turns
    /// them into an `on_click` on the focused element, so activation is the
    /// same code path a mouse click takes. Two implementations of "what
    /// this button does" is how a keyboard path silently rots.
    pub(crate) fn key(&self, keystroke: &Keystroke, window: &mut Window, cx: &mut App) -> bool {
        let len = self.len.get();
        if len == 0 {
            return false;
        }
        let Some(movement) = Move::of(keystroke, self.vertical) else {
            return false;
        };

        let next = movement.apply(self.current.get(), len);
        self.current.set(next);
        self.handle(next, cx).focus(window, cx);
        true
    }
}

#[cfg(test)]
#[path = "roving/tests.rs"]
mod tests;
