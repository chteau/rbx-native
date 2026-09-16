//! The union half of `Scene::resync_part`: re-reading one legacy
//! `UnionOperation`/`NegateOperation` off the DOM and rebuilding whatever it
//! draws as — a computed mesh or the additive pieces recovered from its
//! operation tree — against a boolean this place has already carved.
//!
//! Nothing here evaluates anything: a boolean is a function of the asset's
//! bytes alone, so the tree and mesh [`super::resolve`] carved are kept by
//! whoever owns the place (`load::Resident`) and re-read here. Moving or
//! recolouring a union is then a matrix and a colour, never a BSP build.

use rbx_assets::AssetRef;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::super::material::Catalog;
use super::super::{Part, Resolved, ResolvedInstance};
use super::{csg, from_operation, Entry, Evaluated, Evaluations, Plan};

/// The single-instance counterpart of [`super::plan`] — see
/// `filemesh::patch::replan`.
pub(in crate::scene) fn replan(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
    materials: &mut Catalog,
) -> Option<Entry> {
    from_operation(dom, database, referent, materials)
}

impl Plan {
    /// Whether this plan already had `referent` drawing `asset` — what tells
    /// a union whose asset this place simply never carved (its bytes did not
    /// download) from one an edit has just pointed at an asset nobody has
    /// fetched, which only a load can carve.
    pub(in crate::scene) fn plans(&self, referent: Ref, asset: &AssetRef) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.referent == referent && entry.asset == *asset)
    }
}

impl Evaluations {
    /// What this place carved for `asset`, or `None` when nothing was: its
    /// bytes never downloaded, or they did not parse as an operation tree.
    /// Either way a fresh [`super::resolve`] leaves the union drawing as its
    /// own box, which is what makes both answerable without a reload — see
    /// [`Evaluations::is_known`] for the one case that is not.
    pub(in crate::scene) fn of(&self, asset: &AssetRef) -> Option<&Evaluated> {
        self.known.get(asset)?.as_deref()
    }
}

impl Evaluated {
    /// Whether the boolean itself succeeded, i.e. whether this asset draws as
    /// one computed mesh rather than as its recovered pieces.
    pub(in crate::scene) fn is_carved(&self) -> bool {
        self.mesh.is_some()
    }
}

impl Entry {
    pub(in crate::scene) fn asset(&self) -> &AssetRef {
        &self.asset
    }

    /// Whether a fresh [`super::resolve`] would drop this union's *mesh* as
    /// fully transparent — see `filemesh::Entry::is_invisible`. Says nothing
    /// about a union drawn as its pieces: those carry the transparency of the
    /// parts they were recovered from, and `resolve` draws them whatever the
    /// union's own `Transparency` says.
    pub(in crate::scene) fn is_invisible(&self) -> bool {
        self.alpha <= 0.0
    }

    /// The instance [`super::resolve`] would build for this entry against
    /// `evaluated`'s computed mesh — `None` where a fresh resolution would
    /// drop it (fully transparent) or would need a mesh `resolved` does not
    /// hold, which only a load computes and uploads.
    ///
    /// Where `UsePartColor` is off the colour comes from the operation tree,
    /// exactly as `resolve` takes it: the tree is right here, so that is a
    /// patch like any other rather than something to rebuild for.
    pub(in crate::scene) fn patched(
        &self,
        evaluated: &Evaluated,
        resolved: &Resolved,
    ) -> Option<ResolvedInstance> {
        if self.alpha <= 0.0 || !resolved.meshes.contains_key(&self.asset) {
            return None;
        }
        Some(self.instance(self.carved_color(evaluated)))
    }

    /// The linear colour a computed union mesh is painted: the union's own
    /// where `UsePartColor` is on, and otherwise the biggest additive leaf's,
    /// which only the operation tree knows. Shared with [`super::resolve`] so
    /// a patched union and a rebuilt one can never be painted differently.
    pub(super) fn carved_color(&self, evaluated: &Evaluated) -> [f32; 3] {
        self.color
            .or_else(|| csg::largest_additive_color(&evaluated.tree))
            .unwrap_or(super::super::FALLBACK_COLOR)
            .map(|channel| super::super::srgb_to_linear(f32::from(channel) / 255.0))
    }

    /// The pieces [`super::resolve`] would draw this union as when its
    /// boolean failed: the tree's additive leaves, each placed by this
    /// entry's own placement, in the tree's order — see `tree::Node::pieces`
    /// for why that order is what keeps a piece's identity stable.
    pub(in crate::scene) fn pieces(
        &self,
        evaluated: &Evaluated,
        database: &ReflectionDatabase,
        materials: &mut Catalog,
    ) -> Vec<Part> {
        evaluated
            .tree
            .pieces(self.placement(), self.referent, database, materials)
    }
}
