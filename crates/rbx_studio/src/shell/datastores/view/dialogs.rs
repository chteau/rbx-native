//! The confirm dialog every write goes through, and the conflict dialog
//! shown when Roblox holds a newer copy of an entry.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use rbx_cloud::StoreRef;
use serde_json::Value;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::super::json_tree;
use super::super::state::{Dialog, Pending, Universe};
use super::super::DataStoresWindow;
use super::common::time_label;

/// What the confirm dialog says about one pending write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell::datastores) struct ConfirmText {
    pub(in crate::shell::datastores) title: String,
    pub(in crate::shell::datastores) body: String,
    pub(in crate::shell::datastores) action: String,
    pub(in crate::shell::datastores) danger: bool,
    pub(in crate::shell::datastores) details: Vec<(&'static str, String)>,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn in_store(at: &StoreRef) -> String {
    format!("{} (scope {})", at.store, at.scope)
}

pub(in crate::shell::datastores) fn confirm_text(
    pending: &Pending,
    universe: &Universe,
) -> ConfirmText {
    let experience = match &universe.name {
        Some(name) => format!("{name} ({})", universe.id),
        None => format!("Universe {}", universe.id),
    };
    let of = match &universe.name {
        Some(name) => format!("{name} (universe {})", universe.id),
        None => format!("universe {}", universe.id),
    };
    let mut details: Vec<(&'static str, String)> = vec![("Experience", experience)];
    let store_rows = |label: &'static str, at: &StoreRef, key: &str| {
        vec![
            (label, at.store.clone()),
            ("Scope", at.scope.clone()),
            ("Key", key.to_string()),
        ]
    };
    let (title, body, action, danger) = match pending {
        Pending::SaveEntry {
            at,
            key,
            write,
            creating,
        } => {
            details.extend(store_rows("Data store", at, key));
            details.push((
                "Value",
                format!(
                    "{}, {}, {}",
                    json_tree::size_label(rbx_cloud::value_bytes(&write.value)),
                    plural(write.users.len(), "UserId", "UserIds"),
                    plural(write.attributes.len(), "attribute", "attributes"),
                ),
            ));
            let verb = if *creating { "Create" } else { "Save" };
            let body = if *creating {
                format!("Adds “{key}” to {} of {of}.", in_store(at))
            } else {
                format!("Overwrites “{key}” in {} of {of}.", in_store(at))
            };
            (format!("{verb} “{key}”?"), body, verb.to_string(), false)
        }
        Pending::DeleteEntry { at, key } => {
            details.extend(store_rows("Data store", at, key));
            (
                format!("Delete “{key}”?"),
                format!(
                    "Removes “{key}” from {} of {of}. Roblox keeps its versions for 30 days.",
                    in_store(at)
                ),
                "Delete".to_string(),
                true,
            )
        }
        Pending::IncrementEntry { at, key, amount } => {
            details.extend(store_rows("Data store", at, key));
            details.push(("Value", format!("{amount:+}")));
            (
                format!("Increment “{key}” by {amount}?"),
                format!("Adds {amount} to “{key}” in {} of {of}.", in_store(at)),
                "Increment".to_string(),
                false,
            )
        }
        Pending::Rollback {
            at, key, revision, ..
        } => {
            let when = time_label(revision.revision_create_time.as_deref());
            details.extend(store_rows("Data store", at, key));
            details.push(("Value", format!("Version from {when}")));
            (
                format!("Roll back “{key}”?"),
                format!(
                    "In {} of {of}. Its value, UserIds and attributes from {when} are written as a new version; nothing is lost.",
                    in_store(at)
                ),
                "Roll back".to_string(),
                false,
            )
        }
        Pending::SaveOrdered {
            at,
            key,
            value,
            creating,
        } => {
            details.extend(store_rows("Ordered data store", at, key));
            details.push(("Value", value.to_string()));
            let verb = if *creating { "Create" } else { "Save" };
            (
                format!("{verb} “{key}”?"),
                format!("Sets “{key}” to {value} in {} of {of}.", in_store(at)),
                verb.to_string(),
                false,
            )
        }
        Pending::IncrementOrdered { at, key, amount } => {
            details.extend(store_rows("Ordered data store", at, key));
            details.push(("Value", format!("{amount:+}")));
            (
                format!("Increment “{key}” by {amount}?"),
                format!("Adds {amount} to “{key}” in {} of {of}.", in_store(at)),
                "Increment".to_string(),
                false,
            )
        }
        Pending::DeleteOrdered { at, key } => {
            details.extend(store_rows("Ordered data store", at, key));
            (
                format!("Delete “{key}”?"),
                format!("Removes “{key}” from {} of {of}.", in_store(at)),
                "Delete".to_string(),
                true,
            )
        }
        Pending::SaveMapItem {
            map,
            key,
            write,
            creating,
        } => {
            details.push(("Sorted map", map.clone()));
            details.push(("Key", key.clone()));
            details.push((
                "Value",
                json_tree::size_label(rbx_cloud::value_bytes(&write.value)),
            ));
            details.push(("Expires after", format!("{} s", write.ttl_seconds)));
            let verb = if *creating { "Create" } else { "Save" };
            (
                format!("{verb} “{key}”?"),
                format!("Writes “{key}” in the sorted map {map} of {of}."),
                verb.to_string(),
                false,
            )
        }
        Pending::DeleteMapItem { map, key } => {
            details.push(("Sorted map", map.clone()));
            details.push(("Key", key.clone()));
            (
                format!("Delete “{key}”?"),
                format!("Removes “{key}” from the sorted map {map} of {of}."),
                "Delete".to_string(),
                true,
            )
        }
        Pending::AddQueueItem {
            queue,
            data,
            priority,
            ttl_seconds,
        } => {
            details.push(("Queue", queue.clone()));
            details.push((
                "Value",
                format!(
                    "{}, priority {priority}, expires after {ttl_seconds} s",
                    json_tree::size_label(rbx_cloud::value_bytes(data))
                ),
            ));
            (
                format!("Add an item to “{queue}”?"),
                format!("Appends an item to the queue {queue} of {of}."),
                "Add".to_string(),
                false,
            )
        }
        Pending::ReadQueue {
            queue,
            count,
            invisibility_seconds,
        } => {
            details.push(("Queue", queue.clone()));
            details.push((
                "Value",
                format!("{count} items, hidden for {invisibility_seconds} s"),
            ));
            (
                format!("Read {count} items from “{queue}”?"),
                format!(
                    "Reading hides the items from game servers for {invisibility_seconds} seconds in {of}. Unless you discard them, they come back when that ends."
                ),
                "Read".to_string(),
                false,
            )
        }
        Pending::DiscardQueue { queue, count, .. } => {
            details.push(("Queue", queue.clone()));
            let items = plural(*count, "queue item", "queue items");
            (
                format!("Discard {items}?"),
                format!(
                    "Permanently removes the items you just read from the queue {queue} of {of}."
                ),
                "Discard".to_string(),
                true,
            )
        }
    };
    ConfirmText {
        title,
        body,
        action,
        danger,
        details,
    }
}

fn pending_key(pending: &Pending) -> String {
    match pending {
        Pending::SaveEntry { key, .. }
        | Pending::IncrementEntry { key, .. }
        | Pending::Rollback { key, .. }
        | Pending::DeleteEntry { key, .. } => key.clone(),
        _ => String::new(),
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
                let glyph = if text.danger {
                    ui::dialog_glyph("triangle-alert", ui::red(), ui::red_soft())
                } else {
                    ui::dialog_glyph("database", ui::accent(), ui::wash())
                };
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
