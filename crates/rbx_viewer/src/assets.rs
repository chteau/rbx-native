//! Turns the asset references a DOM carries into decoded images or parsed
//! meshes.
//!
//! This is where `rbx_assets` (cache, native Studio content, decoding) meets
//! `rbx_cloud` (the network): the orphan rule puts the [`AssetFetcher`] impl
//! bridging the two here rather than in either crate.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rbx_assets::{
    decode_image, AssetCache, AssetError, AssetFetcher, AssetRef, AssetResolver, FetchError,
    NativeContent,
};
use rbx_cloud::{ApiKey, Client, CloudError};

use crate::load::Source;

/// Six in flight keeps a place with a few dozen textures fast.
///
/// The ceiling it has to respect is the Open Cloud one, which the keyed asset
/// route reports as `x-ratelimit-limit: 1000, 1000;w=60` — a thousand requests
/// a minute, shared across every key an owner holds. A private asset costs one
/// of those (the anonymous 401 and the CDN download are other hosts), and six
/// workers turning them around in a few hundred milliseconds each sit just
/// under that rate, so only a place with more than about a thousand private
/// assets — or a second tool on the same key — crosses it. Roblox publishes no
/// per-endpoint number for this route and warns that undocumented limits apply
/// besides (creator-docs `content/en-us/cloud/reference/rate-limits.md`), so
/// the defence that matters is `rbx_cloud`'s backoff on the 429, not a smaller
/// pool that would slow every load for the sake of the rare one.
pub(crate) const WORKERS: usize = 6;

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
            .map_err(|err| fetch_error(id, err))
    }
}

/// An asset that is not there to be had is a different answer from a
/// request that did not complete: the first is final (see [`Failure`]), the
/// second is worth asking again.
///
/// The line is where the status came from. A 4xx is the service's verdict on
/// this request — the asset is gone, or the key does not cover it — and the
/// next load asks with the same key and hears the same thing. A 5xx, a rate
/// limit `rbx_cloud` already backed off on four times (see its `retry`
/// module) and a transport error are all about the moment.
fn fetch_error(id: u64, err: CloudError) -> FetchError {
    match err {
        CloudError::Http {
            status: 404 | 410, ..
        } => FetchError::NotFound(id),
        // 408 is the one 4xx that says "ask again": the request timed out
        // on the way, not on its merits.
        CloudError::Http { status, .. } if (400..500).contains(&status) && status != 408 => {
            FetchError::Refused {
                id,
                message: format!("HTTP {status}"),
            }
        }
        err => FetchError::Other {
            id,
            message: err.to_string(),
        },
    }
}

/// Why a reference could not be answered, and whether asking again on a
/// later load could change that. A transient failure is the machine's, not
/// the asset's — a request that did not complete, a cache that would not
/// write, a key not yet configured — and worth one more try per load. A
/// 404, a file the content package does not hold or bytes that will not
/// decode are not: asking again cannot change the answer, and on a real
/// place the ask is the expensive part (a package scan, a round trip), so
/// `load::Resident` keeps those for good.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    pub(crate) warning: String,
    pub(crate) transient: bool,
}

impl Failure {
    fn resolving(reference: &AssetRef, err: &AssetError) -> Self {
        Failure {
            warning: format!("{}: {err}", describe(reference)),
            transient: transient(err),
        }
    }
}

/// See [`Failure`]: whether `err` says something about this machine right
/// now rather than about the asset.
fn transient(err: &AssetError) -> bool {
    match err {
        AssetError::Cache(_) | AssetError::Network(_) => true,
        // Everything `rbx_cloud` reports that is not a plain "not there":
        // transport, a rate limit, a key the next load may find configured.
        AssetError::Fetch(FetchError::Other { .. }) => true,
        AssetError::Empty
        | AssetError::Fetch(FetchError::NotFound(_))
        | AssetError::Fetch(FetchError::Refused { .. })
        | AssetError::UnknownNativePackage(..)
        | AssetError::NativeFileNotFound(_)
        | AssetError::Zip(_)
        | AssetError::ImageDecode(_)
        | AssetError::PackageTooLarge { .. } => false,
    }
}

