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
        let sender = self.channel.get_or_insert_with(std::sync::mpsc::channel).0.clone();
        self.in_flight += 1;
        std::thread::spawn(move || {
            let result = send(&request);
            // The receiver is gone only when the host was dropped.
            let _ = sender.send((plugin, id, result));
        });
    }

    /// Every request finished since the last call.
    pub(crate) fn poll(&mut self) -> Vec<Finished> {
        let mut done = Vec::new();
        if let Some((_, receiver)) = &self.channel {
            while let Ok(finished) = receiver.try_recv() {
                done.push(finished);
            }
        }
        self.in_flight = self.in_flight.saturating_sub(done.len());
        done
    }

    pub(crate) fn busy(&self) -> bool {
        self.in_flight > 0
    }
}

/// The host part of a URL (`https://api.example.com:8080/x` →
/// `api.example.com`), lower-cased; `None` for anything not http(s).
pub(crate) fn host_of(url: &str) -> Option<String> {
    let lower = url.trim().to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or("");
    let host = if authority.starts_with('[') {
        authority.split_once(']').map(|(h, _)| format!("{h}]"))?
    } else {
        authority.split(':').next().unwrap_or("").to_string()
    };
    (!host.is_empty()).then_some(host)
}

fn send(request: &HttpRequest) -> Result<HttpResponse, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent(concat!("rbx-native/", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .into();
    let mut builder = ureq::http::Request::builder()
        .method(request.method.as_str())
        .uri(request.url.as_str());
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let prepared = builder
        .body(request.body.clone().unwrap_or_default())
        .map_err(|e| e.to_string())?;
    let mut response = agent.run(prepared).map_err(|e| e.to_string())?;
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| Some((name.as_str().to_string(), value.to_str().ok()?.to_string())))
        .collect();
    let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    Ok(HttpResponse {
        success: status.is_success(),
        status_code: status.as_u16(),
        status_message: status.canonical_reason().unwrap_or("").to_string(),
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_of_strips_scheme_port_and_path() {
        assert_eq!(host_of("https://API.Example.com:8080/x?y").as_deref(), Some("api.example.com"));
        assert_eq!(host_of("http://u:p@a.b/").as_deref(), Some("a.b"));
        assert_eq!(host_of("ftp://a.b"), None);
        assert_eq!(host_of("https:///x"), None);
    }
}
