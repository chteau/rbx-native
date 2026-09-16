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
//! A failure is remembered too, with its warning: an asset that would not
//! download or decode is not fetched again on every edit, and the warning is
//! answered again each time it is asked for, so the Output dock reads the
//! same after a reload as it did after the load.

use std::collections::HashMap;
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
    /// it failed just now or on an earlier load.
    pub(crate) fn images(
        &mut self,
        references: &[AssetRef],
    ) -> (HashMap<AssetRef, Arc<Image>>, Vec<String>) {
        self.images
            .fetch(references, |missing| shared(assets::load_images(missing)))
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

impl<T: Clone> Table<T> {
    /// Answers `references` from what is already here, asking `load` only
    /// for the ones that are not — never twice for the same reference, a
    /// remembered failure included. A reference `load` answers nothing for
    /// (no resolver could be built at all) is asked again next time.
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
        for reference in wanted {
            match self.entries.get(&reference) {
                Some(Ok(value)) => {
                    found.insert(reference, value.clone());
                }
                Some(Err(warning)) => warnings.push(warning.clone()),
                None => {}
            }
        }
        (found, warnings)
    }
}

/// `references` without repeats, in first-seen order — the order the
/// progress line and the warnings then come out in.
fn distinct(references: &[AssetRef]) -> Vec<AssetRef> {
    let mut seen = Vec::new();
    for reference in references {
        if !seen.contains(reference) {
            seen.push(reference.clone());
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// A loader that decodes `Id(n)` to `n` and fails every odd id, counting
    /// how many references it was actually asked for.
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
                        Err(format!("asset {id}: odd"))
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

    // The whole point: a reload asks for the same assets again and must not
    // decode any of them a second time — nor retry the one that failed —
    // while still answering exactly what a first load answered, warning
    // included.
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

    // No resolver at all (no cache directory, say) answers nothing for the
    // references themselves: they stay unknown rather than remembered as
    // failed, so the next reload tries again once the machine is fixed.
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
