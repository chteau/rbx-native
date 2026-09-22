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

use super::computed::COMPUTED;
use super::edit::pivot::ORIGIN;
use super::{attributes, edit::NAME_PROPERTY, Properties, UNCATEGORIZED};

/// Two properties every instance has but no file stores, read off the
/// instance itself the way `Name` is.
const CLASS_NAME: &str = "ClassName";
const PARENT: &str = "Parent";
const BRICK_COLOR: &str = "BrickColor";
const COLOR: &str = "Color";

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
    source: Source,
}

/// Where an entry's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// Stored under one of its keys, or else the class default.
    Stored,
    /// A part's `BrickColor`: the closest table colour to its `Color`, whose
    /// keys and default the entry carries. Roblox saves only `Color`.
    BrickColor,
    /// Worked out from the part itself (see `computed`).
    Computed,
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
                let mut default = db.default_value(class, &property.name).cloned();
                let mut keys = keys;
                let mut source = Source::Stored;
                if property.name == BRICK_COLOR && db.resolve_property(class, COLOR).is_some() {
                    keys = db
                        .stored_names(class, COLOR)
                        .into_iter()
                        .map(str::to_owned)
                        .collect();
                    default = db.default_value(class, COLOR).cloned();
                    source = Source::BrickColor;
                } else if default.is_none() && COMPUTED.contains(&property.name.as_str()) {
                    source = Source::Computed;
                }
                index.insert(property.name.clone(), entries.len());
                entries.push(Entry {
                    // Never saved, but typing one moves the instance there.
                    read_only: property.name != ORIGIN && read_only(property, &keys),
                    default,
                    owner: descriptor.name.clone(),
                    category: property.category.clone(),
                    name: property.name.clone(),
                    keys,
                    source,
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
                    value: self.value(entry, dom, reference, instance)?,
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
        Some((
            self.value(entry, dom, reference, instance)?,
            entry.read_only,
        ))
    }

    /// What `name` holds on `instance`, stored or defaulted — through the
    /// class's sheet, which already knows its names, rather than asking the
    /// database to walk the class hierarchy for them again. `computed`
    /// reads a dozen of these per part on every frame the panel draws.
    pub(super) fn read(&self, instance: &Instance, name: &str) -> Option<Variant> {
        let sheet = self.sheet(instance.class());
        match sheet.index.get(name).map(|&index| &sheet.entries[index]) {
            Some(entry) if entry.source == Source::Stored => stored(entry, instance).cloned(),
            _ => self
                .db
                .stored_or_default(instance, name)
                .map(|(_, value)| value.clone()),
        }
    }

    /// What `entry` holds on `instance`.
    fn value<'a>(
        &self,
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
            _ => match entry.source {
                Source::Stored => Cow::Borrowed(stored(entry, instance)?),
                Source::BrickColor => Cow::Owned(brick_color(stored(entry, instance)?)?),
                Source::Computed => {
                    Cow::Owned(self.computed(dom, reference, instance, &entry.name)?)
                }
            },
        })
    }
}

/// Stored under any of `entry`'s keys, or else its class default.
fn stored<'a>(entry: &'a Entry, instance: &'a Instance) -> Option<&'a Variant> {
    entry
        .keys
        .iter()
        .find_map(|key| instance.properties().get(key))
        .or(entry.default.as_ref())
}

/// The closest table colour to a part's `Color`.
fn brick_color(color: &Variant) -> Option<Variant> {
    let rgb = match color {
        Variant::Color3uint8 { r, g, b } => [*r, *g, *b],
        Variant::Color3(color) => [color.r, color.g, color.b]
            .map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8),
        _ => return None,
    };
    Some(Variant::BrickColor(
        rbx_dom::BrickColor::nearest(rgb).number,
    ))
}

#[cfg(test)]
mod tests;
