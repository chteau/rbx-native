//! The editor's [`Host`]: the methods of the ABI over a `WeakDom` and a
//! selection.

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::Host;

/// Borrows the place and the selection for one call into a plugin.
/// `set_property` goes through `WeakDom::set_property` (so the change log
/// sees it); `new` applies no defaults beyond the class and name.
pub struct DomHost<'a> {
    pub dom: &'a mut WeakDom,
    pub database: &'a ReflectionDatabase,
    pub selection: &'a mut Vec<Ref>,
    /// Set when `selection.set` ran.
    pub selection_changed: bool,
}

impl Host for DomHost<'_> {
    fn call(&mut self, method: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        todo!()
    }
}
