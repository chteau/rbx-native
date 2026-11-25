//! The network half of `http_get`: which URLs it may reach, and reading one
//! as text. Redirects are followed by hand so every hop is held to the same
//! allowlist as the first URL.

use std::io::Read;
use std::time::Duration;

use url::Url;

const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_REDIRECTS: usize = 5;
/// The largest engine class page (`BasePart.md`) is about 300 KB; anything
/// past this is not documentation.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// What a fetch returned: the body as text and its declared content type.
pub(crate) struct Fetched {
    pub(crate) body: String,
    pub(crate) content_type: String,
}

/// Where `http_get` may go: Roblox's documentation site and the source it is
/// built from. A host with no path prefix is reachable only as a redirect
/// target of an allowed URL, such as a GitHub release download.
const ALLOWED: &[(&str, &str)] = &[
    ("create.roblox.com", "/docs/"),
    ("raw.githubusercontent.com", "/Roblox/creator-docs/"),
    ("github.com", "/Roblox/creator-docs/"),
    ("github.com", "/Roblox/libmp/releases/download/"),
];

/// Hosts GitHub hands release assets and raw files out from.
const REDIRECT_ONLY: &[&str] = &[
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "raw.githubusercontent.com",
];

/// `url` parsed, or why `http_get` refuses it.
pub(crate) fn allowed(url: &str) -> Result<Url, String> {
    let parsed = Url::parse(url.trim()).map_err(|err| format!("`{url}` is not a URL: {err}"))?;
    if is_allowed(&parsed) {
        Ok(parsed)
    } else {
        Err(format!(
            "URL not allowed: {url}. Allowed: https://create.roblox.com/docs/ (Engine API \
             reference, guides, Cloud API, llms.txt indexes) and Roblox's creator-docs \
             repository on GitHub"
        ))
    }
}

fn is_allowed(url: &Url) -> bool {
    plain_https(url)
        && ALLOWED
            .iter()
            .any(|(host, prefix)| url.host_str() == Some(host) && url.path().starts_with(prefix))
}

fn plain_https(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
}

fn redirect_allowed(url: &Url) -> bool {
    is_allowed(url)
        || (plain_https(url)
            && REDIRECT_ONLY
                .iter()
                .any(|host| url.host_str() == Some(host)))
}

/// Reads `url`, which [`allowed`] accepted.
pub(crate) fn get(url: &Url) -> Result<Fetched, String> {
    let agent: ureq::Agent = rbx_net::config()
        .timeout_global(Some(TIMEOUT))
        .max_redirects(0)
        .max_redirects_will_error(false)
        .http_status_as_error(false)
        .build()
        .into();
    let mut current = url.clone();
    for _ in 0..=MAX_REDIRECTS {
        let mut response = agent
            .get(current.as_str())
            .header("User-Agent", "rbx-native")
            .header("Accept", "text/markdown, text/plain, text/html;q=0.8")
            .call()
            .map_err(|err| format!("could not fetch {current}: {err}"))?;
        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get("Location")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| format!("{current} redirected without a Location"))?;
            let next = current
                .join(location)
                .map_err(|err| format!("{current} redirected to a bad URL: {err}"))?;
            if !redirect_allowed(&next) {
                return Err(format!(
                    "{current} redirected outside the allowed sites: {next}"
                ));
            }
            current = next;
            continue;
        }
        if !status.is_success() {
            return Err(format!("HTTP {} fetching {current}", status.as_u16()));
        }
        let content_type = response
            .headers()
            .get("Content-Type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let mut bytes = Vec::new();
        response
            .body_mut()
            .with_config()
            .limit(MAX_BYTES)
            .reader()
            .read_to_end(&mut bytes)
            .map_err(|err| format!("could not read {current}: {err}"))?;
        return Ok(Fetched {
            body: String::from_utf8_lossy(&bytes).into_owned(),
            content_type,
        });
    }
    Err(format!("{url} redirected more than {MAX_REDIRECTS} times"))
}
