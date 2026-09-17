//! What an edit to the DOM means to a scene already built from it.
//!
//! Roblox's own engine never rebuilds its scene: the DataModel is the live
//! picture, and every property write is an event the renderer applies to
//! that one instance. `Headless::apply_changes` is that model here — it
//! takes the `Change` log a mutation produced and patches only the instances
//! it names — and this module is the vocabulary it needs: the log folded by
//! instance ([`fold`]), each instance's part in the picture ([`Role`]), and
//! what the call reports back ([`Applied`]).

mod role;

use std::collections::HashMap;

use rbx_dom::{Change, Ref};

pub(crate) use role::{holds_gui, Known, Role, Roles};

/// What `Headless::apply_changes` did with a change log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// Every instance the log named was patched in place; nothing else was
    /// touched, and the cost scaled with the edit rather than the place.
    Patched,
    /// The whole scene was re-derived from the DOM (`Headless::reload`),
    /// because of the reason named. Everything is right afterwards, as it
    /// would be after any reload; the reason is for whoever wants to know
    /// why the edit cost a rebuild.
    Rebuilt(Rebuild),
}

/// Every reason left for which an edit still rebuilds the whole scene
/// instead of patching the instances it touched. Each variant is one
/// remaining reason, not a catch-all: an edit that fits none of them is
/// patched, whatever it is and however many instances it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rebuild {
    /// A `Sky` was edited, added or removed. Its six panels are prefiltered
    /// into the environment probe every surface samples, and the sun, moon
    /// and star field hang off it too; all of that is keyed by the panel
    /// assets and rebuilt only when they change (see `renderer::rebuild`),
    /// so this costs a few milliseconds and stays a rebuild on purpose.
    Sky,
    /// A `MaterialVariant` or `MaterialService` changed. The material
    /// catalog's layers are defined from the service (see
    /// `scene::material::Catalog::new`), and the texture arrays are uploaded
    /// from the catalog as a whole.
    Materials,
    /// The edit needs an asset this renderer never uploaded: a mesh, a decal
    /// or mesh texture, a material pack, a `SurfaceAppearance` map set, or
    /// the legacy union asset an edit has just pointed a `UnionOperation` at.
    /// Fetching, decoding and (for a union) carving one is a load-time path
    /// today, so only a full reload has it.
    Asset,
}

impl std::fmt::Display for Rebuild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Rebuild::Sky => "a Sky changed",
            Rebuild::Materials => "a material definition changed",
            Rebuild::Asset => "an asset was never uploaded",
        })
    }
}

/// One instance a change log names, with everything the log said about it
/// folded together — see [`fold`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Touched {
    pub(crate) referent: Ref,
    /// Whether the instance was added, removed or reparented, as opposed to
    /// only written to: a structural change reaches its whole subtree (a
    /// `Model` moved into `Workspace` brings every part under it), a
    /// property write reaches the one instance and what hangs off it.
    pub(crate) structural: bool,
    /// The parent it left, for the first `Change::Parent` in the log — the
    /// part a `SpecialMesh` was just taken away from is drawn differently
    /// too, and nothing in the DOM as it stands now still points at it.
    pub(crate) old_parent: Option<Ref>,
}

/// Folds a change log into the instances it names, each once, in the order
/// first named. A `Change` is a hint about *which* instance may differ
/// between the scene and the DOM, never a record of the value — the patch
/// reads the truth back from the DOM — so an instance written ten times is
/// patched once, and whether a referent is being created or destroyed is
/// decided by whether the DOM still has it, not by which variant said so.
/// That last point is what lets an undo hand over the very log its mutation
/// produced: the `Added` of an insert, applied to the DOM the undo restored,
/// finds the instance gone and takes it out.
pub(crate) fn fold(changes: &[Change]) -> Vec<Touched> {
    let mut touched: Vec<Touched> = Vec::new();
    // Where each referent's entry sits, so a log of a thousand writes folds
    // in a thousand lookups rather than a scan per write.
    let mut slots: HashMap<Ref, usize> = HashMap::new();
    for change in changes {
        let (referent, structural, old_parent) = match change {
            Change::Property { referent, .. } => (*referent, false, None),
            Change::Parent { referent, old, .. } => (*referent, true, *old),
            Change::Added(referent) | Change::Removed(referent) => (*referent, true, None),
        };
        match slots.get(&referent) {
            Some(&slot) => {
                let entry = &mut touched[slot];
                entry.structural |= structural;
                if entry.old_parent.is_none() {
                    entry.old_parent = old_parent;
                }
            }
            None => {
                slots.insert(referent, touched.len());
                touched.push(Touched {
                    referent,
                    structural,
                    old_parent,
                });
            }
        }
    }
    touched
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(id: u32, name: &str) -> Change {
        Change::Property {
            referent: Ref::new(id),
            name: name.to_string(),
        }
    }

    #[test]
    fn writes_on_one_instance_fold_to_one_entry_however_many() {
        let touched = fold(&[write(7, "size"), write(7, "CFrame"), write(7, "size")]);

        assert_eq!(
            touched,
            vec![Touched {
                referent: Ref::new(7),
                structural: false,
                old_parent: None,
            }]
        );
    }

    #[test]
    fn instances_keep_the_order_they_were_first_named_in() {
        let touched = fold(&[write(2, "a"), write(1, "a"), write(2, "b"), write(3, "a")]);

        let order: Vec<u32> = touched.iter().map(|t| t.referent.value()).collect();
        assert_eq!(order, vec![2, 1, 3]);
    }

    // An insert logs `Added` and then every default it sets on the new
    // instance; the lot is one structural entry, since the add builds the
    // instance from the DOM as it stands anyway.
    #[test]
    fn an_add_with_its_setup_writes_is_one_structural_entry() {
        let touched = fold(&[
            Change::Added(Ref::new(1)),
            write(1, "size"),
            write(1, "CFrame"),
        ]);

        assert_eq!(touched.len(), 1);
        assert!(touched[0].structural);
    }

    #[test]
    fn a_reparent_remembers_the_parent_it_left() {
        let moved = Change::Parent {
            referent: Ref::new(4),
            old: Some(Ref::new(2)),
            new: Some(Ref::new(3)),
        };
        let again = Change::Parent {
            referent: Ref::new(4),
            old: Some(Ref::new(3)),
            new: Some(Ref::new(5)),
        };

        let touched = fold(&[write(4, "Name"), moved, again]);

        assert_eq!(touched.len(), 1);
        assert!(touched[0].structural);
        assert_eq!(
            touched[0].old_parent,
            Some(Ref::new(2)),
            "two moves in one log leave the first origin as the one nothing now points at"
        );
    }

    #[test]
    fn a_removal_is_structural_and_an_empty_log_names_nothing() {
        assert!(fold(&[]).is_empty());
        let touched = fold(&[Change::Removed(Ref::new(9))]);
        assert_eq!(touched.len(), 1);
        assert!(touched[0].structural);
    }
}
