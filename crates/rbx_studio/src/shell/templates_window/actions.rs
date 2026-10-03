//! What the window does to the files: create, delete, rename, duplicate and
//! import, each through the templates store, then a reload. Also the window's
//! own notices, since its Output-dock lines would land in another window.

use std::path::PathBuf;

use gpui_kit::*;

use crate::script_templates::{Imported, NameError, SkipReason, StoreError, CLASSES};

use super::{Selected, TemplatesWindow};

/// A message the window shows above the editor until it no longer applies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Notice {
    /// A Class change was refused: red, until the next edit or class change.
    MoveFailed { to: &'static str, reason: String },
    /// Something worth saying (what an import skipped, a failed delete):
    /// neutral, until dismissed.
    Info(String),
}

/// The class new and imported templates go to: the selected row's, or
/// Script when nothing is selected.
pub(super) fn class_of(selected: Option<&Selected>) -> &'static str {
    match selected {
        Some(Selected::Starter(class))
        | Some(Selected::Template { class, .. })
        | Some(Selected::Skipped { class, .. }) => class,
        None => CLASSES[0],
    }
}

impl TemplatesWindow {
    /// Selects `row`, opens it, and puts the caret in its editor.
    pub(super) fn select_and_edit(
        &mut self,
        row: Selected,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = Some(row);
        self.open_editor(window, cx);
        if let Some(editor) = &self.editor {
            editor.state.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.shell
            .update(cx, |shell, cx| shell.reload_script_templates(cx));
    }

    /// Writes a new template; the caller shows the error on the name field.
    pub(super) fn create(
        &mut self,
        class: &'static str,
        name: &str,
        source: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), StoreError> {
        let name = self
            .shell
            .read(cx)
            .script_templates
            .create(class, name, source)?;
        self.reload(cx);
        self.select_and_edit(Selected::Template { class, name }, window, cx);
        Ok(())
    }

    /// Deletes a template or a skipped file, then selects the next row of
    /// its group (the one before it if it was last), or the group's starter.
    pub(super) fn delete(&mut self, row: &Selected, window: &mut Window, cx: &mut Context<Self>) {
        let templates = &self.shell.read(cx).script_templates;
        let (result, next) = match row {
            Selected::Template { class, name } => {
                let group: Vec<_> = templates
                    .extras()
                    .iter()
                    .filter(|t| t.class == *class)
                    .map(|t| t.name.clone())
                    .collect();
                let at = group.iter().position(|n| n == name).unwrap_or(0);
                let next = group
                    .get(at + 1)
                    .or(at.checked_sub(1).and_then(|i| group.get(i)))
                    .map(|name| Selected::Template {
                        class,
                        name: name.clone(),
                    })
                    .unwrap_or(Selected::Starter(class));
                (templates.delete(class, name), next)
            }
            Selected::Skipped { class, file_name } => (
                templates.delete_skipped(class, file_name),
                Selected::Starter(class),
            ),
            Selected::Starter(_) => return,
        };
        if let Err(err) = result {
            self.notice = Some(Notice::Info(format!("Couldn\u{2019}t delete: {err}")));
        }
        self.reload(cx);
        self.selected = Some(next);
        self.open_editor(window, cx);
        cx.notify();
    }

    /// Renames the template `old` of `class`; the caller shows the error.
    pub(super) fn rename_to(
        &mut self,
        class: &'static str,
        old: &str,
        new: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), StoreError> {
        let name = self
            .shell
            .read(cx)
            .script_templates
            .rename(class, old, new)?;
        self.reload(cx);
        self.selected = Some(Selected::Template { class, name });
        self.open_editor(window, cx);
        cx.notify();
        Ok(())
    }

    /// Duplicate (a template) and Duplicate as new (a starter): a copy
    /// called "<name> copy" in the same class, selected, with its name
    /// ready to type over.
    pub(super) fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.selected.clone() else {
            return;
        };
        let (class, base) = match &row {
            Selected::Template { class, name } => (*class, name.clone()),
            Selected::Starter(class) => (*class, "Default starter".to_owned()),
            Selected::Skipped { .. } => return,
        };
        let Some(source) = self.source_of(&row, cx) else {
            return;
        };
        let templates = &self.shell.read(cx).script_templates;
        let Some(copy) = templates.free_name(class, &base, " copy") else {
            return;
        };
        if let Err(err) = templates.write(class, &copy, &source) {
            self.notice = Some(Notice::Info(format!("Couldn\u{2019}t duplicate: {err}")));
            cx.notify();
            return;
        }
        self.reload(cx);
        self.selected = Some(Selected::Template {
            class,
            name: copy.clone(),
        });
        self.open_editor(window, cx);
        self.start_rename(class, copy, window, cx);
    }