/// What one reference turned into: the decoded value, or why it could not —
/// kept by reference so a caller that remembers results across reloads (see
/// `load::Resident`) can remember the failures too.
pub(crate) type Keyed<T> = HashMap<AssetRef, Result<T, Failure>>;

/// Resolves and decodes every reference into an [`Image`], with every failure
/// still attached to the reference it belongs to.
///
/// Failures are warnings, not errors: a texture that will not download leaves
/// its face bare, which is a far better outcome than refusing to open the file.
/// Kept against the reference so a caller with somewhere to show them (the
/// Output dock) can, without changing what already goes to stderr.
pub(crate) fn load_images(references: &[AssetRef]) -> Keyed<Image> {
    load_with("textures", references, fetch_image)
}

/// Resolves and parses every reference into a [`rbx_mesh::Mesh`]; a failure
/// (a v6/v7 file, a network error, a corrupt download, ...) is a warning
/// against its reference, not an error: `Scene::resolve_file_meshes` leaves
/// the affected `MeshPart`/`SpecialMesh` drawing its fallback box.
pub(crate) fn load_meshes(references: &[AssetRef]) -> Keyed<rbx_mesh::Mesh> {
    load_with("meshes", references, fetch_mesh)
}

/// Resolves every reference to its raw bytes, for assets whose format the
/// scene decodes itself (legacy union assets are `.rbxm` files).
pub(crate) fn load_bytes(references: &[AssetRef]) -> Keyed<Vec<u8>> {
    load_with("unions", references, fetch_bytes)
}

/// The cache-and-network resolver behind every background fetch — see
/// [`crate::load::Source`], whose three methods are the same three decoders
/// [`load_images`]/[`load_meshes`]/[`load_bytes`] run in their own pool.
struct Resolved(AssetResolver);

impl Source for Resolved {
    fn image(&self, reference: &AssetRef) -> Result<Image, Failure> {
        fetch_image(&self.0, reference)
    }

    fn mesh(&self, reference: &AssetRef) -> Result<rbx_mesh::Mesh, Failure> {
        fetch_mesh(&self.0, reference)
    }

    fn bytes(&self, reference: &AssetRef) -> Result<Vec<u8>, Failure> {
        fetch_bytes(&self.0, reference)
    }
}

/// Stands in where no resolver could be built at all (no cache directory,
/// say), so every reference is *answered* with that reason rather than left
/// in flight forever — a viewport waiting on a fetch that can never land
/// would keep its placeholders and never say why. Transient, for the same
/// reason [`load_with`]'s is: the next load may find the machine fixed.
struct Unavailable(String);

impl Unavailable {
    fn failure(&self) -> Failure {
        Failure {
            warning: self.0.clone(),
            transient: true,
        }
    }
}

impl Source for Unavailable {
    fn image(&self, _reference: &AssetRef) -> Result<Image, Failure> {
        Err(self.failure())
    }

    fn mesh(&self, _reference: &AssetRef) -> Result<rbx_mesh::Mesh, Failure> {
        Err(self.failure())
    }

    fn bytes(&self, _reference: &AssetRef) -> Result<Vec<u8>, Failure> {
        Err(self.failure())
    }
}

/// What the background workers resolve through. Built once per streaming
/// loader, not once per batch the way [`load_with`] builds one.
pub(crate) fn source() -> Arc<dyn Source> {
    match resolver() {
        Ok(resolver) => Arc::new(Resolved(resolver)),
        Err(err) => {
            let message = format!("rbxview: no assets ({err})");
            eprintln!("{message}");
            Arc::new(Unavailable(message))
        }
    }
}

