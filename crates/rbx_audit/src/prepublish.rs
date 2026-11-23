//! The pre-publish check: everything [`crate::audit`] finds, plus the rules
//! below, run over a place before it goes to Roblox. Each rule's id is
//! [`Check::id`]; `docs/prepublish-check.md` is the user-facing copy of this
//! list, and the editor's Pre-publish dock shows the same text.
//!
//! - **`broken-asset`**: a content property (`MeshId`, `TextureID`,
//!   `Image`, `SoundId`, `AnimationId`, `Decal.Texture`, `MeshContent`, any
//!   property the API dump types as content) that isn't an asset reference
//!   Roblox can load (High), names an asset the asset service says doesn't
//!   exist (High) or refuses (Medium), or is empty where the class needs it
//!   (`MeshPart` mesh, `FileMesh` `SpecialMesh`, `Animation`: Medium;
//!   `Sound`, `Decal`, `Texture`: Low).
//! - **`assets-not-checked`** (Low): ids whose existence couldn't be asked
//!   (offline, or the check was skipped).
//! - **`deprecated-api`** (Low): `wait`, `spawn`, `delay`,
//!   `Instance.new(class, parent)`, `:connect`/`:disconnect`,
//!   `game.Workspace`, and methods the API dump tags `Deprecated`.
//! - **`unanchored-parts`** (Medium): parts in the Workspace that aren't
//!   anchored, welded or constrained to an anchored part, or held by a
//!   mover, outside characters, tools and accessories; one finding per
//!   top-level model with the count.
//! - **`datastore-without-pcall`** (Medium): DataStore and MemoryStore
//!   calls not inside `pcall`, `xpcall` or a `Promise`.
//! - **`heavy-geometry`**: a `MeshPart` over Roblox's
//!   [`MESH_TRIANGLE_LIMIT`] (High), a union over [`UNION_TRIANGLES`]
//!   (Medium).
//! - **`place-budget`** (Medium): more than [`PART_BUDGET`] parts or
//!   [`INSTANCE_BUDGET`] instances in the place.
//! - **`streaming-index`** (Low): with `Workspace.StreamingEnabled`, client
//!   code that reaches a streamed part with `.` instead of `WaitForChild`.
//!
//! Nothing here touches the network: the asset service's answers come in
//! through [`Inputs::presence`], and mesh bytes through [`Inputs::fetch`]
//! (the local asset cache), so the caller decides what is asked and when.

mod assets;
mod parts;
mod scripts;

use std::collections::HashMap;

use rbx_assets::AssetRef;
use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::{full_name, Check, Finding, Severity};

pub use assets::asset_ids;

/// Roblox refuses to import a mesh with more triangles than this
/// (creator-docs, "Mesh requirements"), so one over it was never meant to
/// ship.
pub const MESH_TRIANGLE_LIMIT: usize = 20_000;
/// A union over this many triangles: CSG this heavy costs memory and
/// physics on every device; a mesh is the better tool. Our budget, not a
/// Roblox limit.
pub const UNION_TRIANGLES: usize = 10_000;
/// Parts in the place. Roblox sets no cap; past this, low-end phones drop
/// frames and the join takes long. Our budget.
pub const PART_BUDGET: usize = 30_000;
/// Instances in the place, every class. Our budget, for the same reason.
pub const INSTANCE_BUDGET: usize = 250_000;

/// What the asset service answered for one id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// Delivered, or "sign in to download", which only an existing asset
    /// answers.
    Exists,
    /// "Asset not found".
    Missing,
    /// Refused outright (moderated, or not shared with anyone).
    Restricted,
}

pub struct Inputs<'a> {
    /// The asset service's answer per id; `None` when it wasn't asked
    /// (offline). An id the map lacks counts as not checked.
    pub presence: Option<&'a HashMap<u64, Presence>>,
    /// An asset's bytes from a local cache, never the network: the meshes
    /// `heavy-geometry` measures.
    pub fetch: &'a mut dyn FnMut(&AssetRef) -> Option<Vec<u8>>,
}

