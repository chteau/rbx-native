//! `POST /assets/v1/assets` and `GET /assets/v1/operations/{id}`: upload a
//! new asset through the key, then wait for Roblox to finish processing it.
//!
//! Two types are wrapped: `Decal` for PNGs (the Figma import's images) and
//! `Model` for geometry. `Model` is the one route the Assets API opens
//! to geometry nobody downloaded from Roblox first (`creator-docs`,
//! `cloud/guides/usage-assets.md`): a `Mesh` upload "only accepts content
//! downloaded from the Asset delivery API", while a `Model` takes `.fbx` or
//! glTF and is imported as a `Model` holding one `MeshPart` per mesh, each
//! with a freshly created mesh asset of its own.
//!
//! Never call [`Client::create_model_asset`] from a test — it creates a real
//! asset in the key owner's inventory. Request construction and response
//! parsing are unit-tested instead; the one live check is `#[ignore]`d.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::client::Client;
use crate::error::{self, CloudError};

const CREATE_URL: &str = "https://apis.roblox.com/assets/v1/assets";

/// How long processing may take before giving up. Roblox documents no
/// bound; a one-mesh model is usually done within a few seconds.
const PROCESSING_LIMIT: Duration = Duration::from_secs(120);

/// A file for [`Client::create_model_asset`], with the content type the
/// guide's table pairs with it (`model/gltf+json`, `model/fbx`, …).
pub struct ModelFile<'a> {
    pub name: &'a str,
    pub content_type: &'a str,
    pub bytes: &'a [u8],
}

impl Client {
    /// Uploads `file` as a new `Model` asset owned by `user_id` and waits for
    /// Roblox to process it. Returns the new asset's id.
    pub fn create_model_asset(
        &self,
        display_name: &str,
        description: &str,
        user_id: u64,
        file: &ModelFile,
    ) -> Result<u64, CloudError> {
        self.create_asset("Model", display_name, description, user_id, file)
    }

    /// Uploads a PNG, JPEG or BMP (told apart by their first bytes, since a
    /// Figma image fill comes back in whatever format went in) as a new
    /// `Decal` asset owned by `user_id` and waits for
    /// Roblox to process it. Returns the new asset's id, used as
    /// `rbxassetid://<id>` in `ImageLabel.Image`; the creator docs do not
    /// say in as many words that a decal upload's id is the image itself,
    /// which `image_round_trip` checks live.
    pub fn create_image_asset(
        &self,
        display_name: &str,
        description: &str,
        user_id: u64,
        image: &[u8],
    ) -> Result<u64, CloudError> {
        let (name, content_type) = Self::image_kind(image);
        let file = ModelFile {
            name,
            content_type,
            bytes: image,
        };
        self.create_asset("Decal", display_name, description, user_id, &file)
    }

    /// Unknown bytes go up as PNG and Roblox's own check names the problem.
    fn image_kind(bytes: &[u8]) -> (&'static str, &'static str) {
        match bytes {
            [0xFF, 0xD8, 0xFF, ..] => ("image.jpg", "image/jpeg"),
            [b'B', b'M', ..] => ("image.bmp", "image/bmp"),
            _ => ("image.png", "image/png"),
        }
    }

    fn create_asset(
        &self,
        asset_type: &str,
        display_name: &str,
        description: &str,
        user_id: u64,
        file: &ModelFile,
    ) -> Result<u64, CloudError> {
        let boundary = boundary(file.bytes);
        let body = multipart(
            asset_type,
            display_name,
            description,
            user_id,
            file,
            &boundary,
        );
        let content_type = format!("multipart/form-data; boundary={boundary}");
        let response = self.post_bytes_raw(CREATE_URL, &content_type, &body)?;
        let operation = answer(CREATE_URL, response)?;

        let started = Instant::now();
        let mut delay = Duration::from_millis(500);
        let mut current = operation;
        loop {
            if let Some(done) = finished(&current)? {
                return Ok(done);
            }
            if started.elapsed() > PROCESSING_LIMIT {
                return Err(CloudError::UnexpectedShape(format!(
                    "Roblox was still processing the upload after {}s",
                    PROCESSING_LIMIT.as_secs()
                )));
            }
            std::thread::sleep(delay);
            delay = (delay * 2).min(Duration::from_secs(5));
            let url = operation_url(&current)?;
            current = answer(&url, self.get_raw(&url, true, true)?)?;
        }
    }
}

