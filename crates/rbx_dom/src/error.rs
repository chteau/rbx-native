//! Error types for `WeakDom` mutation.

use thiserror::Error;

use crate::reference::Ref;

/// Errors that can occur while mutating a `WeakDom`.
///
/// Only referent validity is checked here; property *type* validation against a class's
/// reflection data is a caller concern (it needs a `ReflectionDatabase`, which this crate
/// does not depend on) and is deliberately out of scope.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum DomError {
    #[error("no instance exists for referent {0:?}")]
    UnknownInstance(Ref),
}
