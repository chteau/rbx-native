//! `upload_image` and `store_image`, with Studio's arguments and answers.
//!
//! `upload_image` makes each web image a `Decal` asset through Open Cloud's
//! Assets API (`POST /assets/v1/assets`, scopes `asset:read` and
//! `asset:write`), owned by the key's user; a decal upload's id is the image
//! itself, so `rbxassetid://<id>` works in any image property.
//!
//! `store_image` uploads nothing: as in Studio, it keeps a local image for
//! this editor session under an `IMAGEID_<uuid>` handle that other tools
//! take as an attached image.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use rbx_cloud::Client;
use serde_json::{json, Map, Value};

use super::super::{text_argument, Outcome};
use super::{client, describe, key_owner, uuid};
use crate::mcp::Session;

pub(super) const UPLOAD: &str = "upload_image";
pub(super) const STORE: &str = "store_image";

const UPLOAD_SCOPE: &str = "asset:read and asset:write";
const DESCRIPTION: &str = "Uploaded by rbx-native.";
/// Every upload runs at once; past this many, Roblox's rate limits would
/// make the batch outlast the client's wait for an answer.
const MAX_BATCH: usize = 20;
/// Studio's own limit for a stored image.
const MAX_STORED_BYTES: u64 = 5 * 1024 * 1024;
/// Handles kept; the oldest is forgotten past this, bounding the memory.
const MAX_STORED: usize = 32;
const HANDLE_PREFIX: &str = "IMAGEID_";

/// `store_image`'s images, oldest first.
static STORED: Mutex<VecDeque<(String, Vec<u8>)>> = Mutex::new(VecDeque::new());

pub(super) fn upload(arguments: &Value, session: &mut Session) -> Outcome {
    let Some(paths) = arguments.get("imagePaths").and_then(Value::as_array) else {
        return Err("missing the `imagePaths` argument: a list of image URLs".to_owned());
    };
    let paths: Vec<String> = paths
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    if paths.len() > MAX_BATCH {
        return Err(format!(
            "at most {MAX_BATCH} images per call: every upload waits on Roblox's processing"
        ));
    }
    // Studio answers an empty batch with an empty list.
    if paths.is_empty() {
        return Ok(Value::String("[]".to_owned()));
    }
    session.deferred = Some(Box::new(move || upload_all(&paths)));
    Ok(Value::Null)
}

pub(super) fn store(arguments: &Value) -> Outcome {
    let path = text_argument(arguments, "filePath").unwrap_or_default();
    let refuse = |why: &str| Err(format!("Error loading image: {why}"));
    // Studio reads the file before it looks at the name: a missing `.gif`
    // is unreadable, an existing one is the wrong kind.
    let size = match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() > 0 => meta.len(),
        _ => return refuse(&format!("File is empty or could not be read: {path}")),
    };
    let extension = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !["png", "jpg", "jpeg"].contains(&extension.as_str()) {
        return refuse(&format!(
            "Unsupported image file extension \".{extension}\". Supported: jpg, jpeg, png."
        ));
    }
    if size > MAX_STORED_BYTES {
        return refuse(&format!("File is larger than 5MB: {path}"));
    }
    let Ok(bytes) = std::fs::read(path) else {
        return refuse(&format!("File is empty or could not be read: {path}"));
    };
    let handle = format!("{HANDLE_PREFIX}{}", uuid());
    let mut stored = STORED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if stored.len() == MAX_STORED {
        stored.pop_front();
    }
    stored.push_back((handle.clone(), bytes));
    Ok(Value::String(handle))
}

/// Every image at once: each upload mostly waits on Roblox's processing.
fn upload_all(paths: &[String]) -> Result<Value, String> {
    let client = client()?;
    let owner = key_owner(&client)?;
    let outcomes: Vec<Result<u64, String>> = std::thread::scope(|scope| {
        let workers: Vec<_> = paths
            .iter()
            .map(|path| {
                let client = &client;
                scope.spawn(move || upload_one(client, owner, path))
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|_| Err("upload crashed".to_owned()))
            })
            .collect()
    });
    let answer: Map<String, Value> = paths
        .iter()
        .zip(outcomes)
        .map(|(path, outcome)| {
            let value = match outcome {
                Ok(id) => format!("rbxassetid://{id}"),
                Err(why) => format!("Error Upload: {why}"),
            };
            (path.clone(), json!(value))
        })
        .collect();
    Ok(Value::String(Value::Object(answer).to_string()))
}

fn upload_one(client: &Client, owner: u64, path: &str) -> Result<u64, String> {
    if !(path.starts_with("http://") || path.starts_with("https://")) {
        return Err("Only http/https URLs are supported".to_owned());
    }
    let bytes = client
        .download_image(path)
        .map_err(|err| format!("Unable to read file at path: {path} ({err})"))?;
    use rbx_assets::AssetKind;
    if !matches!(
        rbx_assets::sniff(&bytes),
        AssetKind::Png | AssetKind::Jpeg | AssetKind::Bmp
    ) {
        return Err(format!("Not a PNG, JPEG or BMP image: {path}"));
    }
    client
        .create_image_asset(
            &stem(path),
            DESCRIPTION,
            owner,
            rbx_cloud::FileContent::Bytes(&bytes),
        )
        .map_err(|err| describe(&err, UPLOAD_SCOPE))
}

/// The file name without its extension or query, as the asset's name.
fn stem(location: &str) -> String {
    let path = location.split(['?', '#']).next().unwrap_or(location);
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    if stem.is_empty() {
        "Image".to_owned()
    } else {
        stem.to_owned()
    }
}
