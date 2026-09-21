//! Which rows an instance gets: every property Studio's own panel lists for
//! its class, holding the value the file stored — under whichever name it
//! was saved as — or else the class default, the way Studio fills in
//! whatever a file leaves out when it creates the instance.
//!
//! What to list is worked out once per class (a [`Sheet`]) and kept, since
//! the panel asks again on every frame it draws.
//!
//! Roblox's docs do not publish the panel's own filter, so the rule here is
//! read off the API dump's tags:
//!
//! - `Hidden` is out: the tag's whole meaning, e.g. `BasePart.Position`,
//!   which Studio edits through its own Position/Orientation UI instead.
//! - `Deprecated` is out. Every deprecated member the dump does not already
//!   hide is either an old spelling of a property listed under its current
//!   name (`className`, `Fire.size`, `BodyGyro.maxTorque`) — listing it would
//!   show one value twice — or, per its own `deprecation_message` in
//!   creator-docs, superseded or inert (`Sound.Pitch` for `PlaybackSpeed`,
//!   `FormFactorPart.FormFactor`, which "no longer does anything").
//! - Security and `NotScriptable` keep nothing out: creator-docs describes
//!   `Lighting.Technology` — `NotScriptable`, `RobloxScriptSecurity` — as
//!   "only modifiable in Studio", which is to say in this panel.
//!
//! A property with neither a stored value nor a recorded default is left
//! out too: what `Mass` or `AssemblyLinearVelocity` hold is computed by a
//! running engine, and there is nothing true to show for it here.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::{PropertyDescriptor, ReflectionDatabase};

use super::{attributes, edit::NAME_PROPERTY, Properties, UNCATEGORIZED};

/// Two properties every instance has but no file stores, read off the
/// instance itself the way `Name` is.
const CLASS_NAME: &str = "ClassName";
const PARENT: &str = "Parent";

/// One listed property of a class.
pub(super) struct Entry {
    name: String,
    /// The class that declares it: two classes' same-named properties are
    /// only one property when they share this (a `Part`'s and a `Frame`'s
    /// `Size` are not).
    owner: String,
    category: String,
    /// Every name a file may hold the value under, the saved one first.
    keys: Vec<String>,
    default: Option<Variant>,
    read_only: bool,
}

/// Everything [`Properties::named`] needs about one class.
pub(super) struct Sheet {
    entries: Vec<Entry>,
    /// Where each entry sits in `entries`, by name.
    index: HashMap<String, usize>,
    /// Every stored name some reflected property of the class answers to,
    /// listed or not — what keeps a hidden property, or a second spelling of
    /// a listed one, out of the "Other" section.
    claimed: HashSet<String>,
}

impl Sheet {
    fn build(db: &ReflectionDatabase, class: &str) -> Sheet {
        let mut entries = Vec::new();
        let mut index = HashMap::new();
        let mut claimed = HashSet::new();
        let mut seen = HashSet::new();
        let mut current = db.class(class);
        while let Some(descriptor) = current {
            for property in &descriptor.properties {
                // Nearest first, so a subclass's own declaration wins.
                if !seen.insert(property.name.as_str()) {
                    continue;
                }
                let keys: Vec<String> = db
                    .stored_names(class, &property.name)
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                claimed.extend(keys.iter().cloned());
                // A spelling of another property is that property's row.
                if !listed(property) || db.canonical_name(class, &property.name) != property.name {
                    continue;
                }
                index.insert(property.name.clone(), entries.len());
                entries.push(Entry {
                    read_only: read_only(property, &keys),
                    default: db.default_value(class, &property.name).cloned(),
                    owner: descriptor.name.clone(),
                    category: property.category.clone(),
                    name: property.name.clone(),
                    keys,
                });
            }
            current = descriptor
                .superclass
                .as_deref()
                .and_then(|name| db.class(name));
        }
        Sheet {
            entries,
            index,
            claimed,
        }
    }
}

fn listed(property: &PropertyDescriptor) -> bool {
    !property.is_hidden() && !property.is_deprecated()
}

/// The dump's `ReadOnly`, or a property Studio never saves. A property saved
/// under another name is still saved: the dump reports `BasePart.Size` as
/// `CanSave: false` only because what a file holds is `size`.
fn read_only(property: &PropertyDescriptor, keys: &[String]) -> bool {
    let saved_elsewhere = keys.first().is_some_and(|key| *key != property.name);
    property.tags.iter().any(|tag| tag == "ReadOnly") || (!property.can_save && !saved_elsewhere)
}

