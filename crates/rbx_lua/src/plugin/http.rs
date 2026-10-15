//! `HttpService` requests, run on worker threads so a slow server never
//! blocks the editor.

use std::sync::mpsc::{Receiver, Sender};

use super::api::PluginId;
use super::scheduler::AwaitId;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HttpRequest {
    pub(crate) url: String,
    /// Upper-case, `GET` by default.
    pub(crate) method: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HttpResponse {
    /// 2xx.
    pub(crate) success: bool,
    pub(crate) status_code: u16,
    pub(crate) status_message: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: String,
}

pub(crate) type Finished = (PluginId, AwaitId, Result<HttpResponse, String>);

/// In-flight requests. `Default` makes the channel lazily on first use.
#[derive(Default)]
pub(crate) struct Http {
    pub(crate) channel: Option<(Sender<Finished>, Receiver<Finished>)>,
    pub(crate) in_flight: usize,
}

impl Http {
    /// Sends `request` on a new thread (ureq, 30 s timeout, any status
    /// returned as a response rather than an error; only transport failures
    /// are `Err`). Its result comes back from [`Http::poll`].
    pub(crate) fn start(&mut self, plugin: PluginId, id: AwaitId, request: HttpRequest) {
        todo!()
    }

    /// Every request finished since the last call.
    pub(crate) fn poll(&mut self) -> Vec<Finished> {
        todo!()
    }

    pub(crate) fn busy(&self) -> bool {
        self.in_flight > 0
    }
}

/// The host part of a URL (`https://api.example.com:8080/x` →
/// `api.example.com`), lower-cased; `None` for anything not http(s).
pub(crate) fn host_of(url: &str) -> Option<String> {
    todo!()
}
