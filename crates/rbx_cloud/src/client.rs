//! Low-level HTTP plumbing shared by every endpoint module. Endpoint modules
//! (`introspect`, `universes`, `games`, `assets`, `publish`) each add
//! `impl Client` blocks in their own file rather than growing this one.

use std::time::Duration;

use crate::api_key::ApiKey;
use crate::error::CloudError;
use crate::retry;

const USER_AGENT: &str = concat!("rbx-native/", env!("CARGO_PKG_VERSION"));

/// Blocking client for the Roblox Cloud + legacy public APIs. Cheap to clone:
/// the underlying `ureq::Agent` pools connections behind an `Arc`.
#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    api_key: Option<ApiKey>,
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
        }
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

    pub(crate) fn post_bytes_raw(
        &self,
        url: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<RawResponse, CloudError> {
        let req = self
            .agent
            .post(url)
            .header("x-api-key", self.require_api_key()?.as_str())
            .content_type(content_type);
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
}
