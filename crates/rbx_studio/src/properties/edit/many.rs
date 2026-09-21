//! Where an edit lands, and one edit made to every selected instance at
//! once.

use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{commit, edit_text, parse, NAME_PROPERTY};
use crate::properties::{value_edit_kind, EditKind};

/// The name `name`'s value is stored under on `instance`, and that value —
/// or, for a value the file never stored, the name Roblox saves it under and
/// the class default. Either spelling of a property finds it: `Size` and
/// `size` both land on the `size` the renderer and the save path read.
pub(super) fn stored_or_default(
    db: &ReflectionDatabase,
    instance: &Instance,
    name: &str,
) -> Option<(String, Variant)> {
    let class = instance.class();
    let names = db.stored_names(class, name);
    names
        .iter()
        .find_map(|key| Some((key.to_string(), instance.properties().get(*key)?.clone())))
        .or_else(|| {
            let default = db.default_value(class, db.canonical_name(class, name))?;
            Some((names.first()?.to_string(), default.clone()))
        })
}

/// [`commit`] for every instance in `selection`, as one edit: every value is
/// parsed before any is written, so a value one instance rejects changes
/// none of them, and an instance already holding the result is not written
/// at all — a focus leaving an untouched field adds nothing to a file.
///
/// Where the instances' values differ, the panel showed the row empty (see
/// `properties::common`), and what was left empty keeps each instance's own
/// value: nothing typed changes nothing, and typing only a part's height
/// sets every part's height alone.
pub(crate) fn commit_all(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    selection: &[Ref],
    name: &str,
    text: &str,
) -> Result<(), String> {
    let currents: Vec<(String, Variant)> = selection
        .iter()
        .map(|&reference| current(dom, db, reference, name))
        .collect::<Result<_, _>>()?;
    let mixed = currents.windows(2).any(|pair| pair[0].1 != pair[1].1);

    let mut writes = Vec::with_capacity(selection.len());
    for (&reference, (class, value)) in selection.iter().zip(&currents) {
        let text = if mixed {
            fill_blanks(value, text)
        } else {
            text.to_owned()
        };
        if parse(value, db, class, db.canonical_name(class, name), &text)? != *value {
            writes.push((reference, text));
        }
    }
    for (reference, text) in writes {
        commit(dom, db, reference, name, &text)?;
    }
    Ok(())
}

/// An instance's class, and the value `name` holds on it.
fn current(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    reference: Ref,
    name: &str,
) -> Result<(String, Variant), String> {
    let instance = dom
        .get(reference)
        .ok_or_else(|| "the instance no longer exists".to_string())?;
    let value = if name == NAME_PROPERTY {
        Variant::String(instance.name().to_owned())
    } else {
        stored_or_default(db, instance, name)
            .ok_or_else(|| format!("{name} has no current value to type-check against"))?
            .1
    };
    Ok((instance.class().to_owned(), value))
}

/// `typed` with whatever was left empty taken from `current` instead: all
/// of it for a blank field, one part at a time for a value made of parts.
fn fill_blanks(current: &Variant, typed: &str) -> String {
    let Some(own) = edit_text(current) else {
        return typed.to_owned();
    };
    if typed.trim().is_empty() {
        return own;
    }
    // Only a value the panel splits into fields: a string's own commas are
    // text, not parts.
    let parts = matches!(
        value_edit_kind(current, own.clone()),
        EditKind::Fields { .. } | EditKind::Groups { .. } | EditKind::Optional { .. }
    );
    let own_parts: Vec<&str> = own.split(',').map(str::trim).collect();
    let typed_parts: Vec<&str> = typed.split(',').map(str::trim).collect();
    if !parts || own_parts.len() != typed_parts.len() {
        return typed.to_owned();
    }
    typed_parts
        .iter()
        .zip(own_parts)
        .map(|(typed, own)| if typed.is_empty() { own } else { typed })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests;
