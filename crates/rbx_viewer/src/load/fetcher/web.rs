//! The browser's [`Fetcher`]: the same `request`/`drain` contract as the
//! native pool, with each request a `fetch()` against `rbxview --serve` (see
//! `crate::serve`) instead of a worker thread. A page has no threads to put a
//! blocking resolver on, and could not reach Roblox's CDNs from one anyway —
//! they answer with no CORS headers — so the server resolves the bytes (disk
//! cache, anonymous delivery first, Open Cloud key after, Studio's own
//! `rbxasset://` content) and the page decodes them itself, through the very
//! decoders the native workers use.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use rbx_assets::{sniff, Asset, AssetRef};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use super::{Landed, Want};
use crate::assets::{self, Failure};

pub(crate) struct Fetcher {
    /// Where the server's `/asset` route is, with no trailing slash.
    base: Rc<str>,
    landed: Rc<RefCell<Vec<Landed>>>,
}

impl Fetcher {
    pub(crate) fn new(base: &str) -> Self {
        Fetcher {
            base: base.trim_end_matches('/').into(),
            landed: Rc::default(),
        }
    }

    pub(crate) fn request(&self, want: Want, reference: AssetRef) {
        let base = Rc::clone(&self.base);
        let landed = Rc::clone(&self.landed);
        wasm_bindgen_futures::spawn_local(async move {
            let done = resolve(&base, want, reference).await;
            landed.borrow_mut().push(done);
        });
    }

    pub(crate) fn drain(&self) -> Vec<Landed> {
        std::mem::take(&mut self.landed.borrow_mut())
    }
}

async fn resolve(base: &str, want: Want, reference: AssetRef) -> Landed {
    match want {
        Want::Image => {
            let image = image(base, &reference).await.map(Arc::new);
            Landed::Image(reference, image)
        }
        Want::Mesh => {
            let mesh = get(base, &reference)
                .await
                .and_then(|bytes| assets::mesh(&reference, &bytes))
                .map(Arc::new);
            Landed::Mesh(reference, mesh)
        }
        Want::Bytes => {
            let bytes = get(base, &reference).await;
            Landed::Bytes(reference, bytes)
        }
    }
}

/// `assets::fetch_image`, with its one hop through a decal model awaited.
async fn image(base: &str, reference: &AssetRef) -> Result<assets::Image, Failure> {
    let mut asset = asset(get(base, reference).await?);
    if let Some(inner) = assets::wrapped_image(&asset) {
        asset = self::asset(get(base, &inner).await?);
    }
    assets::image(reference, &asset)
}

fn asset(bytes: Vec<u8>) -> Asset {
    Asset {
        kind: sniff(&bytes),
        bytes,
    }
}

/// The path `crate::serve` answers `reference` on.
pub(crate) fn path(reference: &AssetRef) -> Option<String> {
    match reference {
        AssetRef::Id(id) => Some(format!("/id/{id}")),
        AssetRef::Native(path) => Some(format!(
            "/native/{}",
            String::from(js_sys::encode_uri(path))
        )),
        AssetRef::Thumb(_) | AssetRef::Empty => None,
    }
}

/// The resolved bytes of `reference`. The server's status carries the
/// native resolver's verdict: 404 for the asset's own failure (final, the
/// same as `assets::Failure`'s non-transient kind), anything else for the
/// moment's — which includes the server not running at all.
async fn get(base: &str, reference: &AssetRef) -> Result<Vec<u8>, Failure> {
    let Some(path) = path(reference) else {
        return Err(Failure {
            warning: format!("{}: not resolvable", assets::describe(reference)),
            transient: false,
        });
    };
    let url = format!("{base}{path}");
    let transient = |what: String| Failure {
        warning: format!("{}: {what}", assets::describe(reference)),
        transient: true,
    };
    let window = web_sys::window().ok_or_else(|| transient("no window".to_string()))?;
    let response = JsFuture::from(window.fetch_with_str(&url))
        .await
        .map_err(|_| {
            transient(format!(
                "could not reach {url} (is `rbxview --serve` running?)"
            ))
        })?;
    let response: web_sys::Response = response
        .dyn_into()
        .map_err(|_| transient("not a response".to_string()))?;
    let body = JsFuture::from(
        response
            .array_buffer()
            .map_err(|_| transient("no body".to_string()))?,
    )
    .await
    .map_err(|_| transient("body cut short".to_string()))?;
    let bytes = js_sys::Uint8Array::new(&body).to_vec();
    if response.ok() {
        return Ok(bytes);
    }
    // The server puts the resolver's own warning in the body.
    let message = String::from_utf8_lossy(&bytes).into_owned();
    Err(Failure {
        warning: message,
        transient: response.status() != 404,
    })
}
