//! The asset and content-generation tools: searching the Creator Store and
//! an inventory, inserting an asset by id, uploading images, and the AI
//! generation tools, whose backend Roblox keeps to Studio.
//!
//! Everything here reaches Roblox through Open Cloud with the editor's API
//! key, so it runs off the UI thread: a lookup as [`Session::deferred`]
//! work, an insert as [`Session::inserting`], whose download the shell
//! grafts into the place.

mod generate;
mod images;
mod insert;
mod search;

#[cfg(test)]
mod tests;

use rbx_cloud::{ApiKey, Client, CloudError};
use serde_json::Value;

use super::Outcome;
use crate::mcp::Session;

pub(crate) use insert::{inserted, toolbox_fetch, Insertion};

/// `RBX_STUDIO_MCP_ASSETS_MOCK=<http root>` sends every call to a local
/// stand-in for `apis.roblox.com`, for captures and tests without a key.
const MOCK_VARIABLE: &str = "RBX_STUDIO_MCP_ASSETS_MOCK";

/// `None` for a tool this module does not answer.
pub(super) fn run(name: &str, arguments: &Value, session: &mut Session) -> Option<Outcome> {
    Some(match name {
        search::NAME => search::run(arguments, session),
        insert::NAME => insert::run(arguments, session),
        images::UPLOAD => images::upload(arguments, session),
        images::STORE => images::store(arguments),
        generate::MESH | generate::MATERIAL | generate::PROCEDURAL => generate::refuse(name),
        generate::SEGMENT | generate::TEXTURE => {
            generate::mesh_edit(name, session.dom, &session.selection, arguments)
        }
        generate::WAIT => generate::wait(arguments),
        _ => return None,
    })
}

/// A client for the editor's key, or for the mock root.
fn client() -> Result<Client, String> {
    if let Some(root) = mock_root() {
        return Ok(Client::new(Some(ApiKey::new("mock")))
            .with_apis_root(root.clone())
            .with_asset_delivery_roots(
                format!("{root}/asset-delivery/v1/asset"),
                format!("{root}/asset-delivery-api/v1/assetId"),
            ));
    }
    let key = ApiKey::from_env_or_config().ok_or(
        "No Open Cloud API key is set up, and every asset tool calls Roblox with one: add it \
         from Home \u{203a} Manage key.",
    )?;
    Ok(Client::new(Some(key)))
}

fn mock_root() -> Option<String> {
    #[cfg(test)]
    if let Some(root) = tests::MOCK_ROOT.with(|root| root.borrow().clone()) {
        return Some(root);
    }
    std::env::var(MOCK_VARIABLE).ok()
}

/// The key owner, whom an upload belongs to and whose inventory is "mine".
fn key_owner(client: &Client) -> Result<u64, String> {
    client
        .introspect()
        .map(|info| info.authorized_user_id)
        .map_err(|err| describe(&err, "asset:read"))
}

/// A cloud failure in words that say what to change; `scope` is the
/// permission the call needed.
fn describe(err: &CloudError, scope: &str) -> String {
    match err {
        CloudError::Refused {
            status: 401,
            message,
        } if message.contains("Invalid API Key") => {
            format!(
                "Roblox doesn\u{2019}t accept the stored API key: it has expired, been revoked \
                 or was mistyped. Create a new one and add it from Home \u{203a} Manage key. \
                 ({err})"
            )
        }
        CloudError::Http {
            status: 401 | 403, ..
        }
        | CloudError::Refused {
            status: 401 | 403, ..
        }
        | CloudError::MissingScope { .. }
        | CloudError::KeyedAssetRefused { .. } => format!(
            "Roblox refused the API key: it needs the {scope} permission (Home \u{203a} Manage \
             key). ({err})"
        ),
        _ => err.to_string(),
    }
}

/// A random version 4 UUID, as Studio's image handles and insert tags carry.
fn uuid() -> String {
    let mut bytes = [0u8; 16];
    // A handle only has to be unique in this session; a failed read of the
    // OS source still leaves the time to tell handles apart.
    if getrandom::fill(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        bytes = nanos.to_le_bytes();
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}
