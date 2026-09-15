//! Running a Command Bar script: swapping the canonical DOM into a throwaway
//! `rbx_lua::Runtime` and reflecting whatever it changed in the Explorer and
//! the viewport.

use std::rc::Rc;

use gpui_kit::{Context, ScrollStrategy};
use rbx_dom::{Ref, WeakDom};

use crate::command_bar::{self, Feedback};
use crate::explorer::{self, Explorer};

use super::Shell;

impl Shell {
    /// Selects `name`'s first match in the current DOM the way clicking its
    /// row would, expanding whatever ancestors were collapsed — the only way
    /// a screenshot can show an instance `RBX_STUDIO_RUN` just created, since
    /// nothing else can click the tree on the editor's behalf.
    pub(super) fn select_by_name(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(reference) = explorer::find_by_name(&self.dom, name) else {
            return;
        };
        self.select(reference, cx);
    }

    /// Selects `reference` in the Explorer the way clicking its row would,
    /// expanding whatever ancestors were collapsed. Shared with `shell::keys`,
    /// which selects a freshly inserted instance the same way.
    pub(super) fn select(&mut self, reference: Ref, cx: &mut Context<Self>) {
        let Some(item) = self.explorer.item(reference) else {
            return;
        };
        // `set_selected_item` notifies the `clicked` observer, but only once
        // GPUI gets around to flushing it — soon enough after a real click,
        // but never before this same call chain returns when it runs here,
        // synchronously, from a debug var applied during `Shell::new`. Calling
        // `sync_selection` directly is what makes `self.selection`, the
        // Properties panel and the viewport outline agree with the row
        // immediately, exactly as they will again once the deferred observer
        // eventually runs (a no-op by then, the selection already matching).
        let tree = self.tree.clone();
        tree.update(cx, |tree, cx| {
            tree.set_selected_item(Some(&item), cx);
            tree.reveal_item(&item.id, ScrollStrategy::Center, cx);
        });
        self.sync_selection(&tree, cx);
    }

    /// Clears the Explorer selection, synchronously (see [`Shell::select`]'s
    /// doc comment) — `shell::keys`' delete key, once the row it removed was
    /// the whole selection, leaves nothing behind to keep selected.
    pub(super) fn deselect(&mut self, cx: &mut Context<Self>) {
        let tree = self.tree.clone();
        tree.update(cx, |tree, cx| tree.set_selected_item(None, cx));
        self.sync_selection(&tree, cx);
    }

    /// Enter in the Command Bar: reads the input's current text and runs it.
    pub(super) fn run_typed_command(&mut self, cx: &mut Context<Self>) {
        let source = self.command_bar.input().read(cx).value().to_string();
        self.run_command(&source, cx);
    }

    /// Runs `source` against the place, the way the Command Bar always does:
    /// the DOM is swapped into a throwaway `rbx_lua::Runtime` (which has to own
    /// it while the script runs) and swapped back out, mutated or not, once it
    /// returns.
    pub(super) fn run_command(&mut self, source: &str, cx: &mut Context<Self>) {
        // Snapshotted before the swap below, whether or not the script ends
        // up mutating anything — see `shell::history`.
        self.push_history();
        // `WeakDom::new()` is only ever seen back if `run` itself could not be
        // built (an engine fault, not a script one) — see `command_bar::run`.
        let dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let (dom, result) = command_bar::run(dom, &self.database, source);
        self.dom = dom;

        let succeeded = result.is_ok();
        let feedback = Feedback::from_run(result);
        // Logged before the Command Bar's own label is overwritten below: the
        // Output panel is the only place a run's outcome survives past the
        // next one (see `shell::output`).
        self.output.push(source, feedback.clone());
        self.command_bar.set_feedback(feedback);
        if succeeded {
            self.rebuild_after_script(cx);
        }
        cx.notify();
    }

    /// Reflects a successful script in everything that was built from the old
    /// DOM: the Explorer's rows and the viewport's scene. The Properties panel
    /// reads `self.dom` itself, so the re-render is all it needs.
    fn rebuild_after_script(&mut self, cx: &mut Context<Self>) {
        self.rebuild_explorer(cx);
        self.reload_viewport(cx);
    }

    /// Rebuilds the Explorer's rows from the current `self.dom`, keeping the
    /// selection when its referent still resolves. Shared with the
    /// Properties panel's own edit path (see `shell::edit`), since renaming
    /// an instance moves its row the same way a script's rename would.
    pub(super) fn rebuild_explorer(&mut self, cx: &mut Context<Self>) {
        let explorer = Explorer::from_dom(&self.dom, self.icons.as_ref());
        let items = explorer.items(self.show_all_services);
        // A rename, reparent or destroy can invalidate the selection; kept
        // only if its referent still resolves in the rebuilt tree.
        let kept = self
            .selected()
            .filter(|reference| self.dom.get(*reference).is_some());
        let preselected = kept.and_then(|reference| explorer.item(reference));

        self.explorer = Rc::new(explorer);
        self.tree.update(cx, |tree, cx| {
            tree.set_items(items, cx);
            tree.set_selected_item(preselected.as_ref(), cx);
        });
    }

    /// Reflects `self.dom` in the 3D view. Cheap enough to call after every
    /// committed edit, not just a script run: `Headless::reload` is what a
    /// single-property change needs too.
    pub(super) fn reload_viewport(&mut self, cx: &mut Context<Self>) {
        let dom = self.dom.clone();
        self.viewport.update(cx, |viewport, _| viewport.reload(dom));
    }
}
