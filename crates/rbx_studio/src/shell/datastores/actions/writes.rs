//! Writes to the standard data stores: checking an edit, asking before it
//! is sent, sending it, and what follows (a conflict, the entry shown, the
//! key list).

use gpui_kit::*;

use rbx_cloud::{Entry, EntryRef, EntryWrite, Revision, ALL_SCOPES};

use super::super::state::{store_ref, Dialog, EntryPane, EntryTab, Pending, Problem};
use super::super::{fetch, json_tree, DataStoresWindow, Field};

enum Written {
    Entry(Box<Entry>),
    Deleted,
}

/// "2026-10-19T21:50:03Z" as "2026-10-19 21:50 UTC".
fn when(time: Option<&str>) -> String {
    match time.and_then(|t| t.get(..16)) {
        Some(minute) => format!("{} UTC", minute.replace('T', " ")),
        None => "an earlier time".to_string(),
    }
}

fn perform(pending: &Pending) -> Result<Written, Problem> {
    let client = fetch::client()?;
    let reply = match pending {
        Pending::SaveEntry {
            at,
            key,
            write,
            creating,
        } => {
            let saved = if *creating {
                client.create_entry(at, key, write)
            } else {
                client.update_entry(at, key, write)
            };
            saved.map(|e| Written::Entry(Box::new(e)))
        }
        Pending::DeleteEntry { at, key } => client.delete_entry(at, key).map(|()| Written::Deleted),
        Pending::IncrementEntry { at, key, amount } => {
            // An increment that omits UserIds and attributes would clear
            // them, so the current ones are sent back.
            client.get_entry(at, key).and_then(|current| {
                client
                    .increment_entry(at, key, *amount, &current.users, &current.attributes)
                    .map(|e| Written::Entry(Box::new(e)))
            })
        }
        Pending::Rollback {
            at,
            key,
            revision,
            current_etag,
        } => client
            .rollback_entry(at, key, &revision.revision_id, current_etag)
            .map(|e| Written::Entry(Box::new(e))),
        _ => {
            return Err(Problem::Other(
                "That is not a data store write.".to_string(),
            ))
        }
    };
    reply.map_err(|e| fetch::problem(&e))
}

impl DataStoresWindow {
    pub(in super::super) fn submit_edit(&mut self, cx: &mut Context<Self>) {
        match self.build_save(cx) {
            Ok(pending) => {
                if let Some(draft) = &mut self.pane.editing {
                    draft.error = None;
                }
                self.ask(pending, cx);
            }
            Err(message) => {
                if let Some(draft) = &mut self.pane.editing {
                    draft.error = Some(message);
                }
                cx.notify();
            }
        }
    }

    fn build_save(&self, cx: &App) -> Result<Pending, String> {
        let Some(draft) = &self.pane.editing else {
            return Err("Nothing is being edited.".to_string());
        };
        let (Some(universe), Some(store)) =
            (self.universe.as_ref().map(|u| u.id), self.store.as_ref())
        else {
            return Err("Pick a data store first.".to_string());
        };
        let (at, key, etag) = if draft.creating {
            let scope = self.scope_value(cx);
            if scope == ALL_SCOPES {
                return Err("Pick a scope to create the entry in.".to_string());
            }
            let key = self.inputs.get(Field::NewKey, cx).trim().to_string();
            (store_ref(universe, store, &scope), key, None)
        } else {
            let Some(entry) = &self.pane.entry else {
                return Err("Nothing is being edited.".to_string());
            };
            (
                store_ref(universe, store, &entry.scope),
                entry.key.clone(),
                Some(entry.etag.clone()),
            )
        };
        let write = EntryWrite {
            value: json_tree::parse_json(&self.inputs.get(Field::Value, cx))?,
            users: json_tree::parse_users(&self.inputs.get(Field::Users, cx))?,
            attributes: json_tree::parse_attributes(&self.inputs.get(Field::Attributes, cx))?,
            etag,
        };
        rbx_cloud::validate_entry(&key, &write)?;
        Ok(Pending::SaveEntry {
            at,
            key,
            write,
            creating: draft.creating,
        })
    }

    pub(in super::super) fn ask(&mut self, pending: Pending, cx: &mut Context<Self>) {
        self.dialog = Some(Dialog::Confirm {
            pending,
            live_ack: false,
        });
        cx.notify();
    }