/// A 2xx body as JSON; anything else as an error carrying Roblox's own words
/// where the body has them.
pub(crate) fn answer(url: &str, response: crate::client::RawResponse) -> Result<Value, CloudError> {
    if (200..300).contains(&response.status) {
        return Ok(serde_json::from_slice(&response.body)?);
    }
    let said = serde_json::from_slice::<Value>(&response.body)
        .ok()
        .as_ref()
        .and_then(crate::assets::complaint);
    match said {
        Some(message) if response.status != 429 => Err(CloudError::Refused {
            status: response.status,
            message,
        }),
        _ => Err(error::error_for_status(
            url,
            response.status,
            &response.headers,
        )),
    }
}

/// `None` while the operation runs; the asset id once it is done.
fn finished(operation: &Value) -> Result<Option<u64>, CloudError> {
    if !operation
        .get("done")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(None);
    }
    if let Some(error) = operation.get("error").filter(|e| !e.is_null()) {
        let message = crate::assets::complaint(error).unwrap_or_else(|| error.to_string());
        return Err(CloudError::Refused {
            status: error.get("code").and_then(Value::as_u64).unwrap_or(0) as u16,
            message,
        });
    }
    let response = operation.get("response").unwrap_or(&Value::Null);
    let state = response
        .pointer("/moderationResult/moderationState")
        .and_then(Value::as_str)
        .unwrap_or("");
    // The guide's sample says `MODERATION_STATE_APPROVED`, the schema
    // `Approved`: match the word, not the spelling.
    if state.to_ascii_lowercase().contains("rejected") {
        return Err(CloudError::Refused {
            status: 0,
            message: "Roblox moderation rejected the upload".to_string(),
        });
    }
    // The schema says int64, the guide's sample a string: take either.
    let id = match response.get("assetId") {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    };
    id.map(Some)
        .ok_or_else(|| CloudError::UnexpectedShape("finished upload names no assetId".to_string()))
}

fn operation_url(operation: &Value) -> Result<String, CloudError> {
    let id = operation
        .get("operationId")
        .and_then(Value::as_str)
        .or_else(|| {
            operation
                .get("path")
                .and_then(Value::as_str)
                .and_then(|path| path.strip_prefix("operations/"))
        })
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        .ok_or_else(|| CloudError::UnexpectedShape("upload answered no operation".to_string()))?;
    Ok(format!("https://apis.roblox.com/assets/v1/operations/{id}"))
}

