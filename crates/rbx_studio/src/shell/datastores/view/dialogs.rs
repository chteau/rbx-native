//! The confirm dialog every write goes through, and the conflict dialog
//! shown when Roblox holds a newer copy of an entry.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use serde_json::Value;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::super::json_tree;
use super::super::state::{Dialog, Pending};
use super::super::DataStoresWindow;

mod text;

pub(in crate::shell::datastores) use text::confirm_text;
#[cfg(test)]
pub(in crate::shell::datastores) use text::ConfirmText;

fn pending_key(pending: &Pending) -> String {
    match pending {
        Pending::SaveEntry { key, .. }
        | Pending::IncrementEntry { key, .. }
        | Pending::Rollback { key, .. }
        | Pending::DeleteEntry { key, .. } => key.clone(),
        _ => String::new(),
    }
}

/// The icon the confirm dialog wears, so the glyph says what the button
/// will do.
fn confirm_glyph(pending: &Pending) -> (&'static str, Rgba, Rgba) {
    let (name, destructive) = match pending {
        Pending::DeleteEntry { .. }
        | Pending::DeleteOrdered { .. }
        | Pending::DeleteMapItem { .. }
        | Pending::DiscardQueue { .. } => ("trash-2", true),
        Pending::SaveEntry { creating, .. }
        | Pending::SaveOrdered { creating, .. }
        | Pending::SaveMapItem { creating, .. } => {
            (if *creating { "plus" } else { "pencil" }, false)
        }
        Pending::IncrementEntry { .. }
        | Pending::IncrementOrdered { .. }
        | Pending::AddQueueItem { .. } => ("plus", false),
        Pending::Rollback { .. } => ("undo-2", false),
        Pending::ReadQueue { .. } => ("inbox", false),
    };
    if destructive {
        (name, ui::red(), ui::red_soft())
    } else {
        (name, ui::accent(), ui::wash())
    }
}

/// At most 18 lines, so a huge value can’t push the buttons off screen.
fn clipped(value: &Value) -> String {
    let text = json_tree::pretty(value);
    let mut lines: Vec<&str> = text.lines().take(19).collect();
    if lines.len() > 18 {
        lines.truncate(18);
        lines.push("\u{2026}");
    }
    lines.join("\n")
}

fn details_box(rows: Vec<(&'static str, String)>) -> Div {
    v_flex()
        .mt(px(14.))
        .mx(px(20.))
        .ml(px(78.))
        .p(px(10.))
        .gap(px(4.))
        .rounded(px(6.))
        .border_1()
        .border_color(tokens::border())
        .bg(ui::panel2())
        .children(rows.into_iter().map(|(label, value)| {
            h_flex()
                .gap(px(12.))
                .child(
                    ui::text(12., 18.)
                        .w(px(96.))
                        .flex_none()
                        .text_color(tokens::text2())
                        .child(label),
                )
                .child(ui::text(12., 18.).min_w_0().truncate().child(value))
        }))
}

impl DataStoresWindow {
    pub(super) fn dialog_layer(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let universe = self.universe.clone()?;
        match self.dialog.clone()? {
            Dialog::Confirm { pending, live_ack } => {
                let text = confirm_text(&pending, &universe);
                let blocked = universe.live && !live_ack;
                let weight = if text.danger {
                    Weight::Danger
                } else {
                    Weight::Primary
                };
                let (name, fg, fill) = confirm_glyph(&pending);
                let glyph = ui::dialog_glyph(name, fg, fill);
                let live_box = universe.live.then(|| {
                    h_flex()
                        .mx(px(20.))
                        .ml(px(78.))
                        .mt(px(10.))
                        .p(px(10.))
                        .gap(px(10.))
                        .items_center()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(tokens::warning())
                        .child(crate::shell::rows::checkbox(
                            "ds-live-ack",
                            live_ack,
                            cx.listener(move |this, _, _, cx| this.set_live_ack(!live_ack, cx)),
                        ))
                        .child("I know this is live player data")
                });
                let body = v_flex()
                    .child(details_box(text.details.clone()))
                    .children(live_box)
                    .into_any_element();
                let cancel = ui::button("ds-dialog-cancel", "Cancel", Weight::Ghost, false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.dialog = None;
                        cx.notify();
                    }))
                    .into_any_element();
                let action = if blocked {
                    ui::disabled_button("ds-dialog-confirm-off", text.action.clone())
                        .into_any_element()
                } else {
                    ui::button("ds-dialog-confirm", text.action.clone(), weight, false)
                        .on_click(cx.listener(|this, _, _, cx| this.confirm(cx)))
                        .into_any_element()
                };
                Some(
                    ui::dialog(
                        520.,
                        glyph,
                        text.title,
                        text.body,
                        Some(body),
                        vec![cancel, action],
                    )
                    .into_any_element(),
                )
            }
            Dialog::Conflict { pending, newer } => {
                let yours = match &pending {
                    Pending::SaveEntry { write, .. } => write.value.clone(),
                    _ => Value::Null,
                };
                let column = |label: &'static str, value: &Value| {
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(4.))
                        .child(ui::text(12., 18.).text_color(tokens::text2()).child(label))
                        .child(
                            ui::mono(11.5, 17.)
                                .p(px(8.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(tokens::border())
                                .bg(ui::panel2())
                                .overflow_hidden()
                                .child(clipped(value)),
                        )
                };
                let body = h_flex()
                    .mx(px(20.))
                    .ml(px(78.))
                    .mt(px(14.))
                    .gap(px(12.))
                    .items_start()
                    .child(column("Theirs (newer)", &newer.value))
                    .child(column("Yours", &yours))
                    .into_any_element();
                let footer = vec![
                    ui::button("ds-conflict-keep", "Keep theirs", Weight::Ghost, false)
                        .on_click(cx.listener(|this, _, _, cx| this.keep_theirs(cx)))
                        .into_any_element(),
                    ui::button(
                        "ds-conflict-overwrite",
                        "Overwrite with mine\u{2026}",
                        Weight::Primary,
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.overwrite_after_conflict(cx)))
                    .into_any_element(),
                ];
                Some(
                    ui::dialog(
                        640.,
                        ui::dialog_glyph("triangle-alert", tokens::warning(), tokens::error_soft()),
                        format!("“{}” changed since you opened it", pending_key(&pending)),
                        "Roblox has a newer version, probably written by a game server. Yours was not saved.",
                        Some(body),
                        footer,
                    )
                    .into_any_element(),
                )
            }
        }
    }
}
