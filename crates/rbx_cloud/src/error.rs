//! Error type for the whole crate. Never carries an API key or a full signed
//! CDN URL (query strings are stripped before an error is built).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CloudError {
    #[error("no API key configured (set RBX_API_KEY or ~/.config/rbx-native/api_key)")]
    NoApiKey,

    #[error("asset {asset_id} requires authentication and no API key is configured")]
    AuthRequired { asset_id: u64 },

    #[error("rate limited (retry_after={retry_after:?}s)")]
    RateLimited { retry_after: Option<u64> },

    /// `url` has already had its query string stripped by [`error_for_status`].
    #[error("HTTP {status} from {url}")]
    Http { status: u16, url: String },

    #[error("network error: {0}")]
    Transport(String),

    #[error("failed to parse JSON response: {0}")]
    Json(#[from] serde_json::Error),

    #[error("downloaded content is not a Roblox place file")]
    NotAPlace,

    #[error("unexpected API response shape: {0}")]
    UnexpectedShape(String),
}

impl From<ureq::Error> for CloudError {
    fn from(err: ureq::Error) -> Self {
        // ureq's own error variants (io, timeout, protocol, tls, ...) never embed
        // the request URL, so `Display`-ing them can't leak a signed query string.
        CloudError::Transport(err.to_string())
    }
}

/// Builds a [`CloudError`] for a non-2xx response, redacting the query string
/// so a signed CDN URL never ends up in an error message or a log line.
pub(crate) fn error_for_status(
    url: &str,
    status: u16,
    headers: &ureq::http::HeaderMap,
) -> CloudError {
    if status == 429 {
        let retry_after = headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        return CloudError::RateLimited { retry_after };
    }
    CloudError::Http {
        status,
        url: redact_url(url),
    }
}

fn redact_url(url: &str) -> String {
    match url.split_once('?') {
        Some((base, _)) => format!("{base}?<redacted>"),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_url_strips_query_string() {
        assert_eq!(
            redact_url("https://cdn.example.com/asset?sig=abc123&exp=999"),
            "https://cdn.example.com/asset?<redacted>"
        );
    }

    #[test]
    fn redact_url_leaves_query_less_urls_untouched() {
        assert_eq!(
            redact_url("https://apis.roblox.com/cloud/v2/universes/1"),
            "https://apis.roblox.com/cloud/v2/universes/1"
        );
    }

    #[test]
    fn error_for_status_maps_429_to_rate_limited_with_retry_after() {
        let mut headers = ureq::http::HeaderMap::new();
        headers.insert("retry-after", "30".parse().unwrap());
        let err = error_for_status("https://example.com/x", 429, &headers);
        assert!(matches!(
            err,
            CloudError::RateLimited {
                retry_after: Some(30)
            }
        ));
    }

    #[test]
    fn error_for_status_maps_429_without_retry_after_header() {
        let headers = ureq::http::HeaderMap::new();
        let err = error_for_status("https://example.com/x", 429, &headers);
        assert!(matches!(err, CloudError::RateLimited { retry_after: None }));
    }

    #[test]
    fn error_for_status_maps_other_codes_to_http_with_redacted_url() {
        let headers = ureq::http::HeaderMap::new();
        let err = error_for_status("https://example.com/x?token=secret", 404, &headers);
        match err {
            CloudError::Http { status, url } => {
                assert_eq!(status, 404);
                assert!(!url.contains("secret"));
            }
            other => panic!("expected Http, got {other:?}"),
        }
    }
}
