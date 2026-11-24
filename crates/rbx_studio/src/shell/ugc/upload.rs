//! The upload itself: the checked item as a new `Model` asset, and the
//! Creator Hub page to finish it on.
//!
//! For screenshots without a key: `RBX_STUDIO_UGC_UPLOAD_MOCK_ERROR=<text>`
//! fails after one second with that text, and `RBX_STUDIO_UGC_UPLOAD_MOCK_ID=<id>`
//! succeeds after a second and a half with that asset id.

use std::time::Duration;

use rbx_cloud::{FileContent, ModelFile, Owner};

use super::super::freeze::{authorize, describe};

/// What goes up: the selected tree as a binary `.rbxm`, or a source file.
pub(super) enum Payload {
    Rbxm(Vec<u8>),
    File(rbx_import::Upload),
}

/// Who the asset belongs to: the key's own user, or one of their groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Creator {
    Me,
    Group(u64),
}

/// Blocking. Uploads `payload` as a `Model` named `title` and returns the
/// new asset id.
pub(super) fn send(
    payload: &Payload,
    title: &str,
    description: &str,
    creator: Creator,
) -> Result<u64, String> {
    if let Ok(message) = std::env::var("RBX_STUDIO_UGC_UPLOAD_MOCK_ERROR") {
        std::thread::sleep(Duration::from_secs(1));
        return Err(message);
    }
    if let Ok(id) = std::env::var("RBX_STUDIO_UGC_UPLOAD_MOCK_ID") {
        std::thread::sleep(Duration::from_millis(1500));
        return id
            .trim()
            .parse()
            .map_err(|_| format!("RBX_STUDIO_UGC_UPLOAD_MOCK_ID is not an asset id: {id}"));
    }
    let (client, user) = authorize()?;
    send_with(&client, user, payload, title, description, creator)
}

pub(super) fn send_with(
    client: &rbx_cloud::Client,
    user: u64,
    payload: &Payload,
    title: &str,
    description: &str,
    creator: Creator,
) -> Result<u64, String> {
    let owner = match creator {
        Creator::Me => Owner::User(user),
        Creator::Group(id) => Owner::Group(id),
    };
    let file = match payload {
        Payload::Rbxm(bytes) => ModelFile {
            name: "item.rbxm",
            content_type: "model/x-rbxm",
            content: FileContent::Bytes(bytes),
        },
        Payload::File(upload) => ModelFile {
            name: &upload.file_name,
            content_type: upload.content_type,
            content: FileContent::Bytes(&upload.bytes),
        },
    };
    client
        .create_model_asset(title, description, owner, &file)
        .map_err(|e| describe(&e))
}

/// The Creator Hub page of asset `id`.
pub(super) fn creator_hub_url(id: u64) -> String {
    format!("https://create.roblox.com/dashboard/creations/store/{id}/configure")
}

#[cfg(test)]
#[path = "upload/tests.rs"]
mod tests;
