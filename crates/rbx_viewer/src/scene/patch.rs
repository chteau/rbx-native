//! Re-planning one part's mesh entry off the DOM, for `Scene::resync_part`:
//! whether a referent draws through the file-mesh path at all, and the
//! instance it would get against what already downloaded.

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::material::Catalog;
use super::{filemesh, union, Resolved, ResolvedInstance};

/// One re-planned entry of either kind, so the two can be asked the same
/// questions — see `Scene::resync_part`.
pub(super) enum Replanned {
    Mesh(filemesh::Entry),
    Union(union::Entry),
}

impl Replanned {
    /// The entry `referent` would get from a fresh `filemesh::plan` or
    /// `union::plan`, or `None` when it is neither (a plain `Part`, or a
    /// `MeshPart` whose `MeshId` was just emptied, say).
    pub(super) fn of(
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Ref,
        materials: &mut Catalog,
    ) -> Option<Self> {
        filemesh::replan(dom, database, referent, materials)
            .map(Replanned::Mesh)
            .or_else(|| union::replan(dom, database, referent, materials).map(Replanned::Union))
    }

    /// `Transparency` 1: a fresh resolution would drop the instance outright.
    pub(super) fn is_invisible(&self) -> bool {
        match self {
            Replanned::Mesh(entry) => entry.is_invisible(),
            Replanned::Union(entry) => entry.is_invisible(),
        }
    }

    /// The instance a fresh resolution would build against what already
    /// downloaded — `None` where it would need something only a full reload
    /// fetches or computes (see `filemesh::Entry::patched` and
    /// `union::Entry::patched`).
    pub(super) fn patched(&self, resolved: &Resolved) -> Option<ResolvedInstance> {
        match self {
            Replanned::Mesh(entry) => entry.patched(resolved),
            Replanned::Union(entry) => entry.patched(),
        }
    }
}
