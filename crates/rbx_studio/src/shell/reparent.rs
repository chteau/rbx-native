//! Drag-and-drop reparenting in the Explorer.
//!
//! GPUI performs the gesture itself: `on_drag` on the row a drag starts from,
//! `can_drop`/`on_drop` on the row under the cursor, `drag_over` for the
//! highlight, and a ghost it paints at the cursor every frame. What lives here
//! is the two halves it cannot know about — what a dragged row carries, and
//! what a drop does to the DOM. Which drops are legal at all is
//! `explorer::reparent`'s question, asked from both `can_drop` (so an illegal
//! target never lights up) and the drop itself (so nothing else can slip
//! through, and no undo step is pushed for a move that moves nothing).
//!
//! There is deliberately no "drop between two rows" here: real Studio has
//! none either, because a place has no user-orderable sibling order to
//! rearrange — see `explorer::reparent`'s own module comment.

use gpui_kit::component::list::ListItem;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::explorer::reparent;

use super::Shell;

/// What a dragged Explorer row hands to GPUI, and what a drop target reads
/// back out of it.
///
/// A set rather than one referent: dragging a row that belongs to the
/// selection drags the whole selection, which is what creator-docs describes
/// ("to change the parent of **one or more** children (reparent), simply drag
/// and drop them onto the new parent").
#[derive(Clone)]
pub(super) struct DraggedInstances {
    references: Vec<Ref>,
    label: SharedString,
}

impl DraggedInstances {
    /// What dragging the row for `reference` picks up, given what is currently
    /// selected. A row outside the selection drags only itself — pressing it
    /// replaced the selection anyway, and carrying the old one would move
    /// instances the user never touched.
    pub(super) fn new(selected: &[Ref], reference: Ref, name: &SharedString) -> Self {
        let references = if selected.contains(&reference) {
            selected.to_vec()
        } else {
            vec![reference]
        };
        let label = match references.len() {
            0 | 1 => name.clone(),
            count => SharedString::from(format!("{count} instances")),
        };

        DraggedInstances { references, label }
    }
}

/// The ghost that follows the cursor while a drag is in flight. GPUI paints it
/// at the cursor itself, so this only has to say what it looks like.
pub(super) struct DragPreview {
    label: SharedString,
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_1p5()
            .py_0p5()
            .rounded_sm()
            .text_xs()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().accent)
            .text_color(cx.theme().accent_foreground)
            .child(self.label.clone())
    }
}

/// One Explorer row, wrapped in everything the drag gesture needs: it can be
/// picked up, and it can be dropped onto.
///
/// The wrapper carries its own `id` because `on_drag` is only offered on a
/// stateful element, and GPUI needs somewhere to keep the pending mouse-down
/// a drag grows out of. It sits *inside* the tree widget's own row div, whose
/// mouse-down already selects — so pressing a row still selects it, and only
/// a press that then moves becomes a drag.
pub(super) fn draggable_row(
    shell: &Entity<Shell>,
    index: usize,
    target: Ref,
    dragged: DraggedInstances,
    row: ListItem,
) -> AnyElement {
    div()
        .id(("explorer-drag", index))
        .on_drag(dragged, |dragged, _, _, cx| {
            let label = dragged.label.clone();
            cx.new(|_| DragPreview { label })
        })
        // Gates the highlight below as well as the drop itself (GPUI skips
        // every `drag_over` style when this says no), so an illegal target —
        // the dragged instance itself, its own subtree, the parent it already
        // has — simply never lights up.
        .can_drop({
            let shell = shell.clone();
            move |payload, _, cx| {
                payload
                    .downcast_ref::<DraggedInstances>()
                    .is_some_and(|dragged| shell.read(cx).accepts_drop(dragged, target))
            }
        })
        .drag_over::<DraggedInstances>(|style, _, _, cx| style.bg(cx.theme().drop_target))
        .on_drop({
            let shell = shell.clone();
            move |dragged: &DraggedInstances, _, cx| {
                let dragged = dragged.clone();
                shell.update(cx, |shell, cx| {
                    shell.reparent_dropped(&dragged, target, cx);
                });
            }
        })
        .child(row)
        .into_any_element()
}

impl Shell {
    /// Whether dropping `dragged` onto `target` would move anything — asked
    /// once per visible row per frame while a drag is in flight, which is why
    /// `explorer::reparent` walks parents rather than subtrees.
    pub(super) fn accepts_drop(&self, dragged: &DraggedInstances, target: Ref) -> bool {
        reparent::accepts(&self.dom, &self.database, &dragged.references, target)
    }

    /// Reparents whatever the drop legitimately moves, as one undo step,
    /// through the same take/put-back path `shell::keys`' insert and delete
    /// use.
    pub(super) fn reparent_dropped(
        &mut self,
        dragged: &DraggedInstances,
        target: Ref,
        cx: &mut Context<Self>,
    ) {
        let moving = reparent::movable(&self.dom, &self.database, &dragged.references, target);
        // `can_drop` should already have refused this, but a drop that moves
        // nothing must not push an undo step that undoes nothing.
        if moving.is_empty() {
            return;
        }

        // See `shell::history`: snapshotted before the moves below, so one
        // drag is one Ctrl+Z however many instances it carried.
        self.push_history();
        for reference in &moving {
            self.dom.set_parent(*reference, Some(target));
        }

        self.rebuild_explorer(cx);
        self.reveal_moved(&moving, cx);
        match moving[..] {
            // The same cheap path a script's `part.Parent = model` already
            // takes (see `shell::edit::reflect_in_viewport`): only a move that
            // actually crossed the Workspace boundary costs a scene rebuild.
            [only] => self.reflect_in_viewport(only, "Parent", cx),
            _ => self.reload_viewport(cx),
        }
        cx.notify();
    }

    /// Expands the branch the instances landed in and scrolls it into view.
    /// Dropping into a collapsed parent otherwise reads as a delete: the rows
    /// leave where they were and appear nowhere.
    fn reveal_moved(&mut self, moving: &[Ref], cx: &mut Context<Self>) {
        let Some(item) = moving
            .first()
            .and_then(|reference| self.explorer.item(*reference))
        else {
            return;
        };

        let tree = self.tree.clone();
        tree.update(cx, |tree, cx| {
            tree.reveal_item(&item.id, ScrollStrategy::Center, cx);
        });
    }
}

#[cfg(test)]
#[path = "reparent/tests.rs"]
mod tests;
