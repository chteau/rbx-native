//! What a place's assets decode to, kept in memory across reloads.
//!
//! `Headless::reload` re-derives the whole scene from the DOM, and the scene
//! asks for its assets again by reference: every decal image, every material
//! pack, every file mesh, every union's operation tree. The on-disk
//! `rbx_assets::AssetCache` makes downloading them again free; it cannot make
//! decoding them again free, and on a real place that decode was most of a
//! reload. So what they decoded to stays here, keyed by the very reference
//! the scene asks with, for as long as the `Headless` (or the one-shot load)
//! that owns this lives — and only a reference never seen before touches the
//! disk.
//!
//! A failure is remembered too, with its warning. One of the asset's own —
//! a 404, a file the content package does not hold, bytes that will not
//! decode — is remembered for good: asking again cannot change the answer,
//! and the ask is the expensive part. One of the machine's — a request that
//! did not complete, a cache that would not write — is remembered only until
//! the next load: the load that hit it asks for the same reference from
//! several passes (a decal image and a material map can name one asset) and
//! must not download it that many times, while the next `Headless::reload`
//! tries it once more, so a network blip does not leave a face bare for the
//! life of the editor. See `assets::Failure` for which is which. Either way
//! the warning is answered again each time the reference is asked for, so
//! the Output dock reads the same after a reload as it did after the load.
//!
//! # Streaming and blocking
//!
//! A [`Resident::streaming`] one never waits for an asset. A reference it has
//! not seen is handed to the background pool (see [`fetcher`]) and answered
//! as absent for now; the caller draws the fallback and polls [`Resident::poll`]
//! once a tick for what has landed since. That is what a viewport uses — a
//! keystroke that names a new `MeshId` must not put a download between two
//! frames.
//!
//! A [`Resident::default`] one resolves in the caller's own thread, progress
//! line and all, and is what `rbxview`'s one-shot screenshot and its windowed
//! load use: neither has a next frame to stream into, so there is nothing to
//! be gained by returning without the picture.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
#[cfg(test)]
use std::time::Duration;

use rbx_assets::AssetRef;

use super::fetcher::{Fetcher, Landed, Want};
use crate::assets::{self, Failure, Image, Keyed};
use crate::scene::UnionEvaluations;

/// Every decoded asset a place has asked for so far. Behind `Arc`s where the
/// scene keeps its own handle (see `scene::filemesh::Resolved`,
/// `textures::Decor`): the scene's copy and this one are then the same
/// bytes, not two.
#[derive(Default)]
pub(crate) struct Resident {
    images: Table<Arc<Image>>,
    meshes: Table<Arc<rbx_mesh::Mesh>>,
    bytes: Table<Vec<u8>>,
    /// The unions' booleans, which are a function of the asset bytes alone
    /// and the one CPU-heavy step of resolving a scene — see
    /// `scene::union::Evaluations`.
    pub(crate) unions: UnionEvaluations,
    /// Where a reference this has never seen is fetched and decoded, or `None`
    /// to resolve it in the calling thread instead — see the module doc.
    fetcher: Option<Fetcher>,
}

/// What [`Resident::poll`] found waiting.
#[derive(Default)]
pub(crate) struct Settled {
    /// Every reference answered since the last poll, whether it decoded or
    /// failed: what the caller checks its scene against to decide whether
    /// anything it draws has to be re-resolved.
    pub(crate) references: Vec<AssetRef>,
    /// The warning of each of those that failed, once per reference for the
    /// life of this `Resident` — a fetch that cannot succeed is not worth
    /// saying so about on every later edit.
    pub(crate) warnings: Vec<String>,
}

impl Resident {
    /// One that hands every unseen reference to a background pool and never
    /// blocks — see the module doc.
    pub(crate) fn streaming() -> Self {
        Resident::fed_by(assets::source())
    }

    /// [`Resident::streaming`] against a source of the caller's choosing —
    /// what a test uses to stream assets with no cache directory and no
    /// network anywhere in reach.
    pub(crate) fn fed_by(source: Arc<dyn super::fetcher::Source>) -> Self {
        Resident {
            fetcher: Some(Fetcher::new(source, assets::WORKERS)),
            ..Resident::default()
        }
    }

    /// Every decoded image among `references`, decoding only what was never
    /// asked for before, plus — for a blocking `Resident` — the warning of
    /// every one that failed, whether it failed just now or earlier in the
    /// same load. A streaming one answers only from memory, queues whatever
    /// is missing and reports its warnings through [`Resident::poll`]
    /// instead.
    pub(crate) fn images(
        &mut self,
        references: &[AssetRef],
    ) -> (HashMap<AssetRef, Arc<Image>>, Vec<String>) {
        match &self.fetcher {
            Some(fetcher) => (
                self.images.take(references, Want::Image, fetcher),
                Vec::new(),
            ),
            None => self
                .images
                .fetch(references, |missing| shared(assets::load_images(missing))),
        }
    }

