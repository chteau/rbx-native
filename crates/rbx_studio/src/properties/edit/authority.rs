//! Setting `Workspace.AuthorityMode` to `Server` turns on the five other
//! `Workspace` properties Server Authority needs, as Studio does (creator-docs
//! `projects/server-authority`: "setting this automatically sets the
//! following five").
//!
//! The writes happen inside the one `commit_all` an edit is, so the whole
//! thing is a single undo step: `Shell::apply_edit` snapshots the DOM before
//! it and pushes it once. Switching back to `Automatic` leaves the five as
//! they are — the docs don't say Studio reverts them, and a creator may have
//! wanted `StreamingEnabled` on for its own sake.

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::commit;

const CLASS: &str = "Workspace";
pub(super) const PROPERTY: &str = "AuthorityMode";
const SERVER: &str = "Server";

/// The companions, each with the text that enables it.
const COMPANIONS: [(&str, &str); 5] = [
    ("NextGenerationReplication", "Enabled"),
    ("PlayerScriptsUseInputActionSystem", "Enabled"),
    ("SignalBehavior", "Deferred"),
    ("UseFixedSimulation", "Enabled"),
    ("StreamingEnabled", "true"),
];

/// After `text` was written to `name` on `selection`: if that made a
/// `Workspace` `Server`, enables its companions.
pub(super) fn follow(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    selection: &[Ref],
    name: &str,
    text: &str,
) -> Result<(), String> {
    if name != PROPERTY || text.trim() != SERVER {
        return Ok(());
    }
    for &reference in selection {
        if dom.get(reference).is_none_or(|i| i.class() != CLASS) {
            continue;
        }
        for (companion, value) in COMPANIONS {
            commit(dom, db, reference, companion, value)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
