//! Reimport and Auto-Reimport, behind the `auto-reimport` beta switch, as
//! Studio's beta of that name (DevForum, "[Studio Beta] Auto-Reimport",
//! 2026-09-08, and Studio's own Explorer menu, measured with the beta on).
//!
//! Model › Import 3D Model… stamps the placed model with an
//! `RBX_ReimportId` and remembers its file on this device (see [`store`]).
//! The Explorer's menu then offers what Studio's Reimport submenu does:
//! Reimport, Configure… (pick the file), Remove reimport config (drops the
//! id), and Auto-Reimport, which watches the file for the rest of the
//! session and reimports whenever it is saved. Turning it on reimports once
//! straight away, and it is not saved with the place, both as in Studio.
//!
//! Every reimport goes through Import 3D's own upload
//! ([`import3d::upload_model`]), so, as Studio says of its own, each one
//! uploads the file again and the meshes get new asset ids; what changes in
//! the model is decided by [`merge`].
//!
//! The file is polled rather than watched through the OS, as the theme is
//! (see `theme::watch`): one stat a second per watched file. A change is
//! reimported once the file has sat still for a whole poll, so an exporter
//! writing in several passes triggers one upload, not several.

mod merge;
mod store;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui_kit::*;
use rbx_dom::{Ref, Variant, WeakDom};

use super::import3d::{self, Imported};
use super::Shell;
use crate::command_bar::Feedback;
use crate::properties::attributes;
use crate::theme::{fingerprint, POLL_INTERVAL};

use store::Store;
pub(crate) use store::ID_ATTRIBUTE;

pub(crate) const BETA_ID: &str = "auto-reimport";
const SOURCE: &str = "Reimport";

pub(super) fn enabled() -> bool {
    crate::beta::enabled(BETA_ID)
}

/// The session's watched instances and reimports in flight.
#[derive(Default)]
pub(super) struct Reimports {
    watched: HashMap<Ref, Watch>,
    busy: HashSet<Ref>,
    polling: bool,
}

impl Reimports {
    pub(super) fn is_watched(&self, target: Ref) -> bool {
        self.watched.contains_key(&target)
    }

    pub(super) fn watched(&self) -> HashSet<Ref> {
        self.watched.keys().copied().collect()
    }
}

/// One watched file: its last fingerprint, and whether a change to it is
/// waiting for the file to settle.
struct Watch {
    file: PathBuf,
    stamp: u64,
    pending: bool,
}

impl Watch {
    fn new(file: PathBuf) -> Watch {
        let stamp = fingerprint([file.as_path()]);
        Watch {
            file,
            stamp,
            pending: false,
        }
    }

    /// One poll's verdict on the file now fingerprinting as `now`: true when
    /// it changed and has since held still for a poll, and no reimport of
    /// it is already running.
    fn step(&mut self, now: u64, busy: bool) -> bool {
        if now != self.stamp {
            self.stamp = now;
            self.pending = true;
            return false;
        }
        let due = self.pending && !busy;
        if due {
            self.pending = false;
        }
        due
    }
}

/// The instance's reimport id, if it has one.
pub(crate) fn id_of(dom: &WeakDom, target: Ref) -> Option<String> {
    match attributes::attributes(dom, target).remove(ID_ATTRIBUTE)? {
        Variant::String(id) if !id.is_empty() => Some(id),
        _ => None,
    }
}

/// Writes (or with `None` removes) the id directly: the Properties
/// panel's own setter refuses the reserved `RBX` prefix, as Studio's
/// `SetAttribute` does outside its own code.
fn set_id(dom: &mut WeakDom, target: Ref, id: Option<&str>) -> Result<(), String> {
    let mut current = attributes::attributes(dom, target);
    match id {
        Some(id) => current.insert(ID_ATTRIBUTE.to_owned(), Variant::String(id.to_owned())),
        None => current.remove(ID_ATTRIBUTE),
    };
    if current.is_empty() {
        let _ = dom.remove_property(target, attributes::ATTRIBUTES_PROPERTY);
        return Ok(());
    }
    attributes::write_attributes(dom, target, &current)
}

