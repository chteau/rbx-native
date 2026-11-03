//! The network half of the catalog, run off the UI thread: downloads and
//! icons.

use std::sync::Arc;

use gpui_kit::RenderImage;
use rbx_cloud::{ApiKey, Client};

/// Creator Store asset type id of a plugin.
const PLUGIN_TYPE: u32 = 38;

pub(in crate::shell) fn client() -> Client {
    Client::new(ApiKey::from_env_or_config())
}

/// Downloads `id` and checks it is a plugin; the bytes are the model file
/// exactly as Roblox serves it.
pub(super) fn download(id: u64) -> Result<Vec<u8>, String> {
    let content = client().asset(id).map_err(|err| match err {
        rbx_cloud::CloudError::AuthRequired { .. } | rbx_cloud::CloudError::NoApiKey => {
            "Roblox wants you signed in to download plugins: add an Open Cloud key from \
             Home \u{203a} Manage key."
                .to_owned()
        }
        other => other.to_string(),
    })?;
    match content.asset_type_id {
        Some(PLUGIN_TYPE) | None => Ok(content.bytes),
        Some(other) => Err(format!("Asset {id} is not a plugin (type {other}).")),
    }
}

/// Every icon URL for `ids`, then each PNG decoded; a failed one is `None`.
pub(in crate::shell) fn fetch_icons(ids: &[u64]) -> Vec<(u64, Option<Arc<RenderImage>>)> {
    let client = client();
    let urls = client.asset_icon_urls(ids).unwrap_or_default();
    // A handful of downloads at a time: one by one, the verified list's
    // icons took about ten seconds to fill in.
    let chunk = urls.len().div_ceil(6).max(1);
    let mut icons: Vec<_> = std::thread::scope(|scope| {
        let workers: Vec<_> = urls
            .chunks(chunk)
            .map(|batch| {
                let client = &client;
                scope.spawn(move || {
                    batch
                        .iter()
                        .map(|(id, url)| (*id, icon(client, url)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap_or_default())
            .collect()
    });
    for &id in ids {
        if !icons.iter().any(|(got, _)| *got == id) {
            icons.push((id, None));
        }
    }
    icons
}

fn icon(client: &Client, url: &str) -> Option<Arc<RenderImage>> {
    let png = client.download_image(url).ok()?;
    let rgba = image::load_from_memory(&png).ok()?.to_rgba8();
    let (w, h) = rgba.dimensions();
    crate::render_image::to_render_image(rgba.into_raw(), w, h)
}
