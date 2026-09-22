//! Editing the place from an Explorer row: the `+` insert picker, the
//! right-click context menu, and renaming a row in place.
//!
//! Split into three submodules, each well inside this file's own ~400-line
//! budget: [`picker`] (the class list the `+` opens, and the two insertion
//! preferences beside its search field), [`menu`] (the right-click menu) and
//! [`rename`] (the in-place name box). What lives here is only what all
//! three share — the state they keep between frames, and where the two
//! popups anchor.
//!
//! Both popups are painted from `Render for Shell` rather than from inside
//! the Explorer's own tree, for the reason `shell::menu` gives: a popup
//! nested in a scrolled, virtualised list is clipped by it. They are
//! *controlled* the same way a dropdown is — which one is open lives here,
//! not inside the element — so opening either closes the other and Escape
//! closes both.

use gpui_kit::component::input::InputState;
use gpui_kit::*;
use rbx_dom::Ref;

use super::Shell;

pub(super) mod menu;
pub(super) mod picker;
pub(super) mod rename;

#[cfg(test)]
#[path = "explorer_edit/tests.rs"]
mod tests;

/// Everything the Explorer's row affordances keep between frames.
#[derive(Default)]
pub(super) struct ExplorerEdit {
    /// The row the pointer is over. The `+` draws on that row alone: one on
    /// every row of a place with a thousand parts is a column of identical
    /// glyphs competing with the names it sits beside.
    hovered: Option<Ref>,
    picker: Option<picker::Picker>,
    menu: Option<menu::RowMenu>,
    renaming: Option<rename::Renaming>,
    /// Where the pointer last was, in window coordinates. Both popups anchor
    /// here — including when a keystroke rather than a click opened one,
    /// which is the whole reason this is tracked rather than read off the
    /// event that opened it.
    pointer: Point<Pixels>,
    /// A box to put the caret in on the next frame — see
    /// [`Shell::focus_explorer_edit`].
    focus_next: Option<Entity<InputState>>,
    /// What Change Class last turned something into, most recent first —
    /// suggested again next time. This session's only: a class picked in
    /// one place says little about the next.
    pub(super) recent_classes: Vec<String>,
}

impl Shell {
    /// Records the pointer for [`ExplorerEdit::pointer`]. Deliberately does
    /// not notify: this runs on every mouse move the window sees, and a
    /// repaint per pixel to record a position nothing is currently drawing
    /// from would cost a frame each time.
    pub(super) fn note_pointer(&mut self, position: Point<Pixels>) {
        self.explorer_edit.pointer = position;
    }

    /// Puts the caret in the picker's search box or the name box that was
    /// just opened. Deferred to the frame that draws it, rather than done
    /// when it was created: GPUI blurs a focus handle whose element is not
    /// in the rendered tree, so focusing a box that does not exist yet is
    /// undone before the caret ever appears — and the blur that undoes it
    /// would take the rename with it.
    pub(super) fn focus_explorer_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.explorer_edit.focus_next.take() else {
            return;
        };
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            // A rename is almost always a new name rather than an edit to
            // the old one, so the first keystroke should replace it. The
            // picker's own box is empty, where this does nothing.
            state.select_all(window, cx);
        });
    }

    /// Whether a name box is open in the tree. While it is, the Explorer's
    /// own keyboard contract has to stand aside: its type-ahead would eat
    /// every letter typed into the box, its arrows would move the selection
    /// instead of the caret, and Delete would remove the instance being
    /// renamed rather than a character.
    pub(in crate::shell) fn renaming_in_place(&self) -> bool {
        self.explorer_edit.renaming.is_some()
    }

    /// What the Explorer's per-row closure needs from this state, read once
    /// per render. The closure runs inside the tree's own layout pass and
    /// cannot borrow the shell back out of it, so it carries this instead —
    /// two referents and an `Entity`, all cheap to clone.
    pub(super) fn row_slots(&self) -> RowSlots {
        RowSlots {
            hovered: self.explorer_edit.hovered,
            renaming: self
                .explorer_edit
                .renaming
                .as_ref()
                .map(|renaming| (renaming.target(), renaming.input().clone())),
        }
    }

    /// GPUI reports a row's hover from the row itself, so "the pointer left
    /// the tree entirely" arrives as the last hovered row reporting `false`
    /// — clearing on any other row's `false` would fight whichever row the
    /// pointer moved *onto*.
    pub(super) fn hover_row(&mut self, reference: Ref, hovered: bool, cx: &mut Context<Self>) {
        let next = if hovered {
            Some(reference)
        } else if self.explorer_edit.hovered == Some(reference) {
            None
        } else {
            return;
        };
        if self.explorer_edit.hovered != next {
            self.explorer_edit.hovered = next;
            cx.notify();
        }
    }

    /// The picker and the context menu, painted over the whole window from
    /// `Render for Shell`.
    pub(super) fn explorer_popups(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        [self.picker_popup(cx), self.row_menu_popup(cx)]
            .into_iter()
            .flatten()
            .collect()
    }

    /// Escape's half of "no keyboard traps" for these two popups (WCAG
    /// 2.1.2), plus the cancel half of an in-place rename. Returns whether
    /// anything was actually closed, so the caller can skip the repaint when
    /// nothing was.
    pub(super) fn close_explorer_popups(&mut self) -> bool {
        self.explorer_edit.picker.take().is_some()
            | self.explorer_edit.menu.take().is_some()
            | self.explorer_edit.renaming.take().is_some()
    }

    /// Where a popup opened from the Explorer goes. Anchored to the pointer
    /// rather than to the row: a row is 28px tall inside a virtualised list
    /// that offers no geometry to anchor to, and the pointer is where the
    /// gesture that opened it happened anyway.
    fn popup_anchor(&self) -> Point<Pixels> {
        self.explorer_edit.pointer
    }
}

/// The Explorer's per-row state, snapshotted for one render — see
/// [`Shell::row_slots`].
#[derive(Clone)]
pub(super) struct RowSlots {
    hovered: Option<Ref>,
    renaming: Option<(Ref, Entity<InputState>)>,
}

impl RowSlots {
    /// What one row draws in place of its label, and what it carries at its
    /// right edge: the name box while it is being renamed, the `+` while the
    /// pointer is over it. Never both — a row mid-rename has the caret in
    /// it, and an insert button beside a name being typed is a target for a
    /// mis-click, not an affordance.
    pub(super) fn of(&self, shell: &Entity<Shell>, reference: Ref) -> RowWidgets {
        let name = self
            .renaming
            .as_ref()
            .filter(|(target, _)| *target == reference)
            .map(|(_, input)| rename::name_box(input));
        let trailing = (self.hovered == Some(reference) && name.is_none())
            .then(|| picker::insert_button(shell, reference));
        RowWidgets { name, trailing }
    }
}

/// The two widgets an Explorer row cannot build for itself — see
/// [`RowSlots::of`].
#[derive(Default)]
pub(super) struct RowWidgets {
    pub(super) name: Option<AnyElement>,
    pub(super) trailing: Option<AnyElement>,
}