/// Shared worker-pool machinery behind [`load_images`], [`load_meshes`] and
/// [`load_bytes`]: same bounded concurrency, same disk cache, same
/// warn-and-skip failure handling — only what a resolved
/// [`Asset`](rbx_assets::Asset) turns into differs.
///
/// Every warning is the same text already `eprintln!`'d, kept against its
/// reference for a caller (`Loaded::from_dom`, ultimately the Output dock)
/// that wants to show it somewhere besides stderr; the CLI's stderr output is
/// unchanged either way. When no resolver could be built at all (no cache
/// directory, say) every reference fails with that one message: it then
/// reaches the caller through the same channel as any other failure, keyed
/// by references the caller actually looks up, and — transient, since it is
/// the machine's fault — is retried by the next load once that is fixed.
fn load_with<T: Send>(
    label: &str,
    references: &[AssetRef],
    decode: impl Fn(&AssetResolver, &AssetRef) -> Result<T, Failure> + Sync,
) -> Keyed<T> {
    if references.is_empty() {
        return HashMap::new();
    }

    let resolver = match resolver() {
        Ok(resolver) => resolver,
        Err(err) => {
            let message = format!("rbxview: no {label} ({err})");
            eprintln!("{message}");
            let failure = Failure {
                warning: message,
                transient: true,
            };
            return references
                .iter()
                .map(|reference| (reference.clone(), Err(failure.clone())))
                .collect();
        }
    };

    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results = Mutex::new(HashMap::new());

    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(references.len()) {
            scope.spawn(|| {
                while let Some(reference) = references.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let result = decode(&resolver, reference);
                    results
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(reference.clone(), result);
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

    let results: Keyed<T> = results.into_inner().unwrap_or_else(|e| e.into_inner());
    // After the progress line is finished, so a warning never lands in the
    // middle of it.
    for failure in results.values().filter_map(|result| result.as_ref().err()) {
        eprintln!("warning: {}", failure.warning);
    }
    results
}

fn resolver() -> Result<AssetResolver, String> {
    #[cfg(test)]
    {
        if let Some(failure) = tests::forced_failure() {
            return Err(failure);
        }
    }
    let cache = AssetCache::new(None).map_err(|err| err.to_string())?;
    let native = NativeContent::new(cache.native_packages_dir());
    let client = Client::new(ApiKey::from_env_or_config());

    Ok(AssetResolver::new(
        cache,
        Box::new(CloudFetcher(client)),
        native,
    ))
}

fn fetch_image(resolver: &AssetResolver, reference: &AssetRef) -> Result<Image, Failure> {
    let failed = |err: AssetError| Failure::resolving(reference, &err);

    let asset = resolver.resolve(reference).map_err(failed)?;
    let decoded = decode_image(&asset).map_err(failed)?;

    let (width, height) = decoded.dimensions();
    Ok(Image {
        width,
        height,
        pixels: decoded.into_raw(),
    })
}

fn fetch_mesh(resolver: &AssetResolver, reference: &AssetRef) -> Result<rbx_mesh::Mesh, Failure> {
    let asset = resolver
        .resolve(reference)
        .map_err(|err| Failure::resolving(reference, &err))?;
    // The bytes are on disk by now, so a parse that fails would fail again.
    rbx_mesh::parse(&asset.bytes).map_err(|err| Failure {
        warning: format!("{}: {err}", describe(reference)),
        transient: false,
    })
}

fn fetch_bytes(resolver: &AssetResolver, reference: &AssetRef) -> Result<Vec<u8>, Failure> {
    resolver
        .resolve(reference)
        .map(|asset| asset.bytes)
        .map_err(|err| Failure {
            warning: err.to_string(),
            transient: transient(&err),
        })
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
pub(crate) mod tests {
    use std::cell::RefCell;

    use super::*;

    thread_local! {
        /// Why [`resolver`] should fail on this thread, while a
        /// [`ResolverFailure`] is alive.
        static FORCED_FAILURE: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    pub(crate) fn forced_failure() -> Option<String> {
        FORCED_FAILURE.with(|failure| failure.borrow().clone())
    }

    /// Stands in for a machine on which no resolver can be built (an
    /// unwritable cache directory, say) without touching the real one:
    /// thread-local, so every other test still resolves for real, and lifted
    /// when dropped. `load_with` builds the resolver on the calling thread,
    /// before its workers start, which is what makes a thread-local enough.
    pub(crate) struct ResolverFailure;

    impl ResolverFailure {
        pub(crate) fn new(message: &str) -> Self {
            FORCED_FAILURE.with(|failure| *failure.borrow_mut() = Some(message.to_string()));
            ResolverFailure
        }
    }

    impl Drop for ResolverFailure {
        fn drop(&mut self) {
            FORCED_FAILURE.with(|failure| *failure.borrow_mut() = None);
        }
    }

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
        assert!(load_images(&[]).is_empty());
    }

    fn permanent(warning: &str) -> Failure {
        Failure {
            warning: warning.to_string(),
            transient: false,
        }
    }

    #[test]
    fn load_with_keeps_every_warning_against_its_reference() {
        let references = vec![AssetRef::Id(1), AssetRef::Id(2)];
        let results = load_with(
            "things",
            &references,
            |_resolver, reference| match reference {
                AssetRef::Id(1) => Ok(1u32),
                _ => Err(permanent(&format!("{}: boom", describe(reference)))),
            },
        );

        assert_eq!(results.get(&AssetRef::Id(1)), Some(&Ok(1)));
        assert_eq!(
            results.get(&AssetRef::Id(2)),
            Some(&Err(permanent("asset 2: boom")))
        );
        assert_eq!(results.len(), 2);
    }

    // The line between "ask again next load" and "remembered for good" is
    // whether the answer is about this machine or about the asset — see
    // `Failure`. `TestPlace.rbxl` names a `SpawnLocation.png` its content
    // package does not hold, and each ask is a 200 ms package scan.
    #[test]
    fn only_a_failure_of_the_machine_is_transient() {
        assert!(transient(&AssetError::Cache(
            rbx_assets::CacheError::NoCacheDir
        )));
        assert!(transient(&AssetError::Network("timed out".to_string())));
        assert!(transient(&AssetError::Fetch(FetchError::Other {
            id: 1,
            message: "rate limited".to_string(),
        })));

        assert!(!transient(&AssetError::Fetch(FetchError::NotFound(1))));
        assert!(!transient(&AssetError::Fetch(FetchError::Refused {
            id: 1,
            message: "HTTP 403".to_string(),
        })));
        assert!(!transient(&AssetError::NativeFileNotFound(
            "textures/SpawnLocation.png".to_string()
        )));
        assert!(!transient(&AssetError::ImageDecode("bad".to_string())));
    }

    fn http(status: u16) -> CloudError {
        CloudError::Http {
            status,
            url: "https://example.com/x".to_string(),
        }
    }

    #[test]
    fn a_404_from_the_cloud_is_not_found_and_everything_else_is_other() {
        assert!(matches!(fetch_error(7, http(404)), FetchError::NotFound(7)));
        assert!(matches!(fetch_error(7, http(410)), FetchError::NotFound(7)));

        let down = CloudError::Transport("connection reset".to_string());
        assert!(matches!(
            fetch_error(7, down),
            FetchError::Other { id: 7, .. }
        ));
    }

    // The keyed route's refusals: a 403 means this key does not cover the
    // asset, and the next load asks with the same key. A 429 or a 5xx got
    // through `rbx_cloud`'s retries and is still the moment's fault.
    #[test]
    fn a_verdict_on_the_request_is_permanent_and_an_outage_is_not() {
        for status in [400, 401, 403, 409, 422] {
            assert!(
                matches!(
                    fetch_error(7, http(status)),
                    FetchError::Refused { id: 7, .. }
                ),
                "HTTP {status} should be final"
            );
        }

        for status in [408, 500, 502, 503] {
            assert!(
                matches!(
                    fetch_error(7, http(status)),
                    FetchError::Other { id: 7, .. }
                ),
                "HTTP {status} should be worth asking again"
            );
        }

        let limited = CloudError::RateLimited {
            retry_after: Some(5),
        };
        assert!(matches!(
            fetch_error(7, limited),
            FetchError::Other { id: 7, .. }
        ));
    }

    // The one failure that is nobody's in particular has to be somebody's
    // to be seen: a caller looks results up by the references it asked with.
    #[test]
    fn a_resolver_that_cannot_be_built_fails_every_reference_with_its_message() {
        let _failure = ResolverFailure::new("cache dir is a file");
        let references = [AssetRef::Id(1), AssetRef::Id(2)];

        let results = load_with::<u32>("things", &references, |_resolver, _reference| Ok(0));

        assert_eq!(results.len(), 2);
        for reference in &references {
            assert_eq!(
                results.get(reference),
                Some(&Err(Failure {
                    warning: "rbxview: no things (cache dir is a file)".to_string(),
                    transient: true,
                }))
            );
        }
    }

    #[test]
    fn load_with_returns_nothing_for_an_empty_reference_list() {
        let results = load_with::<u32>("things", &[], |_resolver, _reference| Ok(0));
        assert!(results.is_empty());
    }
}
