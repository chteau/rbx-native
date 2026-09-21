//! The Shell side of the script editor: opening a tab, getting its text into
//! the DOM, and keeping every open tab honest about what the DOM holds.
//!
//! A tab's text reaches `Source` through `script_editor::source::write`, which
//! ends at the same `WeakDom::set_property` every other property edit does,
//! with a `Shell::push_history` snapshot taken immediately before it exactly
//! as `shell::edit` takes one — so undo, redo and Ctrl+S need to know nothing
//! about scripts.

use std::time::Duration;

use gpui_kit::component::input::{EditorState, InputEvent};
use gpui_kit::*;
use rbx_dom::Ref;

use crate::explorer;
use crate::script_editor::tabs::Opened;
use crate::script_editor::{highlight, source, OpenScript};

use super::Shell;

/// Read once at startup by `Shell::new`; documented in `main`'s module doc
/// comment alongside the other debug aids.
pub(crate) const OPEN_VARIABLE: &str = "RBX_STUDIO_OPEN_SCRIPT";

/// How long typing must pause before a tab's text is written to the DOM.
///
/// Long enough that a run of typing lands as one undo step rather than one
/// per keystroke — Studio coalesces a typing run into a single waypoint the
/// same way, and at one snapshot per keystroke a 50-deep history would hold
/// about a sentence. Short enough to be imperceptible. Correctness never
/// rides on it: everything that needs an up-to-date DOM (Ctrl+S, undo/redo,
/// closing a tab) flushes first, so this only sets undo granularity.
const COMMIT_DELAY: Duration = Duration::from_millis(400);

