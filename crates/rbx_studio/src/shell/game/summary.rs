//! Save All and Publish All, with the per-place report they leave on screen.
//!
//! Save All writes every loaded place with unsaved changes, each through its
//! own Ctrl+S path. A place that was never opened in this window has no
//! edits, so there is nothing of it to write.
//!
//! Publish All asks once, then uploads every place linked to a Roblox place
//! through the same upload Publish to Roblox makes. A loaded place uploads
//! its editor's tree; one never opened uploads its file as it is on disk.

use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use rbx_cloud::PublishMode;

use crate::command_bar::Feedback;
use crate::home;
use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::workspace::GameWorkspace;

/// One place's line in the report.
pub(crate) struct Row {
    label: String,
    state: State,
}

#[derive(Clone, PartialEq)]
enum State {
    /// Publish All's question is still open: what would happen.
    Ready(String),
    Running,
    Done(String),
    Failed(String),
    Skipped(String),
}

pub(crate) struct Summary {
    title: &'static str,
    rows: Vec<Row>,
    /// Publish All before its confirmation: how many places would upload.
    confirm: Option<usize>,
    /// Kept to stay subscribed to each uploading place.
    _watching: Vec<Subscription>,
}

fn state_of(feedback: &Feedback) -> State {
    match feedback {
        Feedback::Error(text) => State::Failed(text.clone()),
        Feedback::Output(text) | Feedback::Warning(text) | Feedback::Info(text) => {
            State::Done(text.clone())
        }
        Feedback::Idle => State::Done(String::new()),
    }
}

impl GameWorkspace {
    /// File › Save All: every loaded place with unsaved changes, in order.
    pub(crate) fn save_all(&mut self, cx: &mut Context<Self>) {
        let mut rows = Vec::new();
        for (index, slot) in self.slots.iter().enumerate() {
            let label = slot.label.clone();
            let Some(shell) = slot.shell.clone() else {
                rows.push(Row {
                    label,
                    state: State::Skipped("Not opened, nothing to save".into()),
                });
                continue;
            };
            if !self.dirty(&shell, cx) {
                rows.push(Row {
                    label,
                    state: State::Skipped("No unsaved changes".into()),
                });
                continue;
            }
            shell.update(cx, |shell, cx| shell.save(cx));
            let state = if self.dirty(&shell, cx) {
                State::Failed("Not saved; the reason is in this place\u{2019}s Output".into())
            } else {
                let file = self.slots[index].path.file_name().unwrap_or_default();
                State::Done(format!("Saved to {}", file.to_string_lossy()))
            };
            rows.push(Row { label, state });
        }
        self.summary = Some(Summary {
            title: "Save All",
            rows,
            confirm: None,
            _watching: Vec::new(),
        });
        cx.notify();
    }

    /// File › Publish All: the question first, listing what each place would do.
    pub(crate) fn ask_publish_all(&mut self, cx: &mut Context<Self>) {
        if let Some(shell) = self.active_shell().cloned() {
            if shell.update(cx, |shell, cx| shell.offline_refuses("Publish All", cx)) {
                return;
            }
        }
        let rows: Vec<Row> = self
            .slots
            .iter()
            .map(|slot| Row {
                label: slot.label.clone(),
                state: match home::link_of(&slot.path) {
                    Some(link) => State::Ready(format!("Publish to place {}", link.place_id)),
                    None => State::Skipped("Not linked to a Roblox place".into()),
                },
            })
            .collect();
        let count = rows
            .iter()
            .filter(|row| matches!(row.state, State::Ready(_)))
            .count();
        self.summary = Some(Summary {
            title: "Publish All",
            rows,
            confirm: Some(count),
            _watching: Vec::new(),
        });
        cx.notify();
    }

