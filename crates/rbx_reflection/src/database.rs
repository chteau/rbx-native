//! In-memory index of class and enum metadata.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use crate::class::{ClassDescriptor, PropertyDescriptor};
use crate::enums::EnumDescriptor;
use crate::error::ReflectionError;
use crate::parse::parse_dump;

/// A searchable collection of Roblox class and enum descriptors.
///
/// Supports property resolution via superclass inheritance and enum value lookup.
#[derive(Debug, Clone)]
pub struct ReflectionDatabase {
    classes: HashMap<String, ClassDescriptor>,
    enums: HashMap<String, EnumDescriptor>,
}

impl ReflectionDatabase {
    /// Parses a JSON API dump into a searchable database.
    pub fn from_json_str(json: &str) -> Result<Self, ReflectionError> {
        let (classes, enums) = parse_dump(json)?;

        Ok(ReflectionDatabase {
            classes: classes.into_iter().map(|c| (c.name.clone(), c)).collect(),
            enums: enums.into_iter().map(|e| (e.name.clone(), e)).collect(),
        })
    }

    /// Reads a JSON API dump from any reader and builds a database.
    pub fn from_reader<R: Read>(mut reader: R) -> Result<Self, ReflectionError> {
        let mut json = String::new();
        reader.read_to_string(&mut json)?;
        Self::from_json_str(&json)
    }

