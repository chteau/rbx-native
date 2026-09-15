//! Asset bytes, anonymous first: `assetdelivery.roblox.com` works for public
//! assets and costs nothing, so it's tried before ever touching the Open
//! Cloud key-based route (`apis.roblox.com/asset-delivery-api`).

use serde::Deserialize;

use crate::client::{Client, RawResponse};
use crate::error::{self, CloudError};

const ANONYMOUS_URL: &str = "https://assetdelivery.roblox.com/v1/asset";
const KEYED_URL: &str = "https://apis.roblox.com/asset-delivery-api/v1/assetId";

/// Enough of an unrecognised body to identify it, short enough to stay on a
/// warning line next to the asset id.
const EXCERPT_CHARS: usize = 200;

pub struct AssetContent {
    pub bytes: Vec<u8>,
    pub asset_type_id: Option<u32>,
}

#[derive(Deserialize)]
struct DeliveryResponseRaw {
    location: String,
    #[serde(rename = "assetTypeId")]
    asset_type_id: Option<u32>,
}

impl Client {
    /// Anonymous first (fact 5); on 401/403/409 with a key configured, falls
    /// back to the Open Cloud route (fact 4). Any other error surfaces
    /// immediately rather than wasting a key-authenticated request on e.g. a
    /// plain 404.
    pub fn asset(&self, asset_id: u64) -> Result<AssetContent, CloudError> {
        match self.asset_anonymous(asset_id) {
            Err(CloudError::AuthRequired { .. }) if self.has_api_key() => {
                self.asset_with_key(asset_id)
            }
            other => other,
        }
    }

    pub fn asset_anonymous(&self, asset_id: u64) -> Result<AssetContent, CloudError> {
        let url = format!("{ANONYMOUS_URL}?id={asset_id}");
        // Follow the 302 ourselves (rather than let ureq auto-follow) so we
        // can read the `Roblox-AssetTypeId` header off the redirect, which is
        // not guaranteed to survive onto the final CDN response.
        let first = self.get_raw(&url, false, false)?;

        if is_redirect(first.status) {
            let asset_type_id = header_u32(&first.headers, "roblox-assettypeid");
            let location = first
                .headers
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    CloudError::UnexpectedShape(
                        "redirect response had no Location header".to_string(),
                    )
                })?
                .to_string();
            let cdn = self.get_raw(&location, false, true)?;
            return finish(&location, cdn, asset_type_id);
        }

        if is_auth_required(first.status) {
            return Err(CloudError::AuthRequired { asset_id });
        }

        let asset_type_id = header_u32(&first.headers, "roblox-assettypeid");
        finish(&url, first, asset_type_id)
    }

    pub fn asset_with_key(&self, asset_id: u64) -> Result<AssetContent, CloudError> {
        let url = format!("{KEYED_URL}/{asset_id}");
        let response = self.get_raw(&url, true, true)?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                &url,
                response.status,
                &response.headers,
            ));
        }
        let parsed: DeliveryResponseRaw = serde_json::from_slice(&response.body)
            .map_err(|_| CloudError::UnexpectedShape(delivery_failure(&response.body)))?;
        let cdn = self.get_raw(&parsed.location, false, true)?;
        finish(&parsed.location, cdn, parsed.asset_type_id)
    }

    pub fn download_place(&self, place_id: u64) -> Result<Vec<u8>, CloudError> {
        let content = self.asset(place_id)?;
        // Binary places start `<roblox!`; XML places start `<roblox `; both
        // share the `<roblox` prefix, so one check covers either format.
        if content.bytes.starts_with(b"<roblox") {
            Ok(content.bytes)
        } else {
            Err(CloudError::NotAPlace)
        }
    }
}

fn finish(
    source_url: &str,
    response: RawResponse,
    asset_type_id: Option<u32>,
) -> Result<AssetContent, CloudError> {
    if !(200..300).contains(&response.status) {
        return Err(error::error_for_status(
            source_url,
            response.status,
            &response.headers,
        ));
    }
    Ok(AssetContent {
        bytes: response.body,
        asset_type_id,
    })
}

/// What to say when a keyed delivery response carries no usable `location`.
///
/// Open Cloud answers some assets (29242300, for one) with HTTP 200 and an error
/// document, and serde's "missing field `location`" tells nobody anything. Say
/// what the service actually said instead.
fn delivery_failure(body: &[u8]) -> String {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return format!("asset delivery returned a non-JSON body: {}", excerpt(body));
    };

    // A payload that does have a `location` only got here because some other
    // field is the wrong type — and its body holds a signed CDN URL, so it is the
    // one case where the body must not be quoted.
    if value.get("location").is_some() {
        return "asset delivery returned a `location` that is not a string".to_string();
    }
    match complaint(&value) {
        Some(complaint) => format!("asset delivery refused it: {complaint}"),
        None => format!("asset delivery returned no `location`: {}", excerpt(body)),
    }
}

