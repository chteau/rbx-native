//! Records of mutations applied to a `WeakDom`, drained by `WeakDom::take_changes`.

use crate::reference::Ref;

/// A single mutation applied to a `WeakDom` since its change log was last cleared.
///
/// A consumer that wants to stay in sync with an edited DOM (a Properties panel, a
/// viewport renderer) can drain this list after a batch of edits and update only what
/// it names, instead of re-scanning every instance.
///
/// Renaming an instance is reported as `Property` with `name` set to `"Name"` rather
/// than through a dedicated variant: a rename is, semantically, just another property
/// write, and reusing `Property` keeps this enum's shape uniform for consumers that
/// dispatch on `name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The property named `name` on `referent` was written (inserted or overwritten).
    /// The new value is not carried here; read it back from the DOM if needed.
    Property { referent: Ref, name: String },
    /// `referent` moved from parent `old` to parent `new` (`None` means the root level).
    Parent {
        referent: Ref,
        old: Option<Ref>,
        new: Option<Ref>,
    },
    /// A new instance was inserted into the DOM.
    Added(Ref),
    /// An instance was deleted. `WeakDom::remove` emits one of these per removed
    /// instance, including every descendant in the removed subtree.
    Removed(Ref),
    /// `referent`'s class changed in place (see `WeakDom::set_class`). Structural
    /// to every consumer: what an instance is decides which pass draws it, which
    /// icon it carries and which properties it lists, so it is re-read whole
    /// rather than patched one value at a time.
    Class(Ref),
}
