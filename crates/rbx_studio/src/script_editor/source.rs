//! Reading and writing a script instance's `Source` on the live DOM.
//!
//! Deliberately not routed through `properties::edit::commit`, which every
//! other property edit goes through: that path trims the text it commits
//! (see `properties::edit::parse`'s `String` arm), which is right for a
//! one-line field and destructive for source code, where a trailing newline
//! and a leading blank line are both part of what the author typed. It writes
//! through the same `WeakDom::set_property` that path ends at, so undo, Ctrl+S
//! and the Properties panel all see an ordinary property edit and need to know
//! nothing about the script editor.

#[cfg(test)]
mod tests;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

/// The property a script keeps its code in.
pub(crate) const SOURCE_PROPERTY: &str = "Source";

/// The base class of everything that has a `Source` to edit: `Script`,
/// `LocalScript` and `ModuleScript` all derive from it. Asking the reflection
/// dump rather than matching those three names means a class the dump knows
/// and this list would not (`CoreScript`) still opens correctly.
const SOURCE_CONTAINER: &str = "LuaSourceContainer";

/// Whether double-clicking `reference` in the Explorer should open it in the
/// script editor.
pub(crate) fn is_script(dom: &WeakDom, db: &ReflectionDatabase, reference: Ref) -> bool {
    dom.get(reference)
        .is_some_and(|instance| db.is_subclass_of(instance.class(), SOURCE_CONTAINER))
}

/// The instance's `Source` as it stands. `None` when the referent no longer
/// resolves, or when it has no `Source` — a script inserted through the
/// Explorer has none until something writes one.
pub(crate) fn read(dom: &WeakDom, reference: Ref) -> Option<String> {
    match dom.get(reference)?.properties().get(SOURCE_PROPERTY)? {
        Variant::String(source) => Some(source.clone()),
        _ => None,
    }
}

/// Whether the instance's `Source` is already exactly `text`, answered
/// without cloning the source out of the DOM to find out — this runs once per
/// open tab on every render of the panel (see
/// `crate::shell::Shell::resync_scripts`).
pub(crate) fn is(dom: &WeakDom, reference: Ref, text: &str) -> bool {
    matches!(
        dom.get(reference)
            .and_then(|instance| instance.properties().get(SOURCE_PROPERTY)),
        Some(Variant::String(source)) if source == text
    )
}

/// Writes `source` to the instance's `Source`, reporting whether the DOM
/// actually changed.
///
/// Writing a value already there is reported as no change and skipped, which
/// is what stops an editor re-seeded from the DOM (after an undo, say) from
/// echoing that same text straight back as a fresh edit — and so from burying
/// the undo the user just asked for under a new history entry.
pub(crate) fn write(dom: &mut WeakDom, reference: Ref, source: &str) -> bool {
    if is(dom, reference, source) {
        return false;
    }
    dom.set_property(
        reference,
        SOURCE_PROPERTY,
        Variant::String(source.to_owned()),
    )
    .is_ok()
}

/// The tab label for a script: its instance name, which follows a rename
/// because it is read back out of the DOM on every render rather than cached
/// when the tab opened.
pub(crate) fn label(dom: &WeakDom, reference: Ref) -> Option<String> {
    Some(dom.get(reference)?.name().to_owned())
}
