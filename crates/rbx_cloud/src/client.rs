//! Low-level HTTP plumbing shared by every endpoint module. Endpoint modules
//! (`introspect`, `universes`, `games`, `assets`, `publish`) each add
//! `impl Client` blocks in their own file rather than growing this one.

use std::time::Duration;

use crate::api_key::ApiKey;
use crate::error::CloudError;
use crate::retry;

/// Exactly this, no parameters: see [`Client::send_json`].
const JSON: &str = "application/json";

const USER_AGENT: &str = concat!("rbx-native/", env!("CARGO_PKG_VERSION"));

/// Blocking client for the Roblox Cloud + legacy public APIs. Cheap to clone:
/// the underlying `ureq::Agent` pools connections behind an `Arc`.
#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    api_key: Option<ApiKey>,
    /// Asset-delivery roots (anonymous, keyed) a test points at a local
    /// server.
    #[cfg(test)]
    pub(crate) asset_roots: Option<(String, String)>,
    apis_root: String,
}

/// Verbs `send_json` supports; GETs go through `get_with`, which retries them.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    Post,
    Patch,
    Delete,
}

/// A response reduced to what every endpoint needs to make its own success/
/// error decision: the crate disables ureq's built-in "4xx/5xx is an error"
/// behavior so callers can distinguish e.g. a 401 (retry with a key) from a
/// 404 (give up) from a 429 (surface retry-after).
pub(crate) struct RawResponse {
    pub status: u16,
    pub headers: ureq::http::HeaderMap,
    pub body: Vec<u8>,
}

type GetBuilder = ureq::RequestBuilder<ureq::typestate::WithoutBody>;

impl Client {
    pub fn new(api_key: Option<ApiKey>) -> Self {
        let config = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .build();
        Client {
            agent: config.into(),
            api_key,
            #[cfg(test)]
            asset_roots: None,
            apis_root: "https://apis.roblox.com".to_string(),
        }
    }

    /// Points the `apis.roblox.com` calls at another server (a local mock).
    pub fn with_apis_root(mut self, root: impl Into<String>) -> Self {
        self.apis_root = root.into();
        self
    }

    #[allow(dead_code)]
    pub(crate) fn apis_url(&self, path: &str) -> String {
        format!("{}{}", self.apis_root, path)
    }