    /// Whether `reference` was asked for as an image and would not download
    /// or decode — for an edit deciding whether a face it cannot paint is
    /// one a full build would have left unpainted too, or one nobody has
    /// fetched yet.
    pub(crate) fn image_failed(&self, reference: &AssetRef) -> bool {
        matches!(self.images.entries.get(reference), Some(Err(_)))
    }

    /// [`Resident::images`] for file meshes.
    pub(crate) fn meshes(
        &mut self,
        references: &[AssetRef],
    ) -> (HashMap<AssetRef, Arc<rbx_mesh::Mesh>>, Vec<String>) {
        match &self.fetcher {
            Some(fetcher) => (
                self.meshes.take(references, Want::Mesh, fetcher),
                Vec::new(),
            ),
            None => self
                .meshes
                .fetch(references, |missing| shared(assets::load_meshes(missing))),
        }
    }

    /// [`Resident::images`] for the raw bytes of a legacy union asset. Handed
    /// out by value: they are a few kilobytes of `.rbxm`, and
    /// `scene::union::resolve` wants to own them.
    pub(crate) fn bytes(
        &mut self,
        references: &[AssetRef],
    ) -> (HashMap<AssetRef, Vec<u8>>, Vec<String>) {
        match &self.fetcher {
            Some(fetcher) => (
                self.bytes.take(references, Want::Bytes, fetcher),
                Vec::new(),
            ),
            None => self.bytes.fetch(references, assets::load_bytes),
        }
    }

    /// Which of `references` this has an answer to, `None` where the answer
    /// was a failure and absent where it is still coming — what a renderer
    /// pass that uploads an image itself needs in order to tell "never" from
    /// "not yet" (see `renderer::particles`, whose emitters are dropped
    /// outright by the first and must survive the second).
    pub(crate) fn answered(&self, references: &[AssetRef]) -> Answered {
        references
            .iter()
            .filter_map(|reference| {
                let answer = match self.images.entries.get(reference)? {
                    Ok(image) => Some(image.clone()),
                    Err(_) => None,
                };
                Some((reference.clone(), answer))
            })
            .collect()
    }

    /// Files everything the background pool has finished since the last call.
    ///
    /// Empty for a blocking `Resident`, and on most ticks of a streaming one.
    pub(crate) fn poll(&mut self) -> Settled {
        let Some(fetcher) = &self.fetcher else {
            return Settled::default();
        };

        let mut settled = Settled::default();
        for landed in fetcher.drain() {
            let (reference, warning) = match landed {
                Landed::Image(reference, result) => self.images.land(reference, result),
                Landed::Mesh(reference, result) => self.meshes.land(reference, result),
                Landed::Bytes(reference, result) => self.bytes.land(reference, result),
            };
            settled.references.push(reference);
            settled.warnings.extend(warning);
        }
        settled
    }

    /// Whether anything is still on its way. Drives the one-off swap-in the
    /// moment a place has finished loading, and tells a test when to stop
    /// waiting.
    pub(crate) fn in_flight(&self) -> usize {
        self.images.in_flight.len() + self.meshes.in_flight.len() + self.bytes.in_flight.len()
    }

    /// Blocks until nothing is in flight or `timeout` runs out, filing
    /// everything that lands. Never called from a render thread — it exists
    /// for a test and for a caller with no frame to draw meanwhile.
    #[cfg(test)]
    pub(crate) fn settle(&mut self, timeout: Duration) -> Settled {
        let deadline = std::time::Instant::now() + timeout;
        let mut all = self.poll();
        while self.in_flight() > 0 && std::time::Instant::now() < deadline {
            // Polled on a timer rather than waited for on the channel: taking
            // a result off it here would be taking it away from `poll`, which
            // is the only thing that files one.
            std::thread::sleep(Duration::from_millis(1));
            let settled = self.poll();
            all.references.extend(settled.references);
            all.warnings.extend(settled.warnings);
        }
        all
    }

    /// Drops what one reference decoded to, so the next scene that names it
    /// fetches and decodes it again.
    ///
    /// Only a benchmark has any business calling this: it is how a harness
    /// stages "an edit naming an asset this session has never seen" against a
    /// fixture whose assets are all in the on-disk cache, without evicting the
    /// cache itself and measuring a download. The next request for the
    /// reference is skipped too, so the reload that stages the edit rebuilds
    /// the place without it — see [`Table::forget`].
    pub(crate) fn forget(&mut self, reference: &AssetRef) {
        self.images.forget(reference);
        self.meshes.forget(reference);
        self.bytes.forget(reference);
    }

    /// Drops every remembered transient failure, so the next ask for it
    /// fetches again. Called once at the start of every load (see
    /// `Loaded::from_dom`): that is the unit a retry is worth — see the
    /// module doc.
    pub(crate) fn forget_failures(&mut self) {
        self.images.forget_failures();
        self.meshes.forget_failures();
        self.bytes.forget_failures();
    }
}

/// Every image a caller asked about that has been answered: `Some` decoded,
/// `None` tried and failed. A reference still in flight is absent.
pub(crate) type Answered = HashMap<AssetRef, Option<Arc<Image>>>;