/// One property of one instance, with the value it holds.
pub(super) struct Named<'a> {
    pub(super) name: &'a str,
    pub(super) owner: &'a str,
    pub(super) category: &'a str,
    pub(super) read_only: bool,
    pub(super) value: Cow<'a, Variant>,
}

impl Properties {
    /// `class`'s sheet, built the first time it is asked for.
    pub(super) fn sheet(&self, class: &str) -> Rc<Sheet> {
        if let Some(sheet) = self.sheets.borrow().get(class) {
            return Rc::clone(sheet);
        }
        let sheet = Rc::new(Sheet::build(&self.db, class));
        self.sheets
            .borrow_mut()
            .insert(class.to_owned(), Rc::clone(&sheet));
        sheet
    }

    /// Every property `instance` gets a row for, in no particular order.
    pub(super) fn named<'a>(
        &'a self,
        dom: &'a WeakDom,
        reference: Ref,
        instance: &'a Instance,
        sheet: &'a Sheet,
    ) -> Vec<Named<'a>> {
        let class = instance.class();
        let mut named: Vec<Named> = sheet
            .entries
            .iter()
            .filter_map(|entry| {
                Some(Named {
                    name: &entry.name,
                    owner: &entry.owner,
                    category: &entry.category,
                    read_only: entry.read_only,
                    value: value(entry, dom, reference, instance)?,
                })
            })
            .collect();

        // What the dump has never heard of — a newer engine's property, say
        // — still shows, under its canonical name, so an edit can reach it.
        let mut unreflected = HashSet::new();
        for key in instance.properties().keys() {
            if sheet.claimed.contains(key) || attributes::is_backing_store(key) {
                continue;
            }
            let name = self.db.canonical_name(class, key);
            if !unreflected.insert(name) {
                continue;
            }
            let Some(value) = self
                .db
                .stored_names(class, name)
                .into_iter()
                .find_map(|key| instance.properties().get(key))
            else {
                continue;
            };
            named.push(Named {
                name,
                owner: "",
                category: UNCATEGORIZED,
                read_only: false,
                value: Cow::Borrowed(value),
            });
        }
        named
    }

    /// The property `named` stands for, on another selected instance: its
    /// value there and whether it is read-only there, or `None` where that
    /// instance has no such property. One lookup rather than all of
    /// [`Self::named`], since a multi-selection asks this of every instance
    /// for every row.
    pub(super) fn value_in<'a>(
        &self,
        named: &Named,
        dom: &'a WeakDom,
        reference: Ref,
        instance: &'a Instance,
        sheet: &'a Sheet,
    ) -> Option<(Cow<'a, Variant>, bool)> {
        if named.owner.is_empty() {
            // Stored under the name it shows as, nearly always: no need to
            // ask the database for its other spellings.
            let value = match instance.properties().get(named.name) {
                Some(value) if !sheet.claimed.contains(named.name) => value,
                _ => self
                    .db
                    .stored_names(instance.class(), named.name)
                    .into_iter()
                    .filter(|key| !sheet.claimed.contains(*key))
                    .find_map(|key| instance.properties().get(key))?,
            };
            return Some((Cow::Borrowed(value), false));
        }
        let entry = &sheet.entries[*sheet.index.get(named.name)?];
        if entry.owner != named.owner {
            return None;
        }
        Some((value(entry, dom, reference, instance)?, entry.read_only))
    }
}

/// What `entry` holds on `instance`: stored under any of its names, or else
/// its class default.
fn value<'a>(
    entry: &'a Entry,
    dom: &WeakDom,
    reference: Ref,
    instance: &'a Instance,
) -> Option<Cow<'a, Variant>> {
    Some(match entry.name.as_str() {
        NAME_PROPERTY => Cow::Owned(Variant::String(instance.name().to_owned())),
        CLASS_NAME => Cow::Owned(Variant::String(instance.class().to_owned())),
        // A service has no parent in the DOM; `nil` would be wrong, since
        // Studio's is the place itself.
        PARENT => Cow::Owned(Variant::Ref(dom.parent(reference)?)),
        _ => entry
            .keys
            .iter()
            .find_map(|key| instance.properties().get(key))
            .or(entry.default.as_ref())
            .map(Cow::Borrowed)?,
    })
}

#[cfg(test)]
mod tests;
