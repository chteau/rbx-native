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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rbx_assets::AssetRef;

use crate::assets::{self, Image, Keyed};
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
}

impl Resident {
    /// Every decoded image among `references`, decoding only what was never
    /// asked for before, plus the warning of every one that failed — whether
    /// it failed just now or earlier in the same load.
    pub(crate) fn images(
        &mut self,
        references: &[AssetRef],
    ) -> (HashMap<AssetRef, Arc<Image>>, Vec<String>) {
        self.images
            .fetch(references, |missing| shared(assets::load_images(missing)))
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
        self.meshes
            .fetch(references, |missing| shared(assets::load_meshes(missing)))
    }

    /// [`Resident::images`] for the raw bytes of a legacy union asset. Handed
    /// out by value: they are a few kilobytes of `.rbxm`, and
    /// `scene::union::resolve` wants to own them.
    pub(crate) fn bytes(
        &mut self,
        references: &[AssetRef],
    ) -> (HashMap<AssetRef, Vec<u8>>, Vec<String>) {
        self.bytes.fetch(references, assets::load_bytes)
    }

    /// Every mesh and union asset this place has asked for and will never
    /// get: a 404, a file the content package does not hold, bytes that
    /// would not decode. A scene is told these up front (see
    /// `Scene::note_lost`) because an edit pointing a part at one of them
    /// draws the box a full build would have drawn, rather than forcing a
    /// reload to ask for it again — and unlike the scene, this outlives
    /// every reload.
    ///
    /// A failure of the machine is left out on purpose: the next load
    /// retries it, so a part pointed at that asset is worth the reload that
    /// does.
    pub(crate) fn lost(&self) -> Vec<AssetRef> {
        self.meshes
            .lost()
            .chain(self.bytes.lost())
            .cloned()
            .collect()
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
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Table {
            entries: HashMap::new(),
        }
    }
}

impl<T> Table<T> {
    /// Every reference here that failed for a reason asking again cannot
    /// change — see the module doc on which is which.
    fn lost(&self) -> impl Iterator<Item = &AssetRef> {
        self.entries
            .iter()
            .filter(|(_, result)| matches!(result, Err(failure) if !failure.transient))
            .map(|(reference, _)| reference)
    }

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
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::assets::Failure;

    fn failure(warning: &str, transient: bool) -> Failure {
        Failure {
            warning: warning.to_string(),
            transient,
        }
    }

