//! Turns the asset references a DOM carries into decoded images or parsed
//! meshes.
//!
//! This is where `rbx_assets` (cache, native Studio content, decoding) meets
//! `rbx_cloud` (the network): the orphan rule puts the [`AssetFetcher`] impl
//! bridging the two here rather than in either crate.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use rbx_assets::{
    decode_image, AssetCache, AssetFetcher, AssetRef, AssetResolver, FetchError, NativeContent,
};
use rbx_cloud::{ApiKey, Client};

/// Roblox allows 3000 asset requests a minute; six in flight keeps a place with
/// a few dozen textures fast while staying an order of magnitude below that.
const WORKERS: usize = 6;

/// A decoded image, tightly packed RGBA8, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Image {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

impl Image {
    /// Whether any pixel is less than fully opaque, which decides if the quads
    /// using it can be drawn in the opaque pass.
    pub(crate) fn has_alpha(&self) -> bool {
        self.pixels.iter().skip(3).step_by(4).any(|&a| a != u8::MAX)
    }
}

struct CloudFetcher(Client);

impl AssetFetcher for CloudFetcher {
    fn fetch_id(&self, id: u64) -> Result<Vec<u8>, FetchError> {
        self.0
            .asset(id)
            .map(|content| content.bytes)
            .map_err(|err| FetchError::Other {
                id,
                message: err.to_string(),
            })
    }
}

/// Resolves and decodes every reference into an [`Image`], skipping the ones
/// that fail.
///
/// Failures are warnings, not errors: a texture that will not download leaves
/// its face bare, which is a far better outcome than refusing to open the file.
/// Returned alongside the map so a caller with somewhere to show them (the
/// Output dock) can, without changing what already goes to stderr.
pub(crate) fn load(references: &[AssetRef]) -> (HashMap<AssetRef, Image>, Vec<String>) {
    load_with("textures", references, fetch_image)
}

/// Resolves and parses every reference into a [`rbx_mesh::Mesh`], skipping the
/// ones that fail (v6/v7 files, a network error, a corrupt download, ...).
///
/// Failures are warnings, not errors: `Scene::resolve_file_meshes` leaves the
/// affected `MeshPart`/`SpecialMesh` drawing its fallback box.
pub(crate) fn load_meshes(
    references: &[AssetRef],
) -> (HashMap<AssetRef, rbx_mesh::Mesh>, Vec<String>) {
    load_with("meshes", references, fetch_mesh)
}

/// Resolves every reference to its raw bytes, for assets whose format the
/// scene decodes itself (legacy union assets are `.rbxm` files).
pub(crate) fn load_bytes(references: &[AssetRef]) -> (HashMap<AssetRef, Vec<u8>>, Vec<String>) {
    load_with("unions", references, |resolver, reference| {
        resolver
            .resolve(reference)
            .map(|asset| asset.bytes)
            .map_err(|err| err.to_string())
    })
}

/// Shared worker-pool machinery behind [`load`], [`load_meshes`] and
/// [`load_bytes`]: same
/// bounded concurrency, same disk cache, same warn-and-skip failure handling —
/// only what a resolved [`Asset`](rbx_assets::Asset) turns into differs.
///
/// The `Vec<String>` returned alongside the map is the same text already
/// `eprintln!`'d, for a caller (`Loaded::from_dom`, ultimately the Output
/// dock) that wants to show it somewhere besides stderr; the CLI's stderr
/// output is unchanged either way.
fn load_with<T: Send>(
    label: &str,
    references: &[AssetRef],
    decode: impl Fn(&AssetResolver, &AssetRef) -> Result<T, String> + Sync,
) -> (HashMap<AssetRef, T>, Vec<String>) {
    if references.is_empty() {
        return (HashMap::new(), Vec::new());
    }

    let resolver = match resolver() {
        Ok(resolver) => resolver,
        Err(err) => {
            let message = format!("rbxview: no {label} ({err})");
            eprintln!("{message}");
            return (HashMap::new(), vec![message]);
        }
    };

    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results = Mutex::new(HashMap::new());
    // Held back until the progress line is finished, so a warning never lands
    // in the middle of it.
    let warnings = Mutex::new(Vec::new());

    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(references.len()) {
            scope.spawn(|| {
                while let Some(reference) = references.get(next.fetch_add(1, Ordering::Relaxed)) {
                    match decode(&resolver, reference) {
                        Ok(value) => {
                            results
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(reference.clone(), value);
                        }
                        Err(warning) => warnings
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push(warning),
                    }
                    progress(
                        label,
                        done.fetch_add(1, Ordering::Relaxed) + 1,
                        references.len(),
                    );
                }
            });
        }
    });
    eprintln!();

    let warnings = warnings.into_inner().unwrap_or_else(|e| e.into_inner());
    for warning in &warnings {
        eprintln!("warning: {warning}");
    }
    (
        results.into_inner().unwrap_or_else(|e| e.into_inner()),
        warnings,
    )
}

