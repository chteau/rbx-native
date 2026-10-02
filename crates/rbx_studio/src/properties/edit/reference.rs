//! A `Ref` row (`ObjectValue.Value`, `Weld.Part0`): an instance in the place,
//! picked from the Explorer the way Studio's own panel does it — "select the
//! Adornee property. Your cursor changes. In the Explorer window, select the
//! part." (creator-docs, `ui/text-input.md`). The pick commits through the
//! panel's ordinary textual path as the target's referent, so a pick is one
//! undo step and applies to a whole multi-selection like any typed edit.
//!
//! `Variant` has no null reference: a `nil` one is an absent key, which is
//! how both file formats read their null referent back. The panel still has
//! to list such a property — a new `Weld` has no `Part0` yet, and that is
//! exactly the value someone opens the panel to set — so it stands in
//! [`NIL_REF`] for the missing value. The stand-in is never written: a commit
//! of it removes the key instead (see [`super::commit`]).

use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

/// The panel's `nil`. No file can produce it: the binary format's referents
/// are non-negative `i32`s and the XML reader numbers instances from 1, so
/// `u32::MAX` is never a real instance's referent.
pub(crate) const NIL_REF: Ref = Ref::new(u32::MAX);

const NIL: &str = "nil";

/// The text a pick commits: the referent's number, or `nil`.
pub(crate) fn ref_text(target: Ref) -> String {
    if target == NIL_REF {
        NIL.to_owned()
    } else {
        target.value().to_string()
    }
}

pub(super) fn parse_ref(text: &str) -> Result<Variant, String> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case(NIL) {
        return Ok(Variant::Ref(NIL_REF));
    }
    text.parse::<u32>()
        .map(|id| Variant::Ref(Ref::new(id)))
        .map_err(|_| format!("{text:?} is not an instance"))
}

/// The class `name` holds an instance of on `class` (`BasePart` for a
/// `Weld`'s `Part0`), or `None` when the property is not instance-typed.
fn target_class<'a>(db: &'a ReflectionDatabase, class: &str, name: &str) -> Option<&'a str> {
    let property = db.resolve_property(class, name)?;
    db.class(&property.value_type)?;
    Some(property.value_type.as_str())
}

/// What an instance-typed property holds where the instance stores
/// nothing: [`NIL_REF`], under the name Roblox saves it as. `None` for any
/// other property.
pub(super) fn nil_default(
    db: &ReflectionDatabase,
    instance: &Instance,
    name: &str,
) -> Option<(String, Variant)> {
    target_class(db, instance.class(), name)?;
    let key = *db.stored_names(instance.class(), name).first()?;
    Some((key.to_owned(), Variant::Ref(NIL_REF)))
}

/// Whether `target` may be what `name` on `class` points at: an instance
/// that exists, of the class the API dump types the property as. Whether
/// Studio's own picker refuses a wrong class is not documented; the dump's
/// type is what Roblox says the property holds, so a `Folder` in a `Part0`
/// is a value this panel declines to write.
///
/// A `Model`'s `PrimaryPart` must also be inside that model: the creator
/// docs say an outside part "will be set to that part but reset to `nil`
/// during the next simulation step", so it is refused rather than written
/// as a value that never sticks.
pub(super) fn check_target(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    owner: Ref,
    class: &str,
    name: &str,
    target: Ref,
) -> Result<(), String> {
    if target == NIL_REF {
        return Ok(());
    }
    let instance = dom
        .get(target)
        .ok_or_else(|| "that instance no longer exists".to_owned())?;
    match target_class(db, class, name) {
        Some(wanted) if !db.is_subclass_of(instance.class(), wanted) => Err(format!(
            "{name} takes a {wanted}, not a {}",
            instance.class()
        )),
        _ if name == "PrimaryPart"
            && db.is_subclass_of(class, "Model")
            && !std::iter::successors(dom.parent(target), |&r| dom.parent(r))
                .any(|r| r == owner) =>
        {
            Err(format!("{name} must be a part inside this {class}"))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
