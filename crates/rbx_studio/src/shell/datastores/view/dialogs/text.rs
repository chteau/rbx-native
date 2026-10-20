//! The words of the confirm dialog: what each pending write says about
//! itself, kept apart from the drawing so tests can check them.

use rbx_cloud::StoreRef;

use crate::shell::datastores::json_tree;
use crate::shell::datastores::state::{Pending, Universe};

use super::super::common::time_label;

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
                format!("Writes “{key}” in the sorted map “{map}” of {of}."),
                verb.to_string(),
                false,
            )
        }
        Pending::DeleteMapItem { map, key } => {
            details.push(("Sorted map", map.clone()));
            details.push(("Key", key.clone()));
            (
                format!("Delete “{key}”?"),
                format!("Removes “{key}” from the sorted map “{map}” of {of}."),
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
                format!("Appends an item to the queue “{queue}” of {of}."),
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
                format!(
                    "{}, hidden for {invisibility_seconds} s",
                    plural(*count as usize, "item", "items")
                ),
            ));
            (
                format!(
                    "Read {} from “{queue}”?",
                    plural(*count as usize, "item", "items")
                ),
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
                    "Permanently removes the items you just read from the queue “{queue}” of {of}."
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