/// Which reimport rows can act on `target`, the menu's guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Rows {
    /// A `Model`: Configure… can give it a file.
    pub(super) model: bool,
    /// Reimport and Auto-Reimport: a file is known for its id here.
    pub(super) configured: bool,
    /// Remove reimport config: it carries an id.
    pub(super) has_id: bool,
}

pub(super) fn rows(
    dom: &WeakDom,
    database: &rbx_reflection::ReflectionDatabase,
    target: Ref,
    store: &Store,
) -> Rows {
    let model = dom
        .get(target)
        .is_some_and(|i| database.is_subclass_of(i.class(), "Model"));
    let id = model.then(|| id_of(dom, target)).flatten();
    Rows {
        model,
        configured: id.as_deref().is_some_and(|id| store.file(id).is_some()),
        has_id: id.is_some(),
    }
}

impl Shell {
    pub(super) fn reimport_rows(&self, target: Ref) -> Rows {
        rows(&self.dom, &self.database, target, &Store::load())
    }

    /// Called as Import 3D places a model: the id on the model and its file
    /// in this device's map, so it can be reimported later.
    pub(super) fn stamp_import(&mut self, root: Ref, file: &Path) {
        let id = store::new_id();
        let mut store = Store::load();
        store.set(&id, file.to_path_buf());
        let stamped = set_id(&mut self.dom, root, Some(&id)).and_then(|()| store.save());
        if let Err(err) = stamped {
            self.output.push(
                SOURCE,
                Feedback::Warning(format!("can\u{2019}t be reimported later: {err}")),
            );
        }
    }