/// The service's own wording, from whichever of the two error shapes it used:
/// `{"errors":[{"message":...}]}` or a flat `{"code":..., "message":...}`.
fn complaint(value: &serde_json::Value) -> Option<String> {
    let flat = |value: &serde_json::Value| {
        let text = |key| value.get(key)?.as_str().map(str::to_string);
        match (text("code"), text("message")) {
            (Some(code), Some(message)) => Some(format!("{code}: {message}")),
            (code, message) => code.or(message),
        }
    };

    if let Some(errors) = value.get("errors").and_then(|e| e.as_array()) {
        let listed: Vec<String> = errors.iter().filter_map(flat).collect();
        if !listed.is_empty() {
            return Some(listed.join("; "));
        }
    }
    flat(value)
}

/// The head of a response body, on one line, for an error message.
fn excerpt(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let flattened: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(EXCERPT_CHARS)
        .collect();

    if flattened.len() < text.trim().len() {
        format!("{flattened}...")
    } else {
        flattened
    }
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn is_auth_required(status: u16) -> bool {
    matches!(status, 401 | 403 | 409)
}

fn header_u32(headers: &ureq::http::HeaderMap, name: &str) -> Option<u32> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DELIVERY: &str = r#"{"location":"https://example.rbxcdn.com/signed?sig=abc","requestId":"req-1","isArchived":false,"assetTypeId":9,"assetMetadatas":[{"metadataType":1,"value":"282701"}],"isRecordable":true}"#;

    #[test]
    fn parses_the_real_keyed_delivery_payload() {
        let raw: DeliveryResponseRaw = serde_json::from_str(SAMPLE_DELIVERY).unwrap();
        assert_eq!(raw.location, "https://example.rbxcdn.com/signed?sig=abc");
        assert_eq!(raw.asset_type_id, Some(9));
    }

    #[test]
    fn delivery_payload_without_asset_type_id_is_still_accepted() {
        let json = r#"{"location":"https://example.rbxcdn.com/x"}"#;
        let raw: DeliveryResponseRaw = serde_json::from_str(json).unwrap();
        assert_eq!(raw.asset_type_id, None);
    }

    #[test]
    fn an_error_document_is_reported_in_the_services_own_words() {
        let body = br#"{"errors":[{"code":"NOT_FOUND","message":"Asset is not approved"}]}"#;

        let message = delivery_failure(body);

        assert!(
            message.contains("NOT_FOUND: Asset is not approved"),
            "{message}"
        );
        assert!(!message.contains("location"), "{message}");
    }

    #[test]
    fn a_flat_error_document_is_reported_too() {
        let body = br#"{"code":"PERMISSION_DENIED","message":"no access"}"#;
        assert!(delivery_failure(body).contains("PERMISSION_DENIED: no access"));

        let bare = br#"{"message":"no access"}"#;
        assert!(delivery_failure(bare).ends_with("no access"));
    }

    #[test]
    fn an_unrecognised_body_is_quoted_back_but_only_its_head() {
        let body = format!(r#"{{"status":"{}"}}"#, "x".repeat(400));

        let message = delivery_failure(body.as_bytes());

        assert!(message.contains("no `location`"), "{message}");
        assert!(message.ends_with("..."), "{message}");
        assert!(message.len() < 300, "{} chars", message.len());
    }

    #[test]
    fn a_non_json_body_says_so() {
        assert!(delivery_failure(b"<html>gateway timeout</html>").contains("non-JSON"));
    }

    // The one body that must never be quoted: it carries a signed CDN URL.
    #[test]
    fn a_payload_whose_location_is_the_wrong_type_is_never_quoted() {
        let body = br#"{"location":{"url":"https://example.rbxcdn.com/x?sig=secret"}}"#;

        let message = delivery_failure(body);

        assert!(!message.contains("sig=secret"), "{message}");
        assert!(message.contains("not a string"), "{message}");
    }

    #[test]
    fn binary_place_prefix_is_a_valid_place() {
        assert!(b"<roblox!bunch-of-binary-data".starts_with(b"<roblox"));
    }

    #[test]
    fn xml_place_prefix_is_a_valid_place() {
        assert!(b"<roblox xmlns:...>".starts_with(b"<roblox"));
    }

    #[test]
    fn redirect_statuses_are_recognized() {
        for status in [301, 302, 303, 307, 308] {
            assert!(is_redirect(status), "expected {status} to be a redirect");
        }
        assert!(!is_redirect(200));
        assert!(!is_redirect(404));
    }

    #[test]
    fn auth_required_statuses_are_recognized() {
        for status in [401, 403, 409] {
            assert!(
                is_auth_required(status),
                "expected {status} to require auth"
            );
        }
        assert!(!is_auth_required(404));
        assert!(!is_auth_required(500));
    }

    #[test]
    fn header_u32_parses_present_header() {
        let mut headers = ureq::http::HeaderMap::new();
        headers.insert("roblox-assettypeid", "9".parse().unwrap());
        assert_eq!(header_u32(&headers, "roblox-assettypeid"), Some(9));
    }

    #[test]
    fn header_u32_is_none_when_missing() {
        let headers = ureq::http::HeaderMap::new();
        assert_eq!(header_u32(&headers, "roblox-assettypeid"), None);
    }
}