fn shared<T>(keyed: Keyed<T>) -> Keyed<Arc<T>> {
    keyed
        .into_iter()
        .map(|(reference, result)| (reference, result.map(Arc::new)))
        .collect()
}

/// One kind of asset's results so far: the value, or the warning it failed
/// with.
struct Table<T> {
    entries: Keyed<T>,
    /// Handed to the background pool and not yet answered. This is what
    /// coalesces: a second scene, edit or tick naming the same reference
    /// finds it here and queues nothing.
    in_flight: Vec<AssetRef>,
    /// Failures already reported to the caller. A remembered failure is
    /// answered as absent for the rest of the session, and saying so once is
    /// the whole of what the Output dock needs.
    warned: Vec<AssetRef>,
    /// References to answer as absent *without* requesting, exactly once
    /// more — see [`Table::forget`].
    skipped: Vec<AssetRef>,
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Table {
            entries: HashMap::new(),
            in_flight: Vec::new(),
            warned: Vec::new(),
            skipped: Vec::new(),
        }
    }
}

impl<T> Table<T> {
    fn forget_failures(&mut self) {
        self.entries
            .retain(|_, result| !matches!(result, Err(failure) if failure.transient));
    }
}

impl<T: Clone> Table<T> {
    /// Answers `references` from what is already here, asking `load` only
    /// for the ones that are not — never twice for the same reference, a
    /// remembered failure included, until [`Table::forget_failures`] lets a
    /// transient one go. A reference `load` answers nothing for is asked
    /// again next time.
    ///
    /// A warning shared by several references comes out once: a resolver
    /// that could not be built fails every reference with the same text (see
    /// `assets::load_with`), and the Output dock wants that line once, not
    /// once per asset.
    fn fetch(
        &mut self,
        references: &[AssetRef],
        load: impl FnOnce(&[AssetRef]) -> Keyed<T>,
    ) -> (HashMap<AssetRef, T>, Vec<String>) {
        let wanted = distinct(references);
        let missing: Vec<AssetRef> = wanted
            .iter()
            .filter(|reference| !self.entries.contains_key(*reference))
            .cloned()
            .collect();
        if !missing.is_empty() {
            self.entries.extend(load(&missing));
        }

        let mut found = HashMap::new();
        let mut warnings = Vec::new();
        let mut reported = HashSet::new();
        for reference in wanted {
            match self.entries.get(&reference) {
                Some(Ok(value)) => {
                    found.insert(reference, value.clone());
                }
                Some(Err(failure)) if reported.insert(failure.warning.as_str()) => {
                    warnings.push(failure.warning.clone());
                }
                Some(Err(_)) | None => {}
            }
        }
        (found, warnings)
    }

    /// [`Table::fetch`] for a streaming loader: whatever is already decoded,
    /// with everything else queued rather than waited for.
    fn take(
        &mut self,
        references: &[AssetRef],
        want: Want,
        fetcher: &Fetcher,
    ) -> HashMap<AssetRef, T> {
        let mut found = HashMap::new();
        for reference in distinct(references) {
            match self.entries.get(&reference) {
                Some(Ok(value)) => {
                    found.insert(reference, value.clone());
                }
                // Tried and failed: asking again would fail again.
                Some(Err(_)) => {}
                None => match self.skipped.iter().position(|held| *held == reference) {
                    Some(at) => {
                        self.skipped.remove(at);
                    }
                    None => self.request(reference, want, fetcher),
                },
            }
        }
        found
    }

    fn request(&mut self, reference: AssetRef, want: Want, fetcher: &Fetcher) {
        if self.in_flight.contains(&reference) {
            return;
        }
        self.in_flight.push(reference.clone());
        fetcher.request(want, reference);
    }

    /// Files one finished request, reporting its warning the first time only.
    fn land(
        &mut self,
        reference: AssetRef,
        result: Result<T, Failure>,
    ) -> (AssetRef, Option<String>) {
        self.in_flight.retain(|held| *held != reference);
        let warning = match &result {
            Err(failure) if !self.warned.contains(&reference) => {
                self.warned.push(reference.clone());
                Some(failure.warning.clone())
            }
            _ => None,
        };
        self.entries.insert(reference.clone(), result);
        (reference, warning)
    }

    /// Drops what `reference` decoded to and skips the *next* request for it.
    ///
    /// The skip is what makes the hook usable: a reload right after this
    /// rebuilds the place as it stood before the asset was ever seen, instead
    /// of re-fetching it from the disk cache in the same millisecond. The
    /// request after that — the edit the harness is actually measuring — goes
    /// through as normal.
    fn forget(&mut self, reference: &AssetRef) {
        self.entries.remove(reference);
        self.warned.retain(|held| held != reference);
        if !self.skipped.contains(reference) {
            self.skipped.push(reference.clone());
        }
    }
}

/// `references` without repeats, in first-seen order — the order the
/// progress line and the warnings then come out in.
fn distinct(references: &[AssetRef]) -> Vec<AssetRef> {
    let mut seen = HashSet::new();
    references
        .iter()
        .filter(|reference| seen.insert(*reference))
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "resident/tests.rs"]
mod tests;
