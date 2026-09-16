//! Re-planning one part's mesh entry off the DOM, for `Scene::resync_part`:
//! whether a referent draws through the file-mesh path, through a legacy
//! union's own path, or through neither.

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::material::Catalog;
use super::{filemesh, union};

/// One re-planned entry of either kind. The two are answered apart rather
/// than behind one interface: a file mesh has one instance to rebuild, while
/// a union may have a mesh, a set of recovered pieces or nothing but its own
/// box, depending on what its asset carved to — see `Scene::resync_union`.
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
}