    pub(in super::super) fn confirm(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Confirm { pending, live_ack }) = self.dialog.clone() else {
            return;
        };
        if self.universe.as_ref().is_some_and(|u| u.live) && !live_ack {
            return;
        }
        self.dialog = None;
        self.run_pending(pending, cx);
    }

    pub(in super::super) fn set_live_ack(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(Dialog::Confirm { live_ack, .. }) = &mut self.dialog {
            *live_ack = on;
            cx.notify();
        }
    }

    pub(in super::super) fn run_pending(&mut self, pending: Pending, cx: &mut Context<Self>) {
        let Some(pending) = self.run_pending_more(pending, cx) else {
            return;
        };
        self.writing += 1;
        cx.notify();
        let job = pending.clone();
        self.request(
            move || perform(&job),
            cx,
            move |this, reply, cx| {
                this.writing = this.writing.saturating_sub(1);
                this.written(pending, reply, cx);
                cx.notify();
            },
        );
    }

    fn written(
        &mut self,
        pending: Pending,
        reply: Result<Written, Problem>,
        cx: &mut Context<Self>,
    ) {
        match reply {
            Ok(Written::Entry(entry)) => {
                let (text, creating) = match &pending {
                    Pending::SaveEntry {
                        at, key, creating, ..
                    } => (
                        format!("Saved \u{201c}{key}\u{201d} in {}.", at.store),
                        *creating,
                    ),
                    Pending::IncrementEntry { key, .. } => (
                        format!("Incremented \u{201c}{key}\u{201d} to {}.", entry.value),
                        false,
                    ),
                    Pending::Rollback { key, revision, .. } => (
                        format!(
                            "Rolled \u{201c}{key}\u{201d} back to the version from {}.",
                            when(revision.revision_create_time.as_deref())
                        ),
                        false,
                    ),
                    _ => (String::new(), false),
                };
                if creating {
                    self.insert_created(&entry);
                }
                self.show_written(Some(*entry), cx);
                self.say(true, text);
            }
            Ok(Written::Deleted) => {
                if let Pending::DeleteEntry { at, key } = &pending {
                    self.keys
                        .items
                        .retain(|k| !(k.scope == at.scope && k.key == *key));
                    self.keys.selected = None;
                    self.say(
                        true,
                        format!("Deleted \u{201c}{key}\u{201d} from {}.", at.store),
                    );
                }
                self.show_written(None, cx);
            }
            Err(Problem::Conflict(_)) => self.fetch_newer(pending, cx),
            Err(problem) => self.say(false, fetch::describe(&problem)),
        }
    }

    /// Puts `entry` in the pane in place of whatever was there.
    fn show_written(&mut self, entry: Option<Entry>, cx: &mut Context<Self>) {
        let old = std::mem::take(&mut self.pane);
        let tab = old.tab;
        self.pane = EntryPane {
            generation: old.generation + 1,
            tab,
            view: old.view,
            collapsed: old.collapsed,
            entry,
            ..EntryPane::default()
        };
        if tab == EntryTab::Versions && self.pane.entry.is_some() {
            self.load_revisions(false, cx);
        }
    }

    fn insert_created(&mut self, entry: &Entry) {
        let at = match self
            .keys
            .items
            .iter()
            .position(|k| k.scope == entry.scope && k.key == entry.key)
        {
            Some(at) => at,
            None => {
                let at = self.keys.items.partition_point(|k| k.key < entry.key);
                self.keys.items.insert(
                    at,
                    EntryRef {
                        scope: entry.scope.clone(),
                        key: entry.key.clone(),
                    },
                );
                at
            }
        };
        self.keys.selected = Some(at);
    }

    /// A write lost to a newer version: fetch it so the dialog can show it.
    fn fetch_newer(&mut self, pending: Pending, cx: &mut Context<Self>) {
        let (at, key) = match &pending {
            Pending::SaveEntry { at, key, .. }
            | Pending::IncrementEntry { at, key, .. }
            | Pending::Rollback { at, key, .. } => (at.clone(), key.clone()),
            _ => {
                self.say(false, "It changed since it was read.".to_string());
                return;
            }
        };
        self.request(
            move || {
                fetch::client()?
                    .get_entry(&at, &key)
                    .map_err(|e| fetch::problem(&e))
            },
            cx,
            move |this, reply, cx| {
                match reply {
                    Ok(newer) => {
                        this.dialog = Some(Dialog::Conflict {
                            pending,
                            newer: Box::new(newer),
                        });
                    }
                    Err(problem) => this.say(false, fetch::describe(&problem)),
                }
                cx.notify();
            },
        );
    }

    pub(in super::super) fn overwrite_after_conflict(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Conflict { mut pending, newer }) = self.dialog.take() else {
            return;
        };
        match &mut pending {
            Pending::SaveEntry {
                write, creating, ..
            } => {
                write.etag = Some(newer.etag.clone());
                // The key exists now, so a create has to become an update.
                *creating = false;
            }
            Pending::Rollback { current_etag, .. } => *current_etag = newer.etag.clone(),
            _ => {}
        }
        self.ask(pending, cx);
    }

    pub(in super::super) fn keep_theirs(&mut self, cx: &mut Context<Self>) {
        if let Some(Dialog::Conflict { newer, .. }) = self.dialog.take() {
            let same = self
                .pane
                .entry
                .as_ref()
                .is_some_and(|e| e.scope == newer.scope && e.key == newer.key);
            if same {
                self.pane.entry = Some(*newer);
            }
            self.pane.editing = None;
        }
        cx.notify();
    }

    pub(in super::super) fn submit_increment(&mut self, cx: &mut Context<Self>) {
        let Some((at, key, _)) = self.entry_target() else {
            return;
        };
        match self.inputs.get(Field::Increment, cx).trim().parse::<i64>() {
            Ok(amount) if amount != 0 => self.ask(Pending::IncrementEntry { at, key, amount }, cx),
            _ => {
                self.say(
                    false,
                    "Enter a whole number other than 0 to increment by.".to_string(),
                );
                cx.notify();
            }
        }
    }

    pub(in super::super) fn ask_delete(&mut self, cx: &mut Context<Self>) {
        if let Some((at, key, _)) = self.entry_target() {
            self.ask(Pending::DeleteEntry { at, key }, cx);
        }
    }

    pub(in super::super) fn ask_rollback(&mut self, revision: Revision, cx: &mut Context<Self>) {
        if let Some((at, key, current_etag)) = self.entry_target() {
            self.ask(
                Pending::Rollback {
                    at,
                    key,
                    revision,
                    current_etag,
                },
                cx,
            );
        }
    }
}