    /// Reimport: uploads the file again through Import 3D and merges what
    /// Roblox made of it into `target`.
    pub(super) fn reimport(&mut self, target: Ref, cx: &mut Context<Self>) {
        let Some(name) = self.dom.get(target).map(|i| i.name().to_string()) else {
            return;
        };
        let file =
            id_of(&self.dom, target).and_then(|id| Store::load().file(&id).map(Path::to_path_buf));
        let Some(file) = file else {
            return self.reimport_report(
                Err(format!(
                    "{name} has no reimport file on this device: use Configure Reimport\u{2026}"
                )),
                cx,
            );
        };
        if !self.reimports.busy.insert(target) {
            return;
        }
        self.command_bar
            .set_feedback(Feedback::Output(format!("Reimporting {name}\u{2026}")));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let stem = name.clone();
            let result = cx
                .background_spawn(async move { import3d::upload_model(&file, &stem, None) })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.finish_reimport(target, name, result, cx)
            });
        })
        .detach();
    }

    fn finish_reimport(
        &mut self,
        target: Ref,
        name: String,
        result: Result<Imported, String>,
        cx: &mut Context<Self>,
    ) {
        self.reimports.busy.remove(&target);
        let imported = match result {
            Ok(imported) => imported,
            Err(err) => return self.reimport_report(Err(format!("{name}: {err}")), cx),
        };
        if self.dom.get(target).is_none() {
            return self.reimport_report(
                Err(format!("{name} was deleted before its reimport finished")),
                cx,
            );
        }
        self.push_history();
        let merged = merge::reimport(&mut self.dom, &self.database, target, &imported.dom);
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        if let Some(note) = imported.note {
            self.output.push(SOURCE, Feedback::Warning(note));
        }
        let outcome = merged.map(|m| {
            format!(
                "Reimported {name} as asset {}: {} mesh{} updated, {} added",
                imported.asset_id,
                m.updated,
                if m.updated == 1 { "" } else { "es" },
                m.added
            )
        });
        self.reimport_report(outcome.map_err(|err| format!("{name}: {err}")), cx);
    }

    /// Configure…: picks the file `target` reimports from, giving it an id
    /// first if it has none.
    pub(super) fn configure_reimport(&mut self, target: Ref, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Reimport From".into()),
        });
        cx.spawn(async move |shell, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(file) = paths.into_iter().next() else {
                return;
            };
            let _ = shell.update(cx, |shell, cx| shell.save_reimport_file(target, file, cx));
        })
        .detach();
    }

    fn save_reimport_file(&mut self, target: Ref, file: PathBuf, cx: &mut Context<Self>) {
        if rbx_import::Format::of(&file).is_none() {
            return self.reimport_report(Err(format!("{}: not a 3D file", file.display())), cx);
        }
        let id = match id_of(&self.dom, target) {
            Some(id) => id,
            None => {
                let id = store::new_id();
                self.push_history();
                let _ = set_id(&mut self.dom, target, Some(&id));
                let changes = self.dom.take_changes();
                self.reflect_changes(&changes, cx);
                self.record_history_change(changes);
                id
            }
        };
        let mut store = Store::load();
        store.set(&id, file.clone());
        let saved = store
            .save()
            .map(|()| format!("Reimports from {}", file.display()));
        if let Some(watch) = self.reimports.watched.get_mut(&target) {
            *watch = Watch::new(file);
        }
        self.reimport_report(saved, cx);
    }

    /// Remove reimport config: the id off the instance (an undo step) and
    /// its file off this device's map.
    pub(super) fn remove_reimport_config(&mut self, target: Ref, cx: &mut Context<Self>) {
        let Some(id) = id_of(&self.dom, target) else {
            return;
        };
        self.reimports.watched.remove(&target);
        self.push_history();
        let _ = set_id(&mut self.dom, target, None);
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        let mut store = Store::load();
        store.remove(&id);
        if let Err(err) = store.save() {
            self.output.push(SOURCE, Feedback::Warning(err));
        }
        cx.notify();
    }

    /// Auto-Reimport: on (reimporting at once) or off again.
    pub(super) fn toggle_auto_reimport(&mut self, target: Ref, cx: &mut Context<Self>) {
        if self.reimports.watched.remove(&target).is_some() {
            cx.notify();
            return;
        }
        let file =
            id_of(&self.dom, target).and_then(|id| Store::load().file(&id).map(Path::to_path_buf));
        let Some(file) = file else {
            return;
        };
        self.reimports.watched.insert(target, Watch::new(file));
        self.reimport(target, cx);
        if !std::mem::replace(&mut self.reimports.polling, true) {
            self.poll_reimports(cx);
        }
    }

    /// Polls the watched files until none is left.
    fn poll_reimports(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| loop {
            cx.background_executor().timer(POLL_INTERVAL).await;
            let going = shell.update(cx, |shell, cx| shell.check_watched(cx));
            if !matches!(going, Ok(true)) {
                break;
            }
        })
        .detach();
    }

    /// One poll: reimports what settled since it changed, and forgets what
    /// was deleted. False once nothing is watched.
    fn check_watched(&mut self, cx: &mut Context<Self>) -> bool {
        let dom = &self.dom;
        self.reimports
            .watched
            .retain(|&target, _| dom.get(target).is_some());
        let busy = &self.reimports.busy;
        let due: Vec<Ref> = self
            .reimports
            .watched
            .iter_mut()
            .filter_map(|(&target, watch)| {
                let now = fingerprint([watch.file.as_path()]);
                watch.step(now, busy.contains(&target)).then_some(target)
            })
            .collect();
        for target in due {
            self.reimport(target, cx);
        }
        let going = !self.reimports.watched.is_empty();
        self.reimports.polling = going;
        going
    }

    fn reimport_report(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        let feedback = match result {
            Ok(message) => Feedback::Output(message),
            Err(message) => Feedback::Error(message),
        };
        self.output.push(SOURCE, feedback.clone());
        self.command_bar.set_feedback(feedback);
        cx.notify();
    }
}