    /// Loads a JSON API dump file from disk and builds a database.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ReflectionError> {
        let json = fs::read_to_string(path)?;
        Self::from_json_str(&json)
    }

    /// Looks up a class descriptor by name.
    pub fn class(&self, name: &str) -> Option<&ClassDescriptor> {
        self.classes.get(name)
    }

    /// Resolves a property descriptor by walking the superclass chain.
    ///
    /// Most properties (Name, Parent, etc.) are declared once on `Instance` and
    /// inherited by every other class. This method searches from the given class
    /// up through its superclass chain until a match is found.
    pub fn resolve_property(
        &self,
        class_name: &str,
        prop_name: &str,
    ) -> Option<&PropertyDescriptor> {
        let mut current = self.classes.get(class_name);

        while let Some(class) = current {
            if let Some(prop) = class.properties.iter().find(|p| p.name == prop_name) {
                return Some(prop);
            }
            current = class
                .superclass
                .as_deref()
                .and_then(|name| self.classes.get(name));
        }

        None
    }

    /// Tests whether a class is the ancestor class itself or inherits from it.
    ///
    /// Walks the superclass chain from `class` upward. Classes unknown to the API dump
    /// return `false`: we cannot assume anything about unknown names, and assume the dump
    /// is the sole authority on the hierarchy.
    pub fn is_subclass_of(&self, class: &str, ancestor: &str) -> bool {
        let mut current = self.classes.get(class);

        while let Some(descriptor) = current {
            if descriptor.name == ancestor {
                return true;
            }
            current = descriptor
                .superclass
                .as_deref()
                .and_then(|name| self.classes.get(name));
        }

        false
    }

    /// Tags carried by a class in the API dump (e.g. `Service`, `NotCreatable`,
    /// `NotBrowsable`). An unknown class or one with no tags returns an empty slice.
    pub fn class_tags(&self, class: &str) -> &[String] {
        self.classes
            .get(class)
            .map(|descriptor| descriptor.tags.as_slice())
            .unwrap_or(&[])
    }

    /// Whether the class is tagged `Service` (a singleton reachable from `DataModel`).
    pub fn is_service(&self, class: &str) -> bool {
        self.class_tags(class).iter().any(|tag| tag == "Service")
    }

    /// Whether instances of the class can be created directly.
    ///
    /// Classes are creatable by default; only an explicit `NotCreatable` tag says
    /// otherwise, so an unknown class is treated as creatable rather than refused.
    pub fn is_creatable(&self, class: &str) -> bool {
        !self
            .class_tags(class)
            .iter()
            .any(|tag| tag == "NotCreatable")
    }

    /// Whether the class should be shown in a class browser/picker.
    ///
    /// Classes are browsable by default; only an explicit `NotBrowsable` tag hides
    /// them, so an unknown class is treated as browsable rather than hidden.
    pub fn is_browsable(&self, class: &str) -> bool {
        !self
            .class_tags(class)
            .iter()
            .any(|tag| tag == "NotBrowsable")
    }

    /// Every value of an enum, in the order the dump lists them.
    pub fn enum_items(&self, enum_name: &str) -> Option<&[(String, u32)]> {
        self.enums.get(enum_name).map(|e| e.items.as_slice())
    }

    /// Converts an enum value to its symbolic name.
    pub fn enum_name(&self, enum_name: &str, value: u32) -> Option<&str> {
        self.enums
            .get(enum_name)
            .and_then(|e| e.items.iter().find(|(_, v)| *v == value))
            .map(|(name, _)| name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const API_DUMP_JSON: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/API-Dump.json"
    ));

    fn database() -> ReflectionDatabase {
        ReflectionDatabase::from_json_str(API_DUMP_JSON).expect("bundled dump must parse")
    }

    #[test]
    fn resolves_inherited_property() {
        let db = database();

        let prop = db
            .resolve_property("Part", "Name")
            .expect("Name is inherited from Instance");
        assert_eq!(prop.value_type, "string");
    }

    #[test]
    fn resolves_enum_ordinal_to_name() {
        let db = database();

        assert_eq!(db.enum_name("Material", 272), Some("SmoothPlastic"));
    }

    #[test]
    fn resolves_property_category() {
        let db = database();

        // Declared directly on Part, and inherited from Instance — both
        // carry a real Category, not the "Function"/"Event" members' blank
        // default (see `parse::RawMember::category`).
        assert_eq!(
            db.resolve_property("Part", "Anchored").unwrap().category,
            "Part"
        );
        assert_eq!(
            db.resolve_property("Part", "Name").unwrap().category,
            "Data"
        );
    }

    #[test]
    fn unreflected_property_resolves_to_none() {
        let db = database();

        assert_eq!(db.resolve_property("Part", "Tags"), None);
    }

    #[test]
    fn subclass_walks_the_whole_superclass_chain() {
        let db = database();

        // SpawnLocation -> Part -> FormFactorPart -> BasePart.
        assert!(db.is_subclass_of("SpawnLocation", "BasePart"));
        assert!(db.is_subclass_of("Part", "Instance"));
    }

    #[test]
    fn a_class_is_a_subclass_of_itself() {
        let db = database();

        assert!(db.is_subclass_of("BasePart", "BasePart"));
    }

    #[test]
    fn unrelated_and_unknown_classes_are_not_subclasses() {
        let db = database();

        assert!(!db.is_subclass_of("Folder", "BasePart"));
        assert!(!db.is_subclass_of("BasePart", "Part"));
        assert!(!db.is_subclass_of("NotAClass", "Instance"));
    }

    #[test]
    fn is_service_matches_the_service_tag() {
        let db = database();

        assert!(db.is_service("Workspace"));
        assert!(!db.is_service("Part"));
        assert!(!db.is_service("NotAClass"));
    }

    #[test]
    fn is_creatable_defaults_true_unless_tagged_not_creatable() {
        let db = database();

        // Workspace and DataModel are both singletons the engine owns.
        assert!(!db.is_creatable("DataModel"));
        assert!(!db.is_creatable("Workspace"));
        assert!(db.is_creatable("Part"));
        // Unknown classes are treated as creatable: absence of a tag is not a refusal.
        assert!(db.is_creatable("NotAClass"));
    }

    #[test]
    fn is_browsable_defaults_true_unless_tagged_not_browsable() {
        let db = database();

        // RenderSettings/NetworkSettings carry NotBrowsable in the dump.
        assert!(!db.is_browsable("RenderSettings"));
        assert!(!db.is_browsable("NetworkSettings"));
        assert!(db.is_browsable("Part"));
        assert!(db.is_browsable("Workspace"));
    }

    #[test]
    fn class_tags_is_empty_for_unknown_or_untagged_classes() {
        let db = database();

        assert_eq!(db.class_tags("NotAClass"), &[] as &[String]);
        assert_eq!(db.class_tags("Part"), &[] as &[String]);
        assert!(!db.class_tags("Workspace").is_empty());
    }

    #[test]
    fn every_service_tagged_class_is_an_instance_subclass() {
        let db = database();

        for name in db.classes.keys() {
            if db.is_service(name) {
                assert!(
                    db.is_subclass_of(name, "Instance"),
                    "{name} is tagged Service but is not an Instance subclass"
                );
            }
        }
    }
}