/// The audit and every pre-publish rule over `dom`, sorted like
/// [`crate::audit`].
pub fn check(dom: &WeakDom, inputs: Inputs) -> Vec<Finding> {
    let db = ReflectionDatabase::shared();
    let (scripts, remotes) = crate::parse(dom);
    let mut findings = crate::audit_parsed(dom, &scripts, remotes);
    findings.extend(assets::check(dom, db, inputs.presence));
    findings.extend(scripts::check(dom, db, &scripts));
    findings.extend(parts::unanchored(dom, db));
    findings.extend(parts::geometry(dom, db, inputs.fetch));
    findings.extend(budgets(dom, db));
    crate::sort(&mut findings);
    findings
}

/// A copy of `dom` with what [`check`] reads (the tree, and every string,
/// number, flag, enum, reference and content property) under the same
/// refs: no geometry, physics or terrain blobs, so it is cheap enough to
/// take on the UI thread and check on another.
pub fn slim_copy(dom: &WeakDom) -> WeakDom {
    let mut copy = WeakDom::new();
    let mut stack: Vec<(Ref, Option<Ref>)> = dom.root_refs().iter().map(|r| (*r, None)).collect();
    while let Some((r, parent)) = stack.pop() {
        let Some(instance) = dom.get(r) else { continue };
        let mut slim = Instance::new(r, instance.class(), instance.name());
        for (key, value) in instance.properties() {
            if matches!(
                value,
                Variant::String(_)
                    | Variant::Bool(_)
                    | Variant::Int32(_)
                    | Variant::Int64(_)
                    | Variant::Enum(_)
                    | Variant::Ref(_)
                    | Variant::Content(_)
            ) {
                slim.properties_mut().insert(key.clone(), value.clone());
            }
        }
        copy.insert(slim);
        if parent.is_some() {
            copy.set_parent(r, parent);
        }
        stack.extend(instance.children().iter().rev().map(|c| (*c, Some(r))));
    }
    copy
}

fn budgets(dom: &WeakDom, db: &ReflectionDatabase) -> Vec<Finding> {
    let (mut parts, mut instances) = (0, 0);
    let mut stack = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        let Some(instance) = dom.get(r) else { continue };
        instances += 1;
        parts += usize::from(db.is_subclass_of(instance.class(), "BasePart"));
        stack.extend_from_slice(instance.children());
    }
    let Some(place) = place_ref(dom) else {
        return Vec::new();
    };
    [
        (parts, PART_BUDGET, "parts"),
        (instances, INSTANCE_BUDGET, "instances"),
    ]
    .into_iter()
    .filter(|(count, budget, _)| count > budget)
    .map(|(count, budget, what)| {
        place_finding(
            dom,
            place,
            Check::PlaceBudget,
            Severity::Medium,
            format!("The place has {count} {what}, over the budget of {budget}: low-end phones will drop frames and joining takes longer."),
        )
    })
    .collect()
}

/// Where a finding about the whole place points: the Workspace, or the
/// first service when there is none.
fn place_ref(dom: &WeakDom) -> Option<Ref> {
    let roots = dom.root_refs();
    roots
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
        .or_else(|| roots.first().copied())
}

fn place_finding(
    dom: &WeakDom,
    instance: Ref,
    check: Check,
    severity: Severity,
    message: String,
) -> Finding {
    Finding {
        check,
        severity,
        instance,
        path: full_name(dom, instance),
        line: None,
        message,
    }
}

/// A property's asset URI: a `Content` URI, or a string property the dump
/// types as a legacy ContentId.
fn content_uri<'a>(
    db: &ReflectionDatabase,
    class: &str,
    key: &str,
    value: &'a Variant,
) -> Option<&'a str> {
    match value {
        Variant::Content(rbx_dom::Content::Uri(uri)) => Some(uri),
        Variant::String(text) if db.is_content_id(class, key) => Some(text),
        _ => None,
    }
}

/// Every instance in `dom`, parents before children.
fn instances(dom: &WeakDom) -> impl Iterator<Item = &Instance> {
    let mut stack: Vec<Ref> = dom.root_refs().iter().rev().copied().collect();
    std::iter::from_fn(move || {
        let instance = dom.get(stack.pop()?)?;
        stack.extend(instance.children().iter().rev());
        Some(instance)
    })
}
