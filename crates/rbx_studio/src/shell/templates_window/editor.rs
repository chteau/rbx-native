//! The selected template's text and its autosave. The editor is the Script
//! Editor's own (`EditorState` with its Luau highlighter); what it adds is
//! the write ~600 ms after the last keystroke, through the store, and the
//! save state the status line shows.

use std::time::Duration;

use gpui_kit::component::input::{EditorState, InputEvent};
use gpui_kit::*;

use crate::script_editor::highlight;
use crate::script_templates::{StoreError, DEFAULT_STEM, MAX_BYTES};
use crate::tokens;

use super::{Selected, TemplatesWindow};

const SAVE_DELAY: Duration = Duration::from_millis(600);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SaveState {
    /// A built-in starter nobody has edited: nothing on disk.
    BuiltIn,
    Saved,
    Saving,
    /// Over [`MAX_BYTES`]: nothing was written, the last good file stays.
    TooLarge,
    /// The write failed; the last good file stays.
    Failed(String),
}

pub(super) struct TemplateEditor {
    pub(super) row: Selected,
    pub(super) state: Entity<EditorState>,
    pub(super) save: SaveState,
    /// The pending autosave; replacing it cancels the one before.
    pending: Option<Task<()>>,
    _changed: Subscription,
}

impl TemplateEditor {
    /// The text's size in bytes, which is what the 256 KiB limit counts.
    pub(super) fn len(&self, cx: &App) -> u64 {
        self.state.read(cx).value().len() as u64
    }
}

impl TemplatesWindow {
    /// What `row` holds on disk, or the built-in starter's text.
    pub(super) fn source_of(&self, row: &Selected, cx: &App) -> Option<String> {
        let shell = self.shell.read(cx);
        let templates = &shell.script_templates;
        match row {
            Selected::Starter(class) => templates
                .default_for(class)
                .or_else(|| super::super::keys::default_template(&shell.database, class))
                .map(str::to_owned),
            Selected::Template { class, name } => templates
                .extras()
                .iter()
                .find(|t| t.class == *class && t.name == *name)
                .map(|t| t.source.clone()),
            Selected::Skipped { .. } => None,
        }
    }

    fn save_state_on_disk(&self, row: &Selected, cx: &App) -> SaveState {
        match row {
            Selected::Starter(class)
                if self
                    .shell
                    .read(cx)
                    .script_templates
                    .default_for(class)
                    .is_none() =>
            {
                SaveState::BuiltIn
            }
            _ => SaveState::Saved,
        }
    }

    /// Opens the selected row in a fresh editor (a skipped file never is).
    pub(super) fn open_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor = None;
        let Some(row) = self.selected.clone() else {
            return;
        };
        let Some(source) = self.source_of(&row, cx) else {
            return;
        };
        let state = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .language(highlight::LANGUAGE)
                .folding(false)
                .searchable(true)
                .default_value(source);
            state.set_highlighter_factory(highlight::factory(), cx);
            // The card's own `bg`, and the current line at 3% white.
            state.set_line_number_gutter(px(46.), px(14.), cx);
            state.set_surface_colors(tokens::black().into(), hsla(0., 0., 1., 0.03), cx);
            state
        });
        let changed = cx.subscribe(&state, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.schedule_save(cx);
            }
        });
        let save = self.save_state_on_disk(&row, cx);
        self.editor = Some(TemplateEditor {
            row,
            state,
            save,
            pending: None,
            _changed: changed,
        });
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        editor.save = SaveState::Saving;
        // A refused class change is about the text as it was; typing moves on.
        if matches!(self.notice, Some(super::actions::Notice::MoveFailed { .. })) {
            self.notice = None;
        }
        let Some(editor) = &mut self.editor else {
            return;
        };
        editor.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            this.update(cx, |this, cx| this.save_now(cx)).ok();
        }));
        cx.notify();
    }

    /// Writes the editor's text, or says why it didn't.
    fn save_now(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        editor.pending = None;
        let text = editor.state.read(cx).value().to_string();
        let (class, stem) = match &editor.row {
            Selected::Starter(class) => (*class, DEFAULT_STEM),
            Selected::Template { class, name } => (*class, name.as_str()),
            Selected::Skipped { .. } => return,
        };
        if text.len() as u64 > MAX_BYTES {
            editor.save = SaveState::TooLarge;
            cx.notify();
            return;
        }
        let written = self
            .shell
            .read(cx)
            .script_templates
            .write(class, stem, &text);
        let editor = self.editor.as_mut().expect("checked above");
        editor.save = match written {
            Ok(()) => SaveState::Saved,
            Err(StoreError::TooLarge) => SaveState::TooLarge,
            Err(err) => SaveState::Failed(err.to_string()),
        };
        self.shell
            .update(cx, |shell, cx| shell.reload_script_templates(cx));
        cx.notify();
    }

    /// After a reload: a file changed on disk while nothing of the editor's
    /// own is waiting to be written takes the editor's place, so a hand edit
    /// shows up and "Reset to built-in" brings the built-in text back.
    pub(super) fn follow_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.as_ref().map(|e| &e.row) != self.selected.as_ref() {
            self.open_editor(window, cx);
            return;
        }
        let Some(editor) = &self.editor else {
            return;
        };
        // Text the editor couldn't write is still the user's: keep it.
        if editor.pending.is_some()
            || matches!(editor.save, SaveState::TooLarge | SaveState::Failed(_))
        {
            return;
        }
        let Some(source) = self.source_of(&editor.row, cx) else {
            return;
        };
        let save = self.save_state_on_disk(&editor.row, cx);
        let editor = self.editor.as_mut().expect("checked above");
        editor.save = save;
        if editor.state.read(cx).value().as_ref() != source {
            editor
                .state
                .update(cx, |state, cx| state.set_value(source, window, cx));
        }
    }

    /// "Reset to built-in": removes the class's `Default.luau`; the reload
    /// brings the built-in text back into the editor.
    pub(super) fn reset_starter(&mut self, class: &'static str, cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.editor {
            editor.pending = None;
            editor.save = SaveState::Saved;
        }
        let removed = self
            .shell
            .read(cx)
            .script_templates
            .delete(class, DEFAULT_STEM);
        self.after_write(removed, cx);
    }

    /// Writes now whatever is typed but not yet written.
    pub(super) fn flush_save(&mut self, cx: &mut Context<Self>) {
        if self.editor.as_ref().is_some_and(|e| e.pending.is_some()) {
            self.save_now(cx);
        }
    }

    /// Reloads after a write the window made, and reports a failed one in
    /// the Output dock (the status line is the editor's text, not this).
    fn after_write(&mut self, result: Result<(), StoreError>, cx: &mut Context<Self>) {
        self.shell.update(cx, |shell, cx| {
            if let Err(err) = &result {
                shell
                    .output
                    .push_warning(&format!("Script Templates: {err}"));
            }
            shell.reload_script_templates(cx);
        });
        cx.notify();
    }
}
