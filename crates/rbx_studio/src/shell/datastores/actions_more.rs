//! Ordered data stores and the memory store (sorted maps and queues):
//! opening them by name, paging their contents, and the writes, which are
//! asked first like every other write.

use gpui_kit::*;

use rbx_cloud::{Page, SortKey, MAX_MEMORY_VALUE_BYTES};
use serde_json::Value;

use super::state::{Listing, Load, Problem, Status};
use super::{fetch, json_tree, DataStoresWindow};

mod memory;
mod ordered;
mod run;

const MAX_RECENT: usize = 8;
const MAX_SORT_KEY_CHARS: usize = 128;

fn remember(recent: &mut Vec<String>, name: &str) {
    recent.retain(|n| n != name);
    recent.insert(0, name.to_string());
    recent.truncate(MAX_RECENT);
}

fn fill<T>(list: &mut Listing<T>, reply: Result<Page<T>, Problem>, more: bool) -> bool {
    match reply {
        Ok(page) => {
            if more {
                list.items.extend(page.items);
            } else {
                list.items = page.items;
                list.selected = None;
            }
            list.next = page.next_page_token;
            list.load = Load::Idle;
            true
        }
        Err(problem) => {
            list.load = Load::Failed(problem);
            false
        }
    }
}

fn whole_number(text: &str, label: &str) -> Result<Option<i64>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<i64>()
        .map(Some)
        .map_err(|_| format!("{label} must be a whole number."))
}

fn sort_key(text: &str) -> Result<Option<SortKey>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if let Ok(number) = text.parse::<f64>() {
        if number.is_finite() {
            return Ok(Some(SortKey::Number(number)));
        }
    }
    if text.chars().count() > MAX_SORT_KEY_CHARS {
        return Err(format!(
            "The sort key can be at most {MAX_SORT_KEY_CHARS} characters."
        ));
    }
    Ok(Some(SortKey::Text(text.to_string())))
}

fn seconds(text: &str, label: &str, max: u64) -> Result<u64, String> {
    match text.trim().parse::<u64>() {
        Ok(n) if (1..=max).contains(&n) => Ok(n),
        _ => Err(format!("{label} must be a whole number from 1 to {max}.")),
    }
}

fn json_within_limit(text: &str) -> Result<Value, String> {
    let value = json_tree::parse_json(text)?;
    if rbx_cloud::value_bytes(&value) > MAX_MEMORY_VALUE_BYTES {
        return Err(format!(
            "The value can be at most {} KB.",
            MAX_MEMORY_VALUE_BYTES / 1024
        ));
    }
    Ok(value)
}

impl DataStoresWindow {
    fn more_request<T: Send + 'static>(
        &mut self,
        job: impl FnOnce() -> Result<T, Problem> + Send + 'static,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, Result<T, Problem>, &mut Context<Self>) + 'static,
    ) {
        self.track_request(cx);
        cx.spawn(async move |this, cx| {
            let reply = cx.background_spawn(async move { job() }).await;
            let _ = this.update(cx, move |this, cx| {
                this.finish_request(cx);
                done(this, reply, cx);
            });
        })
        .detach();
    }

    fn more_say(&mut self, ok: bool, text: String) {
        self.status = Some(Status { ok, text });
    }

    /// A write: counts as in flight, reports `success` or the problem, and
    /// runs `after` on success.
    fn more_write<T: Send + 'static>(
        &mut self,
        success: String,
        job: impl FnOnce() -> Result<T, Problem> + Send + 'static,
        cx: &mut Context<Self>,
        after: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        self.writing += 1;
        self.more_request(job, cx, move |this, reply, cx| {
            this.writing = this.writing.saturating_sub(1);
            match reply {
                Ok(value) => {
                    this.more_say(true, success);
                    after(this, value, cx);
                }
                Err(problem) => this.more_say(false, fetch::describe(&problem)),
            }
            cx.notify();
        });
    }

    fn universe_id(&self) -> Option<u64> {
        self.universe.as_ref().map(|u| u.id)
    }
}