    /// The Class control: moves the selected template's file into `to`. A
    /// refusal shows in this window, and the control stays on the file's
    /// real class.
    pub(super) fn move_to_class(
        &mut self,
        to: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Selected::Template { class, name }) = self.selected.clone() else {
            return;
        };
        self.flush_save(cx);
        let moved = self
            .shell
            .read(cx)
            .script_templates
            .move_to(class, &name, to);
        self.notice = match moved {
            Ok(()) => {
                self.selected = Some(Selected::Template { class: to, name });
                None
            }
            Err(StoreError::Name(NameError::Taken { class, name })) => Some(Notice::MoveFailed {
                to,
                reason: format!("A {class} template called {name} already exists."),
            }),
            Err(err) => Some(Notice::MoveFailed {
                to,
                reason: err.to_string(),
            }),
        };
        self.reload(cx);
        self.follow_disk(window, cx);
        cx.notify();
    }

    /// The `upload` button and the empty state's "Import .luau files…".
    pub(super) fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let picked = picked.await;
            this.update_in(cx, |this, window, cx| match picked {
                Ok(Ok(Some(paths))) => this.finish_import(paths, window, cx),
                Ok(Ok(None)) => {}
                // No file chooser portal, or no session bus to reach one.
                _ => {
                    this.notice = Some(Notice::Info(
                        "Couldn\u{2019}t open a file picker on this system.".into(),
                    ));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn finish_import(&mut self, picked: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let class = class_of(self.selected.as_ref());
        let imported = self.shell.read(cx).script_templates.import(class, &picked);
        let imported = match imported {
            Ok(imported) => imported,
            Err(err) => {
                self.notice = Some(Notice::Info(format!("Import stopped: {err}")));
                self.reload(cx);
                cx.notify();
                return;
            }
        };
        let line = summary(&imported);
        self.shell.update(cx, |shell, _| {
            shell
                .output
                .push_warning(&format!("Script Templates: {line}"));
        });
        self.notice = (imported.ignored > 0 || !imported.refused.is_empty())
            .then_some(Notice::Info(line.clone()));
        self.reload(cx);
        if let Some(name) = imported.added.into_iter().next() {
            self.select_and_edit(Selected::Template { class, name }, window, cx);
        }
        cx.notify();
    }
}

/// "Imported 3 templates, ignored 2 files that aren't .luau." plus each
/// `.luau` file that couldn't be used.
fn summary(imported: &Imported) -> String {
    let added = imported.added.len();
    let mut line = format!(
        "Imported {added} template{}",
        if added == 1 { "" } else { "s" }
    );
    if imported.ignored > 0 {
        let n = imported.ignored;
        line += &format!(
            ", ignored {n} file{} that {} .luau",
            if n == 1 { "" } else { "s" },
            if n == 1 {
                "isn\u{2019}t"
            } else {
                "aren\u{2019}t"
            }
        );
    }
    for (file, reason) in &imported.refused {
        let why = match reason {
            SkipReason::NotUtf8 => "not UTF-8 text",
            SkipReason::TooLarge => "over 256 KiB",
            SkipReason::Unreadable => "couldn\u{2019}t be read",
        };
        line += &format!(", skipped {file} ({why})");
    }
    line.push('.');
    line
}

#[cfg(test)]
mod tests {
    use super::summary;
    use crate::script_templates::{Imported, SkipReason};

    #[test]
    fn the_import_line_counts_what_was_added_and_ignored() {
        let imported = Imported {
            added: vec!["A".into(), "B".into(), "C".into()],
            ignored: 2,
            refused: Vec::new(),
        };
        assert_eq!(
            summary(&imported),
            "Imported 3 templates, ignored 2 files that aren\u{2019}t .luau."
        );
        let imported = Imported {
            added: vec!["A".into()],
            ignored: 1,
            refused: vec![("Bad.luau".into(), SkipReason::NotUtf8)],
        };
        assert_eq!(
            summary(&imported),
            "Imported 1 template, ignored 1 file that isn\u{2019}t .luau, skipped Bad.luau (not UTF-8 text)."
        );
    }
}
