//! `POST /universes/v1/{universe}/places/{place}/versions`: upload a place
//! file as a new saved or published version.
//!
//! Never call [`Client::publish_place`] from a test or ad-hoc script — it
//! mutates a real experience. This module is exercised only through
//! request-construction/response-parsing unit tests.

use serde::Deserialize;

use crate::client::Client;
use crate::error::{self, CloudError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishMode {
    Saved,
    Published,
}

impl PublishMode {
    fn as_query_value(self) -> &'static str {
        match self {
            PublishMode::Saved => "Saved",
            PublishMode::Published => "Published",
        }
    }
}

#[derive(Deserialize)]
struct PublishResponseRaw {
    #[serde(rename = "versionNumber")]
    version_number: u64,
}

impl Client {
    pub fn publish_place(
        &self,
        universe_id: u64,
        place_id: u64,
        bytes: &[u8],
        mode: PublishMode,
    ) -> Result<u64, CloudError> {
        let url = format!(
            "https://apis.roblox.com/universes/v1/{universe_id}/places/{place_id}/versions?versionType={}",
            mode.as_query_value()
        );
        let response = self.post_bytes_raw(&url, content_type_for(bytes), bytes)?;
        if !(200..300).contains(&response.status) {
            return Err(error::error_for_status(
                &url,
                response.status,
                &response.headers,
            ));
        }
        let parsed: PublishResponseRaw = serde_json::from_slice(&response.body)?;
        Ok(parsed.version_number)
    }
}

/// Roblox distinguishes a binary `.rbxl` from an XML `.rbxlx` by
/// `Content-Type`, not by file extension (we only ever see bytes here). The
/// binary magic (`<roblox!`) is unambiguous; anything starting like XML gets
/// the XML content type, everything else defaults to binary.
fn content_type_for(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"<roblox!") {
        "application/octet-stream"
    } else if bytes.starts_with(b"<?xml") || bytes.starts_with(b"<roblox ") {
        "application/xml"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_publish_response() {
        let raw: PublishResponseRaw = serde_json::from_str(r#"{"versionNumber": 42}"#).unwrap();
        assert_eq!(raw.version_number, 42);
    }

    #[test]
    fn publish_mode_query_values_match_the_api() {
        assert_eq!(PublishMode::Saved.as_query_value(), "Saved");
        assert_eq!(PublishMode::Published.as_query_value(), "Published");
    }

    #[test]
    fn binary_place_gets_octet_stream() {
        assert_eq!(
            content_type_for(b"<roblox!binarydata"),
            "application/octet-stream"
        );
    }

    #[test]
    fn xml_place_gets_application_xml() {
        assert_eq!(content_type_for(b"<roblox xmlns:..."), "application/xml");
        assert_eq!(
            content_type_for(b"<?xml version=\"1.0\"?>"),
            "application/xml"
        );
    }

    #[test]
    fn unrecognized_bytes_default_to_octet_stream() {
        assert_eq!(content_type_for(b"garbage"), "application/octet-stream");
    }
}
