//! Save and Cancel: the local edits land in the place as one undo step,
//! the cloud ones and the sign-in switches go to Roblox together.

use gpui_kit::*;

use super::model::{self, LocalDraft};
use super::{extras, fetch, team_create, GameSettingsWindow, Notice};

/// The notice for a save that sent edits to Roblox, composed from what happened.
fn save_notice(wrote_local: bool, refused: Option<&str>) -> Notice {
    let (ok, text) = match (wrote_local, refused) {
        (false, None) => (true, "Saved to Roblox.".to_string()),
        (true, None) => (true, "Saved to this place and to Roblox.".to_string()),
        (false, Some(why)) => (false, why.to_string()),
        (true, Some(why)) => (false, format!("Saved to this place; Roblox refused: {why}")),
    };
    Notice { ok, text }
}

impl GameSettingsWindow {
    /// Writes the local edits to the place as one undo step, then sends the
    /// cloud ones; the notice says what landed.
    pub(super) fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving || !self.invalid.is_empty() {
            return;
        }
        let mut wrote_local = false;
        if self.local.is_dirty() {
            let writes = self.local.writes();
            let (wrote, reread) = self.shell.update(cx, |shell, cx| {
                let mut targets = Vec::new();
                for (field, value) in writes {
                    match model::service(&shell.dom, field.class) {
                        Some(target) => targets.push((target, field.property, value)),
                        None => shell
                            .output
                            .push_warning(&format!("This place has no {}.", field.class)),
                    }
                }
                let wrote = !targets.is_empty();
                if wrote {
                    shell.push_history();
                    for (target, property, value) in targets {
                        if let Err(e) = shell.dom.set_property(target, property, value) {
                            shell
                                .output
                                .push_warning(&format!("Couldn\u{2019}t set {property}: {e}"));
                        }
                    }
                    let changes = shell.dom.take_changes();
                    shell.reflect_changes(&changes, cx);
                    shell.record_history_change(changes);
                }
                (wrote, LocalDraft::read(&shell.dom, &shell.database))
            });
            self.local = reread;
            wrote_local = wrote;
        }
        let pending = match (&self.link, &self.cloud.remote) {
            (Some(link), Some(remote)) if self.cloud.is_dirty() => {
                Some((link.clone(), remote.clone(), self.cloud.updates()))
            }
            _ => None,
        };
        let team_create = self.team_create.pending(self.link.as_ref());
        let extras = self
            .extras
            .pending(self.team_create.web.as_ref(), self.link.as_ref());
        self.reseed = true;
        match (pending, team_create, extras) {
            (None, None, None) if wrote_local => {
                self.notice = Some(Notice {
                    ok: true,
                    text: "Saved to this place.".to_string(),
                });
            }
            (None, None, None) => {}
            (pending, team_create, extras) => {
                self.saving = true;
                self.notice = None;
                self.generation += 1;
                self.team_create.generation += 1;
                self.extras.generation += 1;
                let generations = (
                    self.generation,
                    self.team_create.generation,
                    self.extras.generation,
                );
                cx.spawn(async move |this, cx| {
                    let (cloud, team_create, extras) = cx
                        .background_spawn(async move {
                            (
                                pending.map(|(link, remote, updates)| {
                                    fetch::save(&link, &remote, &updates)
                                }),
                                team_create.as_ref().map(team_create::save),
                                extras.as_ref().map(extras::save),
                            )
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if generations
                            != (
                                this.generation,
                                this.team_create.generation,
                                this.extras.generation,
                            )
                        {
                            return;
                        }
                        this.saving = false;
                        // Rebased, not reset: edits typed while the request was
                        // out stay pending, and so do parts that did not land.
                        let mut refusals = Vec::new();
                        match cloud {
                            Some(Ok(next)) => this.cloud.rebase(next),
                            Some(Err((landed, text))) => {
                                this.cloud.rebase(*landed);
                                refusals.push(text);
                            }
                            None => {}
                        }
                        if let Some(saved) = team_create {
                            this.team_create.saved(saved, &mut refusals);
                        }
                        if let Some(saved) = extras {
                            this.extras.saved(saved, &mut refusals);
                        }
                        let refusals = refusals.join(" ");
                        this.notice = Some(save_notice(
                            wrote_local,
                            (!refusals.is_empty()).then_some(refusals.as_str()),
                        ));
                        this.reseed = true;
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }

    /// Drops every edit and refills the inputs.
    pub(super) fn revert(&mut self, cx: &mut Context<Self>) {
        if let Some(remote) = self.cloud.remote.clone() {
            self.cloud.reset(remote);
        }
        self.team_create.revert();
        self.extras.revert();
        let local = {
            let s = self.shell.read(cx);
            LocalDraft::read(&s.dom, &s.database)
        };
        self.local = local;
        self.invalid.clear();
        self.notice = None;
        self.reseed = true;
        cx.notify();
    }
}
