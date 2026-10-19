//! Runs the confirmed ordered and memory-store writes.

use gpui_kit::*;

use super::super::state::Pending;
use super::super::{fetch, DataStoresWindow, Field};

impl DataStoresWindow {
    /// Runs the ordered and memory writes; any other pending is handed back.
    pub(in super::super) fn run_pending_more(
        &mut self,
        pending: Pending,
        cx: &mut Context<Self>,
    ) -> Option<Pending> {
        let Some(universe) = self.universe_id() else {
            return match pending {
                Pending::SaveOrdered { .. }
                | Pending::IncrementOrdered { .. }
                | Pending::DeleteOrdered { .. }
                | Pending::SaveMapItem { .. }
                | Pending::DeleteMapItem { .. }
                | Pending::AddQueueItem { .. }
                | Pending::ReadQueue { .. }
                | Pending::DiscardQueue { .. } => None,
                other => Some(other),
            };
        };
        match pending {
            Pending::SaveOrdered {
                at,
                key,
                value,
                creating,
            } => {
                let verb = if creating { "Created" } else { "Saved" };
                let done = format!("{verb} \u{201c}{key}\u{201d}.");
                self.more_write(
                    done,
                    move || {
                        let client = fetch::client()?;
                        if creating {
                            client.create_ordered(&at, &key, value)
                        } else {
                            client.update_ordered(&at, &key, value)
                        }
                        .map(|_| ())
                        .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| this.load_ordered(false, cx),
                );
            }
            Pending::IncrementOrdered { at, key, amount } => {
                let done = format!("Incremented \u{201c}{key}\u{201d} by {amount}.");
                self.more_write(
                    done,
                    move || {
                        fetch::client()?
                            .increment_ordered(&at, &key, amount)
                            .map(|_| ())
                            .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| this.load_ordered(false, cx),
                );
            }
            Pending::DeleteOrdered { at, key } => {
                let done = format!("Deleted \u{201c}{key}\u{201d}.");
                self.more_write(
                    done,
                    move || {
                        fetch::client()?
                            .delete_ordered(&at, &key)
                            .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| {
                        this.fill(Field::OrderedKey, "");
                        this.fill(Field::OrderedValue, "");
                        this.load_ordered(false, cx);
                    },
                );
            }
            Pending::SaveMapItem {
                map,
                key,
                write,
                creating,
            } => {
                let verb = if creating { "Created" } else { "Saved" };
                let done = format!("{verb} \u{201c}{key}\u{201d}.");
                self.more_write(
                    done,
                    move || {
                        let client = fetch::client()?;
                        if creating {
                            client.create_map_item(universe, &map, &key, &write)
                        } else {
                            client.update_map_item(universe, &map, &key, &write)
                        }
                        .map(|_| ())
                        .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| {
                        this.map.creating = None;
                        this.load_map(false, cx);
                    },
                );
            }
            Pending::DeleteMapItem { map, key } => {
                let done = format!("Deleted \u{201c}{key}\u{201d}.");
                self.more_write(
                    done,
                    move || {
                        fetch::client()?
                            .delete_map_item(universe, &map, &key)
                            .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| this.load_map(false, cx),
                );
            }
            Pending::AddQueueItem {
                queue,
                data,
                priority,
                ttl_seconds,
            } => {
                let done = format!("Added an item to \u{201c}{queue}\u{201d}.");
                self.more_write(
                    done,
                    move || {
                        fetch::client()?
                            .add_queue_item(universe, &queue, &data, priority, ttl_seconds)
                            .map(|_| ())
                            .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| {
                        this.queue.adding = false;
                        cx.notify();
                    },
                );
            }
            Pending::ReadQueue {
                queue,
                count,
                invisibility_seconds,
            } => {
                self.more_write(
                    "Read the queue.".to_string(),
                    move || {
                        fetch::client()?
                            .read_queue(universe, &queue, count, invisibility_seconds)
                            .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, read, cx| {
                        let n = read.items.len();
                        let text = if n == 1 {
                            "Read 1 item.".to_string()
                        } else {
                            format!("Read {n} items.")
                        };
                        this.more_say(true, text);
                        this.queue.read = Some(read);
                        this.debug_step(cx);
                    },
                );
            }
            Pending::DiscardQueue {
                queue,
                read_id,
                count,
            } => {
                let done = if count == 1 {
                    "Discarded 1 item.".to_string()
                } else {
                    format!("Discarded {count} items.")
                };
                self.more_write(
                    done,
                    move || {
                        fetch::client()?
                            .discard_queue_items(universe, &queue, &read_id)
                            .map_err(|e| fetch::problem(&e))
                    },
                    cx,
                    |this, (), cx| {
                        this.queue.read = None;
                        cx.notify();
                    },
                );
            }
            other => return Some(other),
        }
        None
    }
}
