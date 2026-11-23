//! Sharing a recording: Export repro… writes a `.rbxrepro`
//! (`crate::playtests::repro`), Copy bug report puts a Markdown summary on
//! the clipboard, and a repro opened from a file replays read-only here.
//!
//! `RBX_STUDIO_EXPORT_DIR` (see `shell::export`) skips the save dialog and
//! writes the repro into that folder: a screenshot and test aid.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::layout::Panel;
use super::super::rows;
use super::super::Shell;
use crate::command_bar::Feedback;
use crate::launcher::ui::{self, Weight};
use crate::playtests::repro::{self, PlaceMeta};
use crate::playtests::{clock, report, store, EventKind, Recording};
use crate::tokens;

/// Where a repro's own lines go in the Output dock.
const REPRO_SOURCE: &str = "Repro";

/// A shared repro, as the window replaying it keeps it.
pub(in crate::shell) struct ReproView {
    pub(in crate::shell) file: PathBuf,
    pub(in crate::shell) recording: Rc<Recording>,
    pub(in crate::shell) note: String,
    /// Version and place mismatches, said on top of the dock.
    pub(in crate::shell) notes: Vec<String>,
    pub(in crate::shell) editor_version: String,
    pub(in crate::shell) created: u64,
}

/// The Export repro… dialog's state.
pub(in crate::shell) struct ExportDraft {
    recording: Recording,
    /// The place as it is now, binary: what Play ran.
    place: Vec<u8>,
    place_name: String,
    universe_id: Option<u64>,
    place_id: Option<u64>,
    note: Entity<InputState>,
    embed: bool,
    /// The file's size with the place in, and without.
    sizes: (u64, u64),
}

impl ExportDraft {
    fn pack(&self, embed: bool, note: &str) -> Result<Vec<u8>, String> {
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        repro::pack(
            &self.recording,
            &self.place,
            PlaceMeta {
                name: self.place_name.clone(),
                universe_id: self.universe_id,
                place_id: self.place_id,
            },
            embed,
            note,
            created,
        )
    }
}

/// `file.rbxrepro` for whatever extension `path` had, or none.
fn with_extension(path: &Path) -> PathBuf {
    if repro::is_repro(path) {
        path.to_path_buf()
    } else {
        let mut name = path.as_os_str().to_owned();
        name.push(format!(".{}", repro::EXTENSION));
        PathBuf::from(name)
    }
}

impl Shell {
    pub(super) fn playtests_place_name(&self) -> String {
        let title = self.title.to_string();
        [".rbxlx", ".rbxl", ".rbxm"]
            .iter()
            .find_map(|ext| title.strip_suffix(ext))
            .unwrap_or(&title)
            .to_owned()
    }

    /// The recording behind dock entry `path`: the repro's, or read off disk.
    fn playtests_recording(&self, path: &Path) -> Result<Rc<Recording>, String> {
        match &self.playtests.repro {
            Some(repro) if repro.file == path => Ok(repro.recording.clone()),
            _ => store::load(path).map(Rc::new),
        }
    }

    pub(super) fn playtests_request_export(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.playtests.export_request = Some(path);
        cx.notify();
    }

    /// Builds the Export repro… dialog asked for since the last frame.
    pub(in crate::shell) fn playtests_prepare_export(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.playtests.export_request.take() else {
            return;
        };
        let recording = match self.playtests_recording(&path) {
            Ok(recording) => (*recording).clone(),
            Err(err) => return self.warn(format!("Export repro: {err}"), cx),
        };
        self.flush_script_edits(cx);
        let place = match crate::save::Format::Binary.encode(&self.dom) {
            Ok(bytes) => bytes,
            Err(err) => return self.warn(format!("Export repro: {err}"), cx),
        };
        let link = crate::home::link_of(&self.path);
        let note =
            cx.new(|cx| InputState::new(window, cx).placeholder("What went wrong? (optional)"));
        note.update(cx, |state, cx| state.focus(window, cx));
        let mut draft = ExportDraft {
            recording,
            place,
            place_name: self.playtests_place_name(),
            universe_id: link.as_ref().map(|link| link.universe_id),
            place_id: link.as_ref().map(|link| link.place_id),
            note,
            embed: true,
            sizes: (0, 0),
        };
        let size = |embed| draft.pack(embed, "").map_or(0, |bytes| bytes.len() as u64);
        draft.sizes = (size(true), size(false));
        self.playtests.export = Some(draft);
        cx.notify();
    }