fn resolver() -> Result<AssetResolver, String> {
    let cache = AssetCache::new(None).map_err(|err| err.to_string())?;
    let native = NativeContent::new(cache.native_packages_dir());
    let client = Client::new(ApiKey::from_env_or_config());

    Ok(AssetResolver::new(
        cache,
        Box::new(CloudFetcher(client)),
        native,
    ))
}

fn fetch_image(resolver: &AssetResolver, reference: &AssetRef) -> Result<Image, String> {
    let failed = |err: &dyn std::fmt::Display| format!("{}: {err}", describe(reference));

    let asset = resolver.resolve(reference).map_err(|err| failed(&err))?;
    let decoded = decode_image(&asset).map_err(|err| failed(&err))?;

    let (width, height) = decoded.dimensions();
    Ok(Image {
        width,
        height,
        pixels: decoded.into_raw(),
    })
}

fn fetch_mesh(resolver: &AssetResolver, reference: &AssetRef) -> Result<rbx_mesh::Mesh, String> {
    let failed = |err: &dyn std::fmt::Display| format!("{}: {err}", describe(reference));

    let asset = resolver.resolve(reference).map_err(|err| failed(&err))?;
    rbx_mesh::parse(&asset.bytes).map_err(|err| failed(&err))
}

fn progress(label: &str, done: usize, total: usize) {
    eprint!("\rrbxview: {label} {done}/{total}");
}

fn describe(reference: &AssetRef) -> String {
    match reference {
        AssetRef::Id(id) => format!("asset {id}"),
        AssetRef::Native(path) => format!("rbxasset://{path}"),
        AssetRef::Thumb(_) => "thumbnail reference".to_string(),
        AssetRef::Empty => "empty reference".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(alpha: u8) -> Image {
        Image {
            width: 1,
            height: 1,
            pixels: vec![10, 20, 30, alpha],
        }
    }

    #[test]
    fn only_a_non_opaque_pixel_makes_an_image_translucent() {
        assert!(!image(255).has_alpha());
        assert!(image(254).has_alpha());
        assert!(image(0).has_alpha());
    }

    #[test]
    fn alpha_is_read_from_the_fourth_byte_of_every_pixel() {
        // Three opaque pixels whose colour bytes include 0: only the alpha
        // lanes may be inspected.
        let opaque = Image {
            width: 3,
            height: 1,
            pixels: vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255],
        };
        assert!(!opaque.has_alpha());
    }

    #[test]
    fn every_reference_kind_describes_itself_without_a_url() {
        assert_eq!(describe(&AssetRef::Id(42)), "asset 42");
        assert_eq!(
            describe(&AssetRef::Native("sky/sun.jpg".to_string())),
            "rbxasset://sky/sun.jpg"
        );
        assert!(!describe(&AssetRef::Thumb("id=1".to_string())).contains("id=1"));
    }

    #[test]
    fn an_empty_reference_list_needs_no_cache_and_no_network() {
        let (images, warnings) = load(&[]);
        assert!(images.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn load_with_collects_warnings_instead_of_only_printing_them() {
        let references = vec![AssetRef::Id(1), AssetRef::Id(2)];
        let (results, warnings) =
            load_with(
                "things",
                &references,
                |_resolver, reference| match reference {
                    AssetRef::Id(1) => Ok(1u32),
                    _ => Err(format!("{}: boom", describe(reference))),
                },
            );

        assert_eq!(results.get(&AssetRef::Id(1)), Some(&1));
        assert_eq!(results.len(), 1);
        assert_eq!(warnings, vec!["asset 2: boom".to_string()]);
    }

    #[test]
    fn load_with_returns_nothing_for_an_empty_reference_list() {
        let (results, warnings) = load_with::<u32>("things", &[], |_resolver, _reference| Ok(0));
        assert!(results.is_empty());
        assert!(warnings.is_empty());
    }
}