/// The `request` and `fileContent` fields the endpoint takes, as
/// `multipart/form-data`.
fn multipart(
    asset_type: &str,
    display_name: &str,
    description: &str,
    user_id: u64,
    file: &ModelFile,
    boundary: &str,
) -> Vec<u8> {
    let request = json!({
        "assetType": asset_type,
        "displayName": display_name,
        "description": description,
        "creationContext": { "creator": { "userId": user_id.to_string() } },
    });
    // A header value cannot hold a quote or a line break.
    let name: String = file
        .name
        .chars()
        .filter(|c| !matches!(c, '"' | '\r' | '\n'))
        .collect();
    let mut body = Vec::with_capacity(file.bytes.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"request\"\r\n\
             Content-Type: application/json\r\n\r\n{request}\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"fileContent\"; filename=\"{name}\"\r\n\
             Content-Type: {}\r\n\r\n",
            file.content_type
        )
        .as_bytes(),
    );
    body.extend_from_slice(file.bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// A boundary that does not occur in `bytes`, as RFC 2046 requires.
fn boundary(bytes: &[u8]) -> String {
    (0u32..)
        .map(|n| format!("rbx-native-{n:08x}"))
        .find(|candidate| {
            !bytes
                .windows(candidate.len())
                .any(|window| window == candidate.as_bytes())
        })
        .expect("some boundary is absent from any finite body")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_carries_the_request_then_the_file() {
        let file = ModelFile {
            name: "freeze\".gltf",
            content_type: "model/gltf+json",
            bytes: b"{}",
        };
        let body = multipart("Model", "Name", "Desc", 42, &file, "B");
        let text = String::from_utf8(body).unwrap();
        assert!(text.starts_with("--B\r\nContent-Disposition: form-data; name=\"request\""));
        assert!(text.contains("\"assetType\":\"Model\""));
        assert!(text.contains("\"userId\":\"42\""));
        assert!(text.contains(
            "filename=\"freeze.gltf\"\r\nContent-Type: model/gltf+json\r\n\r\n{}\r\n--B--\r\n"
        ));
    }

    #[test]
    fn image_bytes_are_sniffed() {
        assert_eq!(Client::image_kind(b"\xFF\xD8\xFF\xE0jfif").1, "image/jpeg");
        assert_eq!(Client::image_kind(b"BM...").1, "image/bmp");
        assert_eq!(Client::image_kind(b"\x89PNG\r\n").1, "image/png");
    }

    #[test]
    fn an_image_goes_up_as_a_png_decal() {
        let file = ModelFile {
            name: "image.png",
            content_type: "image/png",
            bytes: b"png",
        };
        let body = multipart("Decal", "Icon", "", 7, &file, "B");
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("\"assetType\":\"Decal\""));
        assert!(text.contains("filename=\"image.png\"\r\nContent-Type: image/png\r\n\r\npng\r\n"));
    }

    /// Uploads a 1 × 1 PNG as a `Decal` through the stored key (it needs
    /// `asset:read` and `asset:write`) and checks the id it returns
    /// downloads as the image itself, not a decal model wrapping one.
    /// Creates a real asset in the key owner's inventory, hence ignored:
    /// `cargo test -p rbx_cloud image_round_trip -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn image_round_trip() {
        const PNG: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99,
            0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let client = Client::new(crate::ApiKey::from_env_or_config());
        let user = client.introspect().unwrap().authorized_user_id;
        let id = client
            .create_image_asset("rbx-native image test", "", user, PNG)
            .unwrap();
        std::thread::sleep(Duration::from_secs(5));
        let content = client.asset(id).unwrap();
        eprintln!(
            "decal upload gave asset {id}: {} bytes",
            content.bytes.len()
        );
        assert!(
            content.bytes.starts_with(b"\x89PNG"),
            "asset {id} is not the image itself"
        );
    }

    #[test]
    fn the_boundary_never_occurs_in_the_file() {
        let clash = b"xx rbx-native-00000000 yy";
        assert_eq!(boundary(clash), "rbx-native-00000001");
        assert_eq!(boundary(b""), "rbx-native-00000000");
    }

    #[test]
    fn a_running_operation_is_not_finished() {
        let running = json!({ "path": "operations/abc-123", "done": false });
        assert_eq!(finished(&running).unwrap(), None);
        assert_eq!(
            operation_url(&running).unwrap(),
            "https://apis.roblox.com/assets/v1/operations/abc-123"
        );
        let fresh = json!({ "path": "operations/x", "operationId": "abc", "done": false });
        assert!(operation_url(&fresh).unwrap().ends_with("/abc"));
    }

    #[test]
    fn an_operation_id_that_could_change_the_url_is_refused() {
        let hostile = json!({ "path": "operations/../../x?y" });
        assert!(operation_url(&hostile).is_err());
    }

    #[test]
    fn the_guides_sample_answer_gives_its_asset_id() {
        // `creator-docs`, `cloud/guides/usage-assets.md`, "Example Response
        // for Get Operation".
        let done = json!({
            "path": "operations/x",
            "done": true,
            "response": {
                "path": "assets/2205400862",
                "assetId": "2205400862",
                "assetType": "ASSET_TYPE_DECAL",
                "moderationResult": { "moderationState": "MODERATION_STATE_APPROVED" }
            }
        });
        assert_eq!(finished(&done).unwrap(), Some(2205400862));
        let numeric = json!({ "done": true, "response": { "assetId": 7 } });
        assert_eq!(finished(&numeric).unwrap(), Some(7));
    }

    #[test]
    fn a_failed_or_rejected_operation_says_why() {
        let failed = json!({ "done": true, "error": { "code": 3, "message": "bad file" } });
        assert!(matches!(
            finished(&failed),
            Err(CloudError::Refused { message, .. }) if message == "bad file"
        ));
        let rejected = json!({
            "done": true,
            "response": { "assetId": "1", "moderationResult": { "moderationState": "Rejected" } }
        });
        assert!(matches!(
            finished(&rejected),
            Err(CloudError::Refused { .. })
        ));
    }
}