    /// A loader that decodes `Id(n)` to `n` and fails every odd id — as the
    /// machine's fault, the kind a later load retries — counting how many
    /// references it was actually asked for.
    fn loader(asked: &Cell<usize>) -> impl Fn(&[AssetRef]) -> Keyed<u64> + '_ {
        move |references| {
            asked.set(asked.get() + references.len());
            references
                .iter()
                .map(|reference| {
                    let AssetRef::Id(id) = reference else {
                        unreachable!("the tests only ask for ids");
                    };
                    let result = if id % 2 == 0 {
                        Ok(*id)
                    } else {
                        Err(failure(&format!("asset {id}: odd"), true))
                    };
                    (reference.clone(), result)
                })
                .collect()
        }
    }

    #[test]
    fn a_first_fetch_loads_everything_once() {
        let asked = Cell::new(0);
        let mut table = Table::default();

        let (found, warnings) = table.fetch(
            &[AssetRef::Id(2), AssetRef::Id(3), AssetRef::Id(2)],
            loader(&asked),
        );

        assert_eq!(asked.get(), 2, "a repeated reference is asked for once");
        assert_eq!(found, HashMap::from([(AssetRef::Id(2), 2)]));
        assert_eq!(warnings, vec!["asset 3: odd".to_string()]);
    }

    // The whole point: a second pass of the same load asks for the same
    // assets again and must not decode any of them a second time — nor retry
    // the one that failed — while still answering exactly what the first
    // pass answered, warning included.
    #[test]
    fn a_second_fetch_answers_from_memory_warnings_included() {
        let asked = Cell::new(0);
        let mut table = Table::default();
        let references = [AssetRef::Id(2), AssetRef::Id(3)];
        let first = table.fetch(&references, loader(&asked));

        let again = table.fetch(&references, loader(&asked));

        assert_eq!(asked.get(), 2);
        assert_eq!(again, first);
    }

    #[test]
    fn only_a_never_seen_reference_is_loaded_later() {
        let asked = Cell::new(0);
        let mut table = Table::default();
        table.fetch(&[AssetRef::Id(2)], loader(&asked));

        let (found, _) = table.fetch(&[AssetRef::Id(2), AssetRef::Id(4)], loader(&asked));

        assert_eq!(asked.get(), 2);
        assert_eq!(found.len(), 2);
    }

    // A failure of the machine is worth one more try once it may have
    // changed — the next load, not the next ask: the blip is then gone, and
    // the asset resolves as if it had never failed.
    #[test]
    fn a_transient_failure_is_retried_by_the_next_load() {
        let asked = Cell::new(0);
        let mut table = Table::default();
        let (_, warnings) = table.fetch(&[AssetRef::Id(3)], loader(&asked));
        assert_eq!(warnings, vec!["asset 3: odd".to_string()]);
        table.fetch(&[AssetRef::Id(3)], loader(&asked));
        assert_eq!(asked.get(), 1, "the same load never retries");

        table.forget_failures();
        let (found, warnings) = table.fetch(&[AssetRef::Id(3)], |references| {
            asked.set(asked.get() + references.len());
            references
                .iter()
                .map(|reference| (reference.clone(), Ok(30)))
                .collect()
        });

        assert_eq!(asked.get(), 2, "the next load retries exactly once");
        assert_eq!(found, HashMap::from([(AssetRef::Id(3), 30)]));
        assert!(warnings.is_empty());
    }

    // A failure of the asset — a 404, a file the package does not hold —
    // is not: the answer cannot change, and on a real place the ask is the
    // expensive part, so the next load answers it from memory like a
    // success, warning included.
    #[test]
    fn a_permanent_failure_is_not_retried_by_the_next_load() {
        let asked = Cell::new(0);
        let mut table: Table<u64> = Table::default();
        let not_found = |references: &[AssetRef]| {
            asked.set(asked.get() + references.len());
            references
                .iter()
                .map(|reference| (reference.clone(), Err(failure("asset 5: not found", false))))
                .collect()
        };
        table.fetch(&[AssetRef::Id(5)], not_found);

        table.forget_failures();
        let (found, warnings) = table.fetch(&[AssetRef::Id(5)], not_found);

        assert_eq!(asked.get(), 1, "never asked again");
        assert!(found.is_empty());
        assert_eq!(warnings, vec!["asset 5: not found".to_string()]);
    }

    // What `Resident::lost` rests on: only a failure of the asset itself is
    // an answer that outlives the load that got it, so only that one is
    // worth telling a rebuilt scene about.
    #[test]
    fn only_a_permanent_failure_counts_as_lost() {
        let mut table: Table<u64> = Table::default();
        table.fetch(&[AssetRef::Id(2), AssetRef::Id(3)], |references| {
            references
                .iter()
                .map(|reference| {
                    let transient = *reference == AssetRef::Id(3);
                    (
                        reference.clone(),
                        Err(failure("asset: no", transient)) as Result<u64, Failure>,
                    )
                })
                .collect()
        });

        assert_eq!(
            table.lost().cloned().collect::<Vec<_>>(),
            vec![AssetRef::Id(2)]
        );
    }

    // Forgetting the failures must not cost the successes: those are the
    // decodes a reload exists to skip.
    #[test]
    fn forgetting_failures_keeps_every_success() {
        let asked = Cell::new(0);
        let mut table = Table::default();
        let references = [AssetRef::Id(2), AssetRef::Id(3)];
        table.fetch(&references, loader(&asked));

        table.forget_failures();
        let (found, warnings) = table.fetch(&references, loader(&asked));

        assert_eq!(asked.get(), 3, "only the failed reference is asked again");
        assert_eq!(found, HashMap::from([(AssetRef::Id(2), 2)]));
        assert_eq!(warnings, vec!["asset 3: odd".to_string()]);
    }

    // What `assets::load_with` answers when no resolver could be built at
    // all: the one message against every reference. Every reference is
    // remembered as failed (so the next load retries once the machine is
    // fixed), but the dock reads the message once.
    #[test]
    fn one_warning_shared_by_every_reference_is_reported_once() {
        let mut table: Table<u64> = Table::default();
        let no_resolver = |references: &[AssetRef]| {
            references
                .iter()
                .map(|reference| {
                    (
                        reference.clone(),
                        Err(failure("rbxview: no things (boom)", true)),
                    )
                })
                .collect()
        };

        let (found, warnings) = table.fetch(
            &[AssetRef::Id(1), AssetRef::Id(2), AssetRef::Id(3)],
            no_resolver,
        );

        assert!(found.is_empty());
        assert_eq!(warnings, vec!["rbxview: no things (boom)".to_string()]);
    }

    // A loader that answers nothing for a reference leaves it unknown rather
    // than remembered as failed: it is asked again on the very next fetch.
    #[test]
    fn a_reference_the_loader_did_not_answer_is_asked_again() {
        let asked = Cell::new(0);
        let mut table: Table<u64> = Table::default();
        let silent = |references: &[AssetRef]| {
            asked.set(asked.get() + references.len());
            HashMap::new()
        };

        table.fetch(&[AssetRef::Id(2)], silent);
        table.fetch(&[AssetRef::Id(2)], silent);

        assert_eq!(asked.get(), 2);
    }
}
