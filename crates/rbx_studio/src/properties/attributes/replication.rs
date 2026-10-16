//! What Server Authority does not replicate. A predicted instance syncs its
//! custom data through attributes, within limits (creator-docs
//! `projects/server-authority`): only the first 64 attributes on the
//! instance, with names and string values of at most 50 characters. An
//! attribute past one of them still works in the editor and in a normal
//! game; it just never reaches the client's prediction.

use std::collections::BTreeMap;

use rbx_dom::{Variant, WeakDom};

const MAX_COUNT: usize = 64;
const MAX_TEXT_LEN: usize = 50;
const SERVER: u32 = 0;

/// The note shown under the Attributes section on a Server Authority place.
pub(crate) const LIMITS_HELP: &str =
    "Server Authority: a predicted instance replicates only its first 64 \
     attributes, with names and string values of 50 characters or fewer.";

/// Whether the place's `Workspace.AuthorityMode` is `Server`.
pub(crate) fn is_server_authority(dom: &WeakDom) -> bool {
    dom.root_refs()
        .iter()
        .filter_map(|&root| dom.get(root))
        .find(|instance| instance.class() == "Workspace")
        .and_then(|workspace| workspace.properties().get("AuthorityMode"))
        .is_some_and(|mode| *mode == Variant::Enum(SERVER))
}

/// Why each attribute of `attributes` will not replicate on a predicted
/// instance, in the panel's (name) order; an attribute that will is absent.
pub(crate) fn unreplicated(attributes: &BTreeMap<String, Variant>) -> BTreeMap<&str, &'static str> {
    attributes
        .iter()
        .enumerate()
        .filter_map(|(index, (name, value))| {
            let reason = if index >= MAX_COUNT {
                "Past the first 64 attributes: not replicated on predicted instances"
            } else if name.chars().count() > MAX_TEXT_LEN {
                "Name over 50 characters: not replicated on predicted instances"
            } else if matches!(value, Variant::String(text) if text.chars().count() > MAX_TEXT_LEN)
            {
                "Value over 50 characters: not replicated on predicted instances"
            } else {
                return None;
            };
            Some((name.as_str(), reason))
        })
        .collect()
}

#[cfg(test)]
mod tests;
