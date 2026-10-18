//! Three-way merge of place DOMs: a common `base` and two descendants of
//! it, `ours` and `theirs`, merged into one DOM plus the conflicts neither
//! side's edit could settle on its own.
//!
//! Instances are matched across the three by their `UniqueId` property,
//! which Studio keeps stable across saves; referents are not (every save
//! renumbers them). An instance with no usable `UniqueId` (a legacy or XML
//! file, or one this editor created) is matched by position instead: under
//! parents already matched, the i-th unmatched child of a given class and
//! name pairs with the i-th one on the other side. Two instances with
//! different `UniqueId`s never pair. Instances added on both sides pair only
//! by `UniqueId`: two people each adding a `Part` to `Workspace` added two
//! parts.
//!
//! Every value is merged on its own — the name, the class, each property,
//! each attribute, the tag set, the parent, and a script's `Source` line by
//! line — with the usual rule: a side that left a value as the base had it
//! takes the other side's, two equal values are no conflict, and two
//! different edits are one [`Conflict`]. `Ref` values are compared by the
//! instance they name, not by referent. An unresolved conflict takes ours.
//!
//! The output keeps ours' referents for every instance ours has, so it can
//! be laid over the DOM ours came from (see [`apply`]); an instance only
//! theirs has gets a referent above everything ours ever allocated, fixed
//! when the [`Merge`] is made so that resolving conflicts never renumbers
//! anything.

mod apply;
mod identity;
mod nodes;
mod props;
mod text;
mod tree;

pub use apply::apply;
pub use text::{merge_text, TextChunk};

use crate::{Ref, UniqueId, Variant, WeakDom};

/// One of the three inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Base,
    Ours,
    Theirs,
}

/// What a [`Conflict`] is about.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    Name,
    Class,
    /// An ordinary property (never `Source` on a script, `Tags`,
    /// `AttributesSerialize` or `UniqueId`: those merge their own way).
    Property(String),
    /// One attribute of `AttributesSerialize`, by name.
    Attribute(String),
    /// The n-th conflicting hunk of a script's `Source`, counting only the
    /// conflicting ones, in text order.
    Source(usize),
    /// Both sides moved the instance, to different parents (or theirs' move
    /// would put it under its own descendant).
    Parent,
    /// One side removed the instance while the other changed it or
    /// something under it.
    Existence {
        removed_by: Side,
    },
}

/// How a conflict is settled.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    Base,
    Ours,
    Theirs,
    /// A value of the caller's own: a `Variant::String` for a name, class
    /// or source hunk (the hunk's replacement lines, terminators included),
    /// any value for a property or attribute (`Ref`s naming output
    /// referents). Not accepted for `Parent` or `Existence`.
    Custom(Variant),
}

/// Why [`Merge::resolve`] refused a resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    UnknownConflict(usize),
    /// `Base` for `Existence`, `Custom` for `Parent`/`Existence`, or a
    /// non-string `Custom` for a name, class or source hunk.
    Unsupported(usize),
}

/// One value two sides edited differently.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    /// Index into [`Merge::conflicts`].
    pub id: usize,
    pub target: Target,
    /// The instance's referent in [`Merge::output`]: ours' referent when
    /// ours has it, else the one reserved for it. Fixed for the merge's
    /// lifetime, whether or not the current resolutions keep the instance.
    pub instance: Ref,
    pub class: String,
    pub name: String,
    /// The instance's full name (`Workspace.Model.Part`), ours' when ours
    /// has it, else theirs', else base's.
    pub path: String,
    pub unique_id: Option<UniqueId>,
    /// The value on each side; `None` is "absent". Name and class are
    /// `Variant::String`s; a property's or attribute's `Ref`s are translated
    /// to output referents (absent when the instance named is not in the
    /// default output); a source hunk is its lines; a parent is the parent's
    /// full name (`""` for the root); for `Existence` the removing side is
    /// `None` and the others are the instance's full name.
    pub base: Option<Variant>,
    pub ours: Option<Variant>,
    pub theirs: Option<Variant>,
    pub resolution: Option<Resolution>,
}

/// A three-way merge in progress: the inputs, the conflicts found, and the
/// resolutions chosen so far.
#[derive(Debug, Clone)]
pub struct Merge {
    base: WeakDom,
    ours: WeakDom,
    theirs: WeakDom,
    nodes: nodes::Nodes,
    conflicts: Vec<Conflict>,
}

impl Merge {
    pub fn new(base: WeakDom, ours: WeakDom, theirs: WeakDom) -> Merge {
        let nodes = nodes::Nodes::new(&base, &ours, &theirs);
        let (_, conflicts) = tree::build(&base, &ours, &theirs, &nodes, &|_, _| None);
        Merge {
            base,
            ours,
            theirs,
            nodes,
            conflicts,
        }
    }

    pub fn base(&self) -> &WeakDom {
        &self.base
    }

    pub fn ours(&self) -> &WeakDom {
        &self.ours
    }

    pub fn theirs(&self) -> &WeakDom {
        &self.theirs
    }

    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    pub fn unresolved(&self) -> usize {
        self.conflicts
            .iter()
            .filter(|c| c.resolution.is_none())
            .count()
    }

    pub fn resolve(&mut self, id: usize, resolution: Resolution) -> Result<(), ResolveError> {
        let conflict = self
            .conflicts
            .get_mut(id)
            .ok_or(ResolveError::UnknownConflict(id))?;
        let supported = match (&conflict.target, &resolution) {
            (Target::Existence { .. }, Resolution::Base | Resolution::Custom(_)) => false,
            (Target::Parent, Resolution::Custom(_)) => false,
            (Target::Name | Target::Class | Target::Source(_), Resolution::Custom(value)) => {
                matches!(value, Variant::String(_))
            }
            _ => true,
        };
        if !supported {
            return Err(ResolveError::Unsupported(id));
        }
        conflict.resolution = Some(resolution);
        Ok(())
    }

    /// Takes back a resolution; the conflict takes ours again.
    pub fn unresolve(&mut self, id: usize) {
        if let Some(conflict) = self.conflicts.get_mut(id) {
            conflict.resolution = None;
        }
    }

    /// The merged DOM under the resolutions chosen so far.
    pub fn output(&self) -> WeakDom {
        let choose = |instance: Ref, target: &Target| {
            self.conflicts
                .iter()
                .find(|c| c.instance == instance && &c.target == target)
                .and_then(|c| c.resolution.clone())
        };
        tree::build(&self.base, &self.ours, &self.theirs, &self.nodes, &choose).0
    }
}

/// [`Merge::new`] then [`Merge::output`] with nothing resolved: the merged
/// DOM, conflicts taking ours, and the conflicts.
pub fn merge(base: &WeakDom, ours: &WeakDom, theirs: &WeakDom) -> (WeakDom, Vec<Conflict>) {
    let merge = Merge::new(base.clone(), ours.clone(), theirs.clone());
    (merge.output(), merge.conflicts)
}
