//! What the API dump leaves out: the value each property of a freshly created
//! instance holds, and which properties a file stores under a name of their
//! own (`BasePart.Size` as `size`, `BasePart.Color` as `Color3uint8`).
//!
//! Read from `assets/reflection-defaults.json`, which
//! `scripts/reflection-defaults.sh` extracts from rbx-dom's reflection
//! database (MIT). rbx-dom generates that database from Studio itself; the
//! API dump records only types.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use rbx_dom::Variant;
use serde::Deserialize;
use serde_json::Value;

use crate::database::ReflectionDatabase;
use decode::variant;

#[derive(Debug, Default)]
pub(crate) struct Defaults {
    classes: HashMap<String, ClassDefaults>,
    /// Each class's [`Rename`]s, worked out the first time a class is asked
    /// for: a place holds thousands of instances of a few dozen classes,
    /// and reading one renames each of them.
    renames: RwLock<HashMap<String, Arc<[Rename]>>>,
}

/// A spelling a file may use for a property, the name Roblox saves it
/// under, and the property itself — `("Color", "Color3uint8", "Color")`,
/// or `("size", "size_xml", "Size")` for a `Fire`.
#[derive(Debug, Clone)]
pub(crate) struct Rename {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) canonical: String,
}

#[derive(Debug, Default, Deserialize)]
struct ClassDefaults {
    /// A name a file may store, and the property it is a spelling of.
    #[serde(rename = "Aliases", default)]
    aliases: HashMap<String, String>,
    /// A property, and the name Roblox saves it under.
    #[serde(rename = "SerializesAs", default)]
    serializes_as: HashMap<String, String>,
    /// A property an alias names that Roblox never saves, so never loads:
    /// `PackageId`. Read under its alias, but never renamed to.
    #[serde(rename = "NotLoaded", default)]
    not_loaded: HashSet<String>,
    #[serde(skip)]
    values: HashMap<String, Variant>,
    #[serde(rename = "Defaults", default)]
    raw_values: HashMap<String, Value>,
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(rename = "Classes")]
    classes: HashMap<String, ClassDefaults>,
}

impl Defaults {
    /// `weight` names a `FontWeight` member's value — the file spells a
    /// font's weight by name, the DOM stores the number.
    pub(crate) fn parse(
        json: &str,
        weight: impl Fn(&str) -> Option<u16>,
    ) -> Result<Self, serde_json::Error> {
        let raw: RawFile = serde_json::from_str(json)?;
        let mut classes = raw.classes;
        for class in classes.values_mut() {
            // A value this conversion does not understand costs that one
            // property its default, never the rest of the file.
            class.values = std::mem::take(&mut class.raw_values)
                .into_iter()
                .filter_map(|(name, value)| Some((name, variant(value, &weight)?)))
                .collect();
        }
        Ok(Defaults {
            classes,
            renames: RwLock::default(),
        })
    }
}

impl ReflectionDatabase {
    /// The value `property` holds on a freshly created `class`, as Studio
    /// reports it. Only a class Studio can create has any: an abstract
    /// `BasePart` holds nothing, while every `Part` default — inherited ones
    /// included — is recorded under `Part` itself.
    pub fn default_value(&self, class: &str, property: &str) -> Option<&Variant> {
        self.defaults.classes.get(class)?.values.get(property)
    }

    /// The property `name` is a spelling of: `Size` for a stored `size`,
    /// `Color` for `Color3uint8`. Any other name is its own.
    pub fn canonical_name<'a>(&'a self, class: &str, name: &'a str) -> &'a str {
        self.lineage(class)
            .find_map(|class| self.defaults.classes.get(class)?.aliases.get(name))
            .map_or(name, String::as_str)
    }

    /// Every name a file may hold `name`'s value under, the one Roblox saves
    /// first — `["size", "Size"]` for `BasePart.Size`. A value that has none
    /// of these stored is its class default.
    pub fn stored_names<'a>(&'a self, class: &str, name: &'a str) -> Vec<&'a str> {
        let canonical = self.canonical_name(class, name);
        let mut names: Vec<&str> = self
            .lineage(class)
            .find_map(|class| {
                self.defaults
                    .classes
                    .get(class)?
                    .serializes_as
                    .get(canonical)
            })
            .map(String::as_str)
            .into_iter()
            .chain([canonical])
            .collect();
        let mut aliases: Vec<&str> = self
            .lineage(class)
            .filter_map(|class| self.defaults.classes.get(class))
            .flat_map(|class| &class.aliases)
            .filter(|(alias, target)| *target == canonical && !names.contains(&alias.as_str()))
            .map(|(alias, _)| alias.as_str())
            .collect();
        // Sorted only so two runs agree: the map's own order is random.
        aliases.sort_unstable();
        aliases.dedup();
        names.extend(aliases);
        names
    }

    /// Every spelling a file may use for a property of `class` other than
    /// the one Roblox saves, the nearest class's first where two name one
    /// spelling.
    pub(crate) fn renames(&self, class: &str) -> Arc<[Rename]> {
        if let Some(renames) = self
            .defaults
            .renames
            .read()
            .ok()
            .and_then(|renames| renames.get(class).cloned())
        {
            return renames;
        }
        let renames: Arc<[Rename]> = self
            .build_renames(class)
            .into_iter()
            .map(|(from, to, canonical)| Rename {
                from: from.to_owned(),
                to: to.to_owned(),
                canonical: canonical.to_owned(),
            })
            .collect();
        if let Ok(mut memo) = self.defaults.renames.write() {
            memo.insert(class.to_owned(), Arc::clone(&renames));
        }
        renames
    }

    fn build_renames(&self, class: &str) -> Vec<(&str, &str, &str)> {
        let mut renames: Vec<(&str, &str, &str)> = Vec::new();
        for class_defaults in self
            .lineage(class)
            .filter_map(|class| self.defaults.classes.get(class))
        {
            let mut spellings: Vec<(&str, &str)> = class_defaults
                .serializes_as
                .keys()
                .map(|canonical| (canonical.as_str(), canonical.as_str()))
                .chain(
                    class_defaults
                        .aliases
                        .iter()
                        .map(|(alias, canonical)| (alias.as_str(), canonical.as_str())),
                )
                .collect();
            // The first rename to reach a saved name keeps its value, so the
            // maps' random order would pick a different one per run where a
            // file holds two spellings of one saved name (`MaxDistance` and
            // `RollOffMaxDistance`): the current property first, then by name.
            spellings.sort_unstable_by_key(|&(spelling, canonical)| {
                let deprecated = self
                    .resolve_property(class, canonical)
                    .is_some_and(|property| property.is_deprecated());
                (deprecated, spelling)
            });
            for (spelling, canonical) in spellings {
                if class_defaults.not_loaded.contains(canonical) {
                    continue;
                }
                let Some(&saved) = self.stored_names(class, canonical).first() else {
                    continue;
                };
                if spelling != saved && !renames.iter().any(|(from, ..)| *from == spelling) {
                    renames.push((spelling, saved, canonical));
                }
            }
        }
        renames
    }
}

mod decode;

#[cfg(test)]
mod tests;