    /// A keyed write. Only a 429 is re-sent: a throttled write was not
    /// applied, whereas any other failure may have been.
    ///
    /// The body goes out as bare `application/json`: ureq's `send_json` adds
    /// `; charset=utf-8`, which the memory store queue endpoint refuses with
    /// "Content type must be JSON for this request." (HTTP 400).
    pub(crate) fn send_json(
        &self,
        method: Method,
        url: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<RawResponse, CloudError> {
        let what = url.split('?').next().unwrap_or(url);
        let bytes = body.map(serde_json::to_vec).transpose()?;
        retry::rate_limited(what, || {
            let key = self.require_api_key()?.as_str();
            let result = match (method, &bytes) {
                (Method::Delete, _) => self.agent.delete(url).header("x-api-key", key).call(),
                (Method::Post, Some(b)) => self
                    .agent
                    .post(url)
                    .header("x-api-key", key)
                    .content_type(JSON)
                    .send(&b[..]),
                (Method::Post, None) => self.agent.post(url).header("x-api-key", key).send_empty(),
                (Method::Patch, Some(b)) => self
                    .agent
                    .patch(url)
                    .header("x-api-key", key)
                    .content_type(JSON)
                    .send(&b[..]),
                (Method::Patch, None) => {
                    self.agent.patch(url).header("x-api-key", key).send_empty()
                }
            };
            run(result)
        })
    }

    pub(crate) fn require_api_key(&self) -> Result<&ApiKey, CloudError> {
        self.api_key.as_ref().ok_or(CloudError::NoApiKey)
    }

    pub(crate) fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    pub(crate) fn get_raw(
        &self,
        url: &str,
        with_key: bool,
        follow_redirects: bool,
    ) -> Result<RawResponse, CloudError> {
        self.get_with(url, with_key, follow_redirects, |req| req)
    }

    /// Like [`get_raw`](Self::get_raw), but lets the caller add query
    /// parameters through ureq's own builder so values (e.g. an opaque
    /// pagination cursor) are percent-encoded correctly.
    ///
    /// A GET is safe to repeat, so a rate limit or a dropped connection is
    /// retried here rather than surfaced — see [`retry`]. `configure` is
    /// therefore `Fn`: it builds a fresh request per attempt.
    pub(crate) fn get_with(
        &self,
        url: &str,
        with_key: bool,
        follow_redirects: bool,
        configure: impl Fn(GetBuilder) -> GetBuilder,
    ) -> Result<RawResponse, CloudError> {
        // The key is never part of `what`, and the query string is dropped:
        // the retry line it may end up in goes to stderr.
        let what = url.split('?').next().unwrap_or(url);
        retry::idempotent(what, || {
            let mut req = self.agent.get(url);
            if with_key {
                req = req.header("x-api-key", self.require_api_key()?.as_str());
            }
            req = configure(req);
            if !follow_redirects {
                req = req.config().max_redirects(0).build();
            }
            run(req.call())
        })
    }

    pub(crate) fn post_json_raw<T: serde::Serialize>(
        &self,
        url: &str,
        with_key: bool,
        body: &T,
    ) -> Result<RawResponse, CloudError> {
        let mut req = self.agent.post(url);
        if with_key {
            req = req.header("x-api-key", self.require_api_key()?.as_str());
        }
        run(req.send_json(body))
    }

    /// A keyed PATCH with a JSON body. Not retried: Open Cloud's update calls
    /// are not idempotent once a field mask clears a value.
    pub(crate) fn patch_json_raw<T: serde::Serialize>(
        &self,
        url: &str,
        body: &T,
    ) -> Result<RawResponse, CloudError> {
        let req = self
            .agent
            .patch(url)
            .header("x-api-key", self.require_api_key()?.as_str());
        run(req.send_json(body))
    }

    pub(crate) fn post_bytes_raw(
        &self,
        url: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<RawResponse, CloudError> {
        // A place upload can be up to 100 MB, so the agent's 30-second
        // whole-call budget would fail any large place on a slow line.
        // Connecting and Roblox's answer keep their own limits; sending the
        // body gets as long as it takes.
        // ponytail: no body-send limit, a stalled socket waits for TCP to
        // give up; add one sized from `bytes.len()` if that bites.
        let req = self
            .agent
            .post(url)
            .header("x-api-key", self.require_api_key()?.as_str())
            .content_type(content_type)
            .config()
            .timeout_global(None)
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(300)))
            .timeout_recv_body(Some(Duration::from_secs(30)))
            .build();
        run(req.send(bytes))
    }
}

fn run(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<RawResponse, CloudError> {
    let mut response = result?;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = response
        .body_mut()
        .read_to_vec()
        .map_err(|err| CloudError::Transport(err.to_string()))?;
    Ok(RawResponse {
        status,
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_client_without_key_reports_no_api_key_on_require() {
        let client = Client::new(None);
        assert!(!client.has_api_key());
        assert!(matches!(
            client.require_api_key(),
            Err(CloudError::NoApiKey)
        ));
    }

    #[test]
    fn new_client_with_key_reports_it_present() {
        let client = Client::new(Some(ApiKey::new("secret")));
        assert!(client.has_api_key());
        assert_eq!(client.require_api_key().unwrap().as_str(), "secret");
    }

    #[test]
    fn user_agent_includes_crate_name_and_version() {
        assert!(USER_AGENT.starts_with("rbx-native/"));
        assert!(USER_AGENT.len() > "rbx-native/".len());
    }
    #[test]
    fn a_json_write_names_bare_application_json() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let root = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Read the whole request (head and body) before replying: closing a
            // socket with unread bytes makes Windows reset the connection.
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request).to_lowercase();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}");
            String::from_utf8_lossy(&request).to_lowercase()
        });
        let client = Client::new(Some(ApiKey::new("k"))).with_apis_root(root.clone());
        let body = serde_json::json!({"data": 1});
        let response = client
            .send_json(Method::Post, &format!("{root}/q"), Some(&body))
            .unwrap();
        assert_eq!(response.status, 200);
        let head = seen.join().unwrap();
        assert!(
            head.contains("content-type: application/json\r\n"),
            "{head}"
        );
    }
}
