//! Weak reference type for instances in a DOM tree.

/// A weak reference to an instance within a single WeakDom.
///
/// IDs are unique within a given DOM only; two different files can reuse the same ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ref(u32);

impl Ref {
    pub const fn new(id: u32) -> Self {
        Ref(id)
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}