    fn playtests_export_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.playtests.export.take() else {
            return;
        };
        let note = draft.note.read(cx).value().to_string();
        let bytes = match draft.pack(draft.embed, &note) {
            Ok(bytes) => bytes,
            Err(err) => return self.warn(format!("Export repro: {err}"), cx),
        };
        cx.notify();
        let file_name = format!(
            "{}.{}",
            draft
                .recording
                .name
                .replace(|c: char| !c.is_alphanumeric() && c != '_', "-")
                .trim_matches('-'),
            repro::EXTENSION
        );
        if let Ok(directory) = std::env::var(super::super::export::EXPORT_DIR_VARIABLE) {
            self.playtests_write_repro(&Path::new(&directory).join(file_name), &bytes, cx);
            return;
        }
        let directory = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let picked = cx.prompt_for_new_path(&directory, Some(&file_name));
        cx.spawn(async move |this, cx| {
            let Ok(picked) = picked.await else {
                return;
            };
            let _ = this.update(cx, |shell, cx| match picked {
                Ok(Some(path)) => shell.playtests_write_repro(&with_extension(&path), &bytes, cx),
                Ok(None) => {}
                Err(err) => shell.warn(format!("Export repro: no save dialog: {err:#}"), cx),
            });
        })
        .detach();
    }

    fn playtests_write_repro(&mut self, path: &Path, bytes: &[u8], cx: &mut Context<Self>) {
        let feedback = match crate::save::write_atomic(path, bytes) {
            Ok(()) => Feedback::Output(format!(
                "Exported the repro to {} ({}).",
                path.display(),
                repro::size_label(bytes.len() as u64)
            )),
            Err(err) => Feedback::Warning(format!("Could not export the repro: {err}")),
        };
        self.output.push(REPRO_SOURCE, feedback);
        cx.notify();
    }

    /// Copy bug report: the recording's Markdown summary on the clipboard.
    pub(super) fn playtests_copy_report(&mut self, path: &Path, cx: &mut Context<Self>) {
        let recording = match self.playtests_recording(path) {
            Ok(recording) => recording,
            Err(err) => return self.warn(format!("Copy bug report: {err}"), cx),
        };
        let note = match &self.playtests.repro {
            Some(repro) if repro.file == path => repro.note.as_str(),
            _ => "",
        };
        cx.write_to_clipboard(ClipboardItem::new_string(report::bug_report(
            &recording, note,
        )));
        self.output.push(
            REPRO_SOURCE,
            Feedback::Output(format!(
                "Copied a bug report for {} to the clipboard.",
                recording.name
            )),
        );
        cx.notify();
    }

    /// File › Open or a drop of a `.rbxrepro`: a window of its own, this one
    /// staying as it is.
    pub(in crate::shell) fn open_repro_window(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            if let Err(err) = crate::open_place(&path, cx) {
                let _ = this.update(cx, |shell, cx| shell.warn(err, cx));
            }
        });
    }

    /// Makes this window the replay of `opened`: the dock lists it alone,
    /// shown, with its notes on top, and its recorded lines go to Output.
    pub(in crate::shell) fn playtests_open_repro(
        &mut self,
        opened: repro::Opened,
        cx: &mut Context<Self>,
    ) {
        let recording = Rc::new(opened.recording);
        let name_of = |user: u64| {
            recording
                .players
                .iter()
                .find(|player| player.user == user)
                .map_or_else(|| format!("player {user}"), |player| player.name.clone())
        };
        self.output.push(
            REPRO_SOURCE,
            Feedback::Info(format!(
                "Replaying {} from {}, read-only.",
                recording.name,
                opened.file.display()
            )),
        );
        for note in &opened.notes {
            self.output
                .push(REPRO_SOURCE, Feedback::Warning(note.clone()));
        }
        for event in &recording.events {
            let at = clock(event.t);
            let line = match event.kind {
                EventKind::Error => Feedback::Error(format!("[{at}] {}", event.label)),
                EventKind::Warning => Feedback::Warning(format!("[{at}] {}", event.label)),
                EventKind::Chat => {
                    Feedback::Output(format!("[{at}] {}: {}", name_of(event.user), event.label))
                }
                EventKind::Mark => {
                    Feedback::Info(format!("[{at}] mark {} {}", event.label, event.data))
                }
                _ => continue,
            };
            self.output.push(REPRO_SOURCE, line);
        }
        self.playtests.repro = Some(ReproView {
            file: opened.file,
            recording,
            note: opened.manifest.note,
            notes: opened.notes,
            editor_version: opened.manifest.editor_version,
            created: opened.manifest.created,
        });
        self.playtests.entries.clear();
        self.playtests.time = 0.0;
        self.set_panel_open(Panel::Playtests, true, cx);
        self.playtests_refresh(cx);
    }

    /// The repro's card on top of the dock: what it is, the sender's note,
    /// mismatches, and Copy bug report.
    pub(super) fn playtests_repro_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let repro = self.playtests.repro.as_ref()?;
        let made = chrono::DateTime::from_timestamp(repro.created as i64, 0)
            .map(|at| {
                at.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        let file = repro
            .file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let path = repro.file.clone();
        Some(
            v_flex()
                .mx_2()
                .p_2()
                .gap_1()
                .rounded(tokens::radius())
                .border_1()
                .border_color(tokens::border())
                .bg(tokens::field_select())
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(ui::icon("bug", 13.).text_color(ui::accent()))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(tokens::text_xs())
                                .text_color(tokens::text())
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("Repro {file} · read-only")),
                        )
                        .child(
                            ui::icon_button(
                                "playtests-copy-report",
                                "clipboard-copy",
                                "Copy bug report",
                                Weight::Secondary,
                                true,
                            )
                            .on_click(cx.listener(
                                move |shell, _, _, cx| shell.playtests_copy_report(&path, cx),
                            )),
                        ),
                )
                .child(
                    div()
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text2())
                        .child(format!(
                            "Made with RbxNative {} on {made}",
                            repro.editor_version
                        )),
                )
                .when(!repro.note.is_empty(), |card| {
                    card.child(
                        div()
                            .pl_2()
                            .border_l_2()
                            .border_color(tokens::border())
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text())
                            .child(repro.note.clone()),
                    )
                })
                .children(repro.notes.iter().map(|note| {
                    div()
                        .text_size(tokens::text_xs())
                        .text_color(tokens::warning())
                        .child(note.clone())
                }))
                .into_any_element(),
        )
    }

    pub(in crate::shell) fn playtests_export_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let draft = self.playtests.export.as_ref()?;
        let published = draft.place_id.is_some();
        let embed = draft.embed;
        let size = if embed { draft.sizes.0 } else { draft.sizes.1 };
        let small = |text: String, color: Rgba| ui::text(12., 18.).text_color(color).child(text);
        let warning = if embed {
            "Including the place shares all of it: every script, model and asset id in it. Only send this file to people you'd give the place to."
                .to_owned()
        } else {
            format!(
                "Only the place's id and a hash go in. Whoever opens it needs their own copy of place {} to see it under the replay.",
                draft.place_id.unwrap_or_default()
            )
        };
        let toggle = rows::checkbox("playtests-export-embed", embed, {
            let handle = cx.entity();
            move |_, _, cx| {
                handle.update(cx, |shell, cx| {
                    if let Some(draft) = shell.playtests.export.as_mut() {
                        if draft.place_id.is_some() {
                            draft.embed = !draft.embed;
                        }
                    }
                    cx.notify();
                })
            }
        });
        let body = v_flex()
            .px(px(20.))
            .pt(px(16.))
            .gap(px(12.))
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(small("Note".into(), tokens::text2()))
                    .child(Input::new(&draft.note)),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_start()
                    .child(toggle.when(!published, |toggle| toggle.opacity(0.5)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.))
                            .child(small("Include the place".into(), tokens::text()))
                            .child(small(
                                if published {
                                    "Lets anyone replay it over the exact place it was recorded in."
                                        .into()
                                } else {
                                    "This place isn't published, so the repro has to carry it."
                                        .into()
                                },
                                tokens::text2(),
                            )),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_start()
                    .p(px(10.))
                    .rounded(px(6.))
                    .bg(tokens::field_select())
                    .child(
                        ui::icon(if embed { "triangle-alert" } else { "info" }, 14.)
                            .flex_none()
                            .text_color(if embed {
                                tokens::warning()
                            } else {
                                tokens::text2()
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(small(warning, tokens::text())),
                    ),
            )
            .child(small(
                format!(
                    "{} · {} · repro file {}",
                    draft.recording.name,
                    clock(draft.recording.duration),
                    repro::size_label(size)
                ),
                tokens::text2(),
            ));
        Some(
            ui::dialog(
                480.,
                ui::dialog_glyph("package", ui::accent(), ui::wash()),
                "Export repro",
                "One file with this recording and what it needs to replay it, to attach to a bug report.",
                Some(body.into_any_element()),
                vec![
                    ui::button("playtests-export-cancel", "Cancel", Weight::Secondary, false)
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.playtests.export = None;
                            cx.notify();
                        }))
                        .into_any_element(),
                    ui::button("playtests-export-save", "Export…", Weight::Primary, false)
                        .on_click(cx.listener(|shell, _, _, cx| shell.playtests_export_confirm(cx)))
                        .into_any_element(),
                ],
            )
            .into_any_element(),
        )
    }
}