    fn publish_all(&mut self, cx: &mut Context<Self>) {
        let Some(summary) = self.summary.as_mut() else {
            return;
        };
        summary.confirm = None;
        let mut watching = Vec::new();
        for index in 0..self.slots.len() {
            if !matches!(self.row_state(index), Some(State::Ready(_))) {
                continue;
            }
            let (path, shell) = (
                self.slots[index].path.clone(),
                self.slots[index].shell.clone(),
            );
            let Some(link) = home::link_of(&path) else {
                continue;
            };
            self.set_row(index, State::Running);
            match shell {
                Some(shell) => {
                    let seen = shell.read(cx).roblox.outcomes;
                    let started = shell.update(cx, |shell, cx| shell.publish_linked(cx));
                    if let Err(err) = started {
                        self.set_row(index, State::Failed(err));
                        continue;
                    }
                    watching.push(cx.observe(&shell, move |game, shell, cx| {
                        let shell = shell.read(cx);
                        if shell.roblox.outcomes != seen {
                            if let Some(feedback) = &shell.roblox.last_outcome {
                                game.set_row(index, state_of(feedback));
                                cx.notify();
                            }
                        }
                    }));
                }
                None => {
                    cx.spawn(async move |this, cx| {
                        let feedback = cx
                            .background_spawn(async move {
                                match std::fs::read(&path) {
                                    Ok(bytes) => super::super::roblox_publish::upload_file(
                                        &link,
                                        &bytes,
                                        PublishMode::Published,
                                    ),
                                    Err(err) => Feedback::Error(format!(
                                        "Couldn\u{2019}t read {}: {err}",
                                        path.display()
                                    )),
                                }
                            })
                            .await;
                        let _ = this.update(cx, |game, cx| {
                            game.set_row(index, state_of(&feedback));
                            cx.notify();
                        });
                    })
                    .detach();
                }
            }
        }
        if let Some(summary) = self.summary.as_mut() {
            summary._watching = watching;
        }
        cx.notify();
    }

    fn row_state(&self, index: usize) -> Option<State> {
        Some(self.summary.as_ref()?.rows.get(index)?.state.clone())
    }

    fn set_row(&mut self, index: usize, state: State) {
        if let Some(row) = self
            .summary
            .as_mut()
            .and_then(|summary| summary.rows.get_mut(index))
        {
            row.state = state;
        }
    }

    pub(super) fn summary_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let summary = self.summary.as_ref()?;
        let rows = v_flex()
            .px(px(20.))
            .pb(px(14.))
            .gap(px(6.))
            .children(summary.rows.iter().map(row_view));
        let footer = match summary.confirm {
            Some(count) => vec![
                ui::button("game-summary-cancel", "Cancel", Weight::Secondary, false)
                    .on_click(cx.listener(|game, _, _, cx| {
                        game.summary = None;
                        cx.notify();
                    }))
                    .into_any_element(),
                if count == 0 {
                    ui::disabled_button("game-summary-publish", "Publish").into_any_element()
                } else {
                    ui::button(
                        "game-summary-publish",
                        format!("Publish {count} place{}", if count == 1 { "" } else { "s" }),
                        Weight::Primary,
                        false,
                    )
                    .on_click(cx.listener(|game, _, _, cx| game.publish_all(cx)))
                    .into_any_element()
                },
            ],
            None => vec![
                ui::button("game-summary-close", "Close", Weight::Primary, false)
                    .on_click(cx.listener(|game, _, _, cx| {
                        game.summary = None;
                        cx.notify();
                    }))
                    .into_any_element(),
            ],
        };
        let text = match summary.confirm {
            Some(_) => format!(
                "Every place of {} linked to Roblox is uploaded as its new published \
                 version, as Publish to Roblox does for one place.",
                self.manifest.name
            ),
            None => format!(
                "{} \u{b7} {} places",
                self.manifest.name,
                summary.rows.len()
            ),
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .child(ui::dialog(
                    520.,
                    ui::dialog_glyph("layers", ui::accent(), ui::wash()),
                    summary.title,
                    text,
                    Some(rows.into_any_element()),
                    footer,
                ))
                .into_any_element(),
        )
    }
}

fn row_view(row: &Row) -> impl IntoElement {
    let (glyph, color, text) = match &row.state {
        State::Ready(text) => ("upload", tokens::text3(), text.clone()),
        State::Running => ("loader", tokens::text3(), "Uploading\u{2026}".to_owned()),
        State::Done(text) => ("check", ui::green(), text.clone()),
        State::Failed(text) => ("triangle-alert", ui::red(), text.clone()),
        State::Skipped(text) => ("minus", tokens::text3(), text.clone()),
    };
    h_flex()
        .gap(px(10.))
        .items_start()
        .child(
            div()
                .pt(px(2.))
                .text_color(color)
                .child(ui::icon(glyph, 13.)),
        )
        .child(
            v_flex()
                .min_w_0()
                .child(
                    ui::text(12.5, 17.)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens::text())
                        .child(row.label.clone()),
                )
                .child(ui::text(11.5, 16.).text_color(tokens::text3()).child(text)),
        )
}
