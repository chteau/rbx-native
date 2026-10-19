//! The plumbing every action shares: running a request off the UI thread,
//! watching for rate limiting while any are out, the status line, and the
//! scope and entry the requests are aimed at.

use std::time::Duration;

use gpui_kit::*;

use rbx_cloud::{StoreRef, ALL_SCOPES};

use super::super::state::{store_ref, Problem, Status};
use super::super::{DataStoresWindow, Field};

const POLL_EVERY: Duration = Duration::from_millis(300);

impl DataStoresWindow {
    /// Runs `job` off the UI thread, then `done` with its result.
    pub(in super::super) fn request<T: Send + 'static>(
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

    pub(in super::super) fn say(&mut self, ok: bool, text: String) {
        self.status = Some(Status { ok, text });
    }

    /// The scope field as the API wants it.
    pub(in super::super) fn scope_value(&self, cx: &App) -> String {
        let text = self.inputs.get(Field::Scope, cx);
        let text = text.trim();
        match text.to_lowercase().as_str() {
            "" => "global".to_string(),
            "-" | "*" | "all" => ALL_SCOPES.to_string(),
            _ => text.to_string(),
        }
    }

    /// Store, key and etag of the entry in the pane.
    pub(in super::super) fn entry_target(&self) -> Option<(StoreRef, String, String)> {
        let universe = self.universe.as_ref()?.id;
        let store = self.store.as_ref()?;
        let entry = self.pane.entry.as_ref()?;
        Some((
            store_ref(universe, store, &entry.scope),
            entry.key.clone(),
            entry.etag.clone(),
        ))
    }

    pub(in super::super) fn track_request(&mut self, cx: &mut Context<Self>) {
        self.in_flight += 1;
        if self.in_flight == 1 {
            self.waits_seen = rbx_cloud::rate_limit_waits();
            self._poll = Some(cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(POLL_EVERY).await;
                let alive = this.update(cx, |this, cx| {
                    let limited = rbx_cloud::rate_limit_waits() > this.waits_seen;
                    if limited != this.rate_limited {
                        this.rate_limited = limited;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }));
        }
        cx.notify();
    }

    pub(in super::super) fn finish_request(&mut self, cx: &mut Context<Self>) {
        self.in_flight = self.in_flight.saturating_sub(1);
        if self.in_flight == 0 {
            self._poll = None;
            self.rate_limited = false;
            self.waits_seen = 0;
        }
        cx.notify();
    }
}