impl Shell {
    /// Opens `reference` in the script editor, or brings its existing tab to
    /// the front. Anything that is not a script is ignored, so this is safe to
    /// call for any double-clicked Explorer row.
    pub(crate) fn open_script(
        &mut self,
        reference: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !source::is_script(&self.dom, &self.database, reference) {
            return;
        }

        // An already-open script keeps its editor untouched — re-seeding it
        // would throw away an edit in progress.
        if self.scripts.tabs.open(reference) == Opened::Existing {
            self.focus_script(reference, window, cx);
            self.document = super::chrome::Document::Scripts;
            cx.notify();
            return;
        }

        let text = source::read(&self.dom, reference).unwrap_or_default();
        let seed = text.clone();
        let state = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .language(highlight::LANGUAGE)
                // Folding needs a parse tree's block extents, which the lexer
                // this editor highlights with does not produce.
                .folding(false)
                .default_value(seed);
            state.set_highlighter_factory(highlight::factory(), cx);
            state
        });
        let subscription = cx.subscribe(&state, move |shell, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                shell.script_changed(reference, cx);
            }
        });

        self.scripts.open.insert(
            reference,
            OpenScript {
                state,
                synced: text,
                pending: false,
                generation: 0,
                _subscription: subscription,
            },
        );
        self.focus_script(reference, window, cx);
        self.document = super::chrome::Document::Scripts;
        cx.notify();
    }

    /// `RBX_STUDIO_OPEN_SCRIPT=<name>[,<name>...]`: opens each named script
    /// through the exact path a double-click does, once, at startup. The only
    /// way to get a tab open for a screenshot — nothing else can double-click
    /// the Explorer on the editor's behalf (see `AGENTS.md`'s safety rules).
    pub(super) fn apply_debug_open_script(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var(OPEN_VARIABLE) else {
            return;
        };
        for name in spec.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            if let Some(reference) = explorer::find_by_name(&self.dom, name) {
                self.open_script(reference, window, cx);
            }
        }
    }

    /// Brings an open tab to the front; the dock's tab strip's click handler.
    pub(crate) fn activate_script(&mut self, reference: Ref, cx: &mut Context<Self>) {
        self.scripts.tabs.activate(reference);
        cx.notify();
    }

    /// Closes one tab, writing whatever it still held to the DOM first so
    /// closing a tab can never be a way to lose an edit.
    pub(crate) fn close_script(&mut self, reference: Ref, cx: &mut Context<Self>) {
        self.commit_script(reference, None, cx);
        self.scripts.tabs.close(reference);
        self.scripts.open.remove(&reference);
        cx.notify();
    }

    /// Writes every tab's pending text to the DOM now rather than on its own
    /// debounce. `pub(super)`: `shell::history` calls it before an undo and
    /// `shell::save` before a write, so both act on the text as typed.
    pub(super) fn flush_script_edits(&mut self, cx: &mut Context<Self>) {
        for reference in self.scripts.tabs.all().to_vec() {
            self.commit_script(reference, None, cx);
        }
    }

    /// Reconciles every open tab against the DOM. Called from the panel's own
    /// render, so it runs on the frame after whatever changed the DOM without
    /// any of those paths having to know the script editor exists.
    ///
    /// A tab whose script is gone closes; a tab whose `Source` moved
    /// underneath it is re-seeded. A tab holding uncommitted typing is left
    /// alone — that text is what would be destroyed, and its own commit is
    /// what reaches the DOM next.
    pub(super) fn resync_scripts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Shell {
            scripts,
            dom,
            database,
            ..
        } = self;
        scripts
            .tabs
            .retain(|reference| source::is_script(dom, database, reference));
        let live = scripts.tabs.all().to_vec();
        scripts.open.retain(|reference, _| live.contains(reference));

        for reference in live {
            let Some(open) = self.scripts.open.get(&reference) else {
                continue;
            };
            if open.pending || source::is(&self.dom, reference, &open.synced) {
                continue;
            }
            let text = source::read(&self.dom, reference).unwrap_or_default();
            let state = open.state.clone();
            state.update(cx, |state, cx| state.set_value(text.clone(), window, cx));
            self.mark_synced(reference, text);
        }
    }

    /// Whether an open script editor currently holds focus.
    ///
    /// While one does, Ctrl+Z belongs to that editor's own text history and
    /// not to the place's — the same split Studio makes, where the script
    /// editor undoes typing and the place's history undoes everything else.
    /// Running both would step two stacks on one keypress, reverting an
    /// unrelated earlier edit along with the typing. Nothing is lost by
    /// standing aside: whatever the editor's undo leaves behind reaches the
    /// DOM through the usual debounced write.
    pub(super) fn script_editor_focused(&self, window: &Window, cx: &App) -> bool {
        self.scripts
            .open
            .values()
            .any(|open| open.state.focus_handle(cx).contains_focused(window, cx))
    }

    fn focus_script(&self, reference: Ref, window: &mut Window, cx: &mut App) {
        if let Some(open) = self.scripts.open.get(&reference) {
            window.focus(&open.state.focus_handle(cx), cx);
        }
    }

    /// One keystroke in one tab: marks it dirty and schedules the write.
    fn script_changed(&mut self, reference: Ref, cx: &mut Context<Self>) {
        let Some(open) = self.scripts.open.get_mut(&reference) else {
            return;
        };
        open.pending = true;
        open.generation = open.generation.wrapping_add(1);
        let generation = open.generation;

        cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(COMMIT_DELAY).await;
            // The window is gone once this fails; there is nothing left to
            // write to.
            let _ = shell.update(cx, |shell, cx| {
                shell.commit_script(reference, Some(generation), cx);
            });
        })
        .detach();
    }

    /// Writes one tab's text to `Source`. `generation` is `Some` for a
    /// debounced write, which must do nothing if another keystroke has landed
    /// since it was scheduled, and `None` for a flush, which always writes
    /// whatever is there.
    fn commit_script(&mut self, reference: Ref, generation: Option<u64>, cx: &mut Context<Self>) {
        let Some(open) = self.scripts.open.get(&reference) else {
            return;
        };
        if !open.pending || generation.is_some_and(|scheduled| scheduled != open.generation) {
            return;
        }
        let text = open.state.read(cx).value().to_string();

        // Typed back to exactly what the DOM already holds (an undo inside
        // the editor, say): there is no edit to record, and pushing one would
        // put an undo step on the stack that changes nothing.
        if source::is(&self.dom, reference, &text) {
            self.mark_synced(reference, text);
            return;
        }

        // See `shell::history`: the snapshot is of the tree as it stood
        // before the write, and is only pushed once the write has landed.
        self.dom.take_changes();
        let before = self.dom.clone();
        if !source::write(&mut self.dom, reference, &text) {
            // Nothing reached the DOM. The referent stopped resolving between
            // the guard above and here — a tab outlives its script for the
            // frame between a delete and the `resync_scripts` that closes it,
            // and a debounce firing in that window lands exactly here. The
            // tab is marked clean because there is no longer anywhere to
            // write its text to.
            self.mark_synced(reference, text);
            return;
        }
        self.push_history_snapshot(before);
        // Not reflected in the viewport, but a selected script's `Source`
        // row reads the DOM too.
        self.properties.dom_changed();
        // A `Source` write is exactly one property write, so undoing it takes
        // the same fast in-place viewport patch every other single edit does
        // rather than a full reload — but only if the log that write produced
        // is attached to the snapshot (see `shell::history`).
        let changes = self.dom.take_changes();
        self.record_history_change(changes);
        self.mark_synced(reference, text);
        cx.notify();
    }

    fn mark_synced(&mut self, reference: Ref, text: String) {
        if let Some(open) = self.scripts.open.get_mut(&reference) {
            open.synced = text;
            open.pending = false;
        }
    }
}
