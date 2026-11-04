//! Find in Place: Studio's Ctrl+Shift+F panel, a search (and Ctrl+Shift+H
//! replace) over the `Source` of every script in the place, open or not.
//! The search itself is `scan`; the UI is `view`. This file is the state and
//! what the controls do.
//!
//! Replace goes straight to the DOM, as `commit_script` does, but for every
//! affected script at once as one history step, so the place's Undo puts
//! them all back (Studio's own cannot undo a script that is closed).

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_dom::Ref;

use crate::script_editor::find::{Options, Pattern};
use crate::script_editor::source;

use super::chrome::Document;
use super::layout::Panel;
use super::Shell;
use scan::{Place, Results};

mod scan;
mod view;

/// The three text fields, made the first time the panel is asked for since
/// an `InputState` needs a window.
struct Inputs {
    query: Entity<InputState>,
    filter: Entity<InputState>,
    replacement: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

/// Everything a scan read, so a frame can tell whether the results it holds
/// are still true without scanning the place again.
#[derive(PartialEq)]
struct Signature {
    query: String,
    filter: String,
    replacement: Option<String>,
    options: Options,
    revision: u64,
    mutations: u64,
    live: bool,
    typing: Vec<(Ref, u64)>,
}

#[derive(Default)]
pub(crate) struct FindInPlace {
    inputs: Option<Inputs>,
    options: Options,
    show_filter: bool,
    show_replace: bool,
    /// Index into the rows on screen (collapsed scripts' rows are not).
    selected: usize,
    results: Results,
    seen: Option<Signature>,
    collapsed: HashSet<Ref>,
    confirming: bool,
    scroll: ScrollHandle,
}

impl FindInPlace {
    /// (file, hit) indexes of every row on screen, top to bottom.
    fn rows(&self) -> Vec<(usize, usize)> {
        rows_of(&self.results, &self.collapsed)
    }
}

/// Collapsed scripts' rows are not on screen.
fn rows_of(results: &Results, collapsed: &HashSet<Ref>) -> Vec<(usize, usize)> {
    let mut rows = Vec::new();
    for (file, entry) in results.files.iter().enumerate() {
        if !collapsed.contains(&entry.script) {
            rows.extend((0..entry.hits.len()).map(|hit| (file, hit)));
        }
    }
    rows
}

impl Shell {
    fn ensure_find_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find_place.inputs.is_some() {
            return;
        }
        let field = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let query = field("Find in Place", window, cx);
        let filter = field("Script name contains", window, cx);
        let replacement = field("Replace with", window, cx);
        // A changed query starts the list over from its first row.
        let subscriptions = [&query, &filter, &replacement]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |shell, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        shell.find_place.selected = 0;
                        shell.find_place.confirming = false;
                        cx.notify();
                    }
                })
            })
            .collect();
        self.find_place.inputs = Some(Inputs {
            query,
            filter,
            replacement,
            _subscriptions: subscriptions,
        });
    }

    pub(super) fn focus_find_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_find_inputs(window, cx);
        if let Some(inputs) = &self.find_place.inputs {
            inputs.query.update(cx, |state, cx| state.focus(window, cx));
        }
    }

    /// Ctrl+Shift+F / Ctrl+Shift+H and the ribbon's Find / Replace: shows the
    /// panel and puts the caret in its search field, from any document. The
    /// active editor's one-line selection becomes the query, as Studio's does.
    pub(super) fn open_find_in_place(
        &mut self,
        replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let seed = self
            .scripts
            .tabs
            .active()
            .filter(|_| self.document == Document::Scripts)
            .and_then(|active| self.scripts.open.get(&active))
            .map(|open| open.state.read(cx).selected_value().to_string())
            .filter(|text| !text.is_empty() && !text.contains('\n'));
        self.ensure_find_inputs(window, cx);
        if replace {
            self.find_place.show_replace = true;
        }
        self.set_panel_open(Panel::FindInPlace, true, cx);
        if let Some(inputs) = &self.find_place.inputs {
            inputs.query.update(cx, |state, cx| {
                if let Some(seed) = seed {
                    state.set_value(seed, window, cx);
                }
                state.select_all(window, cx);
                state.focus(window, cx);
            });
        }
    }

    fn find_texts(&self, cx: &App) -> Option<(String, String, String)> {
        let inputs = self.find_place.inputs.as_ref()?;
        let read = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let filter = match self.find_place.show_filter {
            true => read(&inputs.filter),
            false => String::new(),
        };
        Some((read(&inputs.query), filter, read(&inputs.replacement)))
    }

    /// Scans again if anything the results depend on moved: the fields, the
    /// toggles, the DOM (an edit, an undo) or typing in an open tab.
    fn refresh_find_results(&mut self, cx: &App) {
        let Some((query, filter, replacement)) = self.find_texts(cx) else {
            return;
        };
        let replacement =
            (self.find_place.show_replace && !replacement.is_empty()).then_some(replacement);
        let live = self.live_shown();
        let mut pending: Vec<(Ref, u64)> = self
            .scripts
            .open
            .iter()
            .filter(|(_, open)| open.pending)
            .map(|(&script, open)| (script, open.generation))
            .collect();
        pending.sort_by_key(|(script, _)| script.value());
        let signature = Signature {
            query,
            filter,
            replacement,
            options: self.find_place.options,
            revision: self.history.revision(),
            mutations: self.edit_dom().mutations(),
            live,
            typing: pending,
        };
        if self.find_place.seen.as_ref() == Some(&signature) {
            return;
        }
        let typing: HashMap<Ref, String> = match live {
            true => HashMap::new(),
            false => self
                .scripts
                .open
                .iter()
                .filter(|(_, open)| open.pending)
                .map(|(&script, open)| (script, open.state.read(cx).value().to_string()))
                .collect(),
        };
        let results = match Pattern::compile(&signature.query, signature.options) {
            Ok(pattern) => scan::scan(
                &Place {
                    dom: self.edit_dom(),
                    database: &self.database,
                    typing: &typing,
                },
                &pattern,
                &signature.filter,
                signature.replacement.as_deref(),
            ),
            Err(error) => Results {
                error: Some(error),
                ..Results::default()
            },
        };
        self.find_place.results = results;
        self.find_place.seen = Some(signature);
        let rows = self.find_place.rows().len();
        self.find_place.selected = self.find_place.selected.min(rows.saturating_sub(1));
    }

    /// Opens the script `range` is in with it selected and revealed. Focus
    /// only follows when asked, so stepping with the arrow keys can go on.
    fn open_find_hit(
        &mut self,
        script: Ref,
        range: Range<usize>,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_place_script(script, window, cx);
        if let Some(open) = self.scripts.open.get(&script) {
            open.state.update(cx, |state, cx| {
                state.reveal_range(range, cx);
                if focus {
                    state.focus(window, cx);
                }
            });
        }
    }

    /// Selects row `row` and opens its match.
    fn select_find_row(
        &mut self,
        row: usize,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.find_place.rows();
        let Some(&(file, hit)) = rows.get(row) else {
            return;
        };
        self.find_place.selected = row;
        let entry = &self.find_place.results.files[file];
        let (script, range) = (entry.script, entry.hits[hit].range.clone());
        self.open_find_hit(script, range, focus, window, cx);
        cx.notify();
    }

    fn step_find_selection(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.find_place.rows().len();
        if count == 0 {
            return;
        }
        let at = self.find_place.selected;
        let next = match down {
            true => (at + 1).min(count - 1),
            false => at.saturating_sub(1),
        };
        self.select_find_row(next, false, window, cx);
    }

    fn toggle_find_file(&mut self, script: Ref, cx: &mut Context<Self>) {
        if !self.find_place.collapsed.remove(&script) {
            self.find_place.collapsed.insert(script);
        }
        let rows = self.find_place.rows().len();
        self.find_place.selected = self.find_place.selected.min(rows.saturating_sub(1));
        cx.notify();
    }

    /// The compiled search, from the fields as they are now.
    fn find_pattern(&self, cx: &App) -> Option<(Pattern, String, String)> {
        let (query, filter, replacement) = self.find_texts(cx)?;
        let pattern = Pattern::compile(&query, self.find_place.options).ok()?;
        Some((pattern, filter, replacement))
    }

    /// Writes every `edits` entry as one undo step; `false` if nothing
    /// changed, in which case no step is recorded.
    fn write_script_edits(&mut self, edits: &[(Ref, String)], cx: &mut Context<Self>) -> bool {
        // See `commit_script`: the snapshot is the tree as it stood before
        // the writes, pushed only once they have landed.
        let Some(before) = scan::write_step(&mut self.dom, edits) else {
            return false;
        };
        self.push_history_snapshot(before);
        self.properties.dom_changed(&[]);
        let changes = self.dom.take_changes();
        self.record_history_change(changes);
        cx.notify();
        true
    }

    /// Replaces the selected match.
    fn replace_find_current(&mut self, cx: &mut Context<Self>) {
        if self.live_shown() {
            return;
        }
        self.flush_script_edits(cx);
        let Some((pattern, filter, replacement)) = self.find_pattern(cx) else {
            return;
        };
        let no_typing = HashMap::new();
        let found = scan::scan(
            &Place {
                dom: &self.dom,
                database: &self.database,
                typing: &no_typing,
            },
            &pattern,
            &filter,
            None,
        );
        let rows = rows_of(&found, &self.find_place.collapsed);
        let Some(&(file, hit)) = rows.get(self.find_place.selected) else {
            return;
        };
        let entry = &found.files[file];
        let Some(text) = source::read(&self.dom, entry.script) else {
            return;
        };
        let edited = pattern.replace_one(&text, &entry.hits[hit].range, &replacement);
        let script = entry.script;
        self.write_script_edits(&[(script, edited)], cx);
    }

    /// Replaces every match in every script the filter keeps, as one step.
    fn replace_find_all(&mut self, cx: &mut Context<Self>) {
        self.find_place.confirming = false;
        if self.live_shown() {
            return;
        }
        self.flush_script_edits(cx);
        let Some((pattern, filter, replacement)) = self.find_pattern(cx) else {
            return;
        };
        let no_typing = HashMap::new();
        let edits = scan::replacements(
            &Place {
                dom: &self.dom,
                database: &self.database,
                typing: &no_typing,
            },
            &pattern,
            &filter,
            &replacement,
        );
        self.write_script_edits(&edits, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
