//! Class and property descriptors from the Roblox API dump.

/// Metadata for a single property on a class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyDescriptor {
    pub name: String,
    pub value_type: String,
    /// The dump's `Category` field (e.g. `"Data"`, `"Appearance"`,
    /// `"Part"`), used to group properties the way Studio's own Properties
    /// panel does. Always present in the dump — no `Option` needed.
    pub category: String,
    /// The dump's per-property `Tags` (e.g. `Hidden`, `Deprecated`,
    /// `ReadOnly`, `NotReplicated`). Empty when the property carries none.
    pub tags: Vec<String>,
    /// The dump's `Serialization.CanLoad`/`CanSave` — whether Studio can
    /// read/write this property to a file. Always present for a Property
    /// member in the dump — no `Option` needed.
    pub can_load: bool,
    pub can_save: bool,
}

impl PropertyDescriptor {
    /// Whether Studio's own Properties panel never lists this property at
    /// all, e.g. `BasePart.Position`/`Orientation`: exposed only through the
    /// dedicated Position/Orientation UI, never as a raw property row.
    pub fn is_hidden(&self) -> bool {
        self.tags.iter().any(|tag| tag == "Hidden")
    }

    /// Whether the property should render with no edit affordance. The dump
    /// sets `ReadOnly` and `Serialization.CanSave: false` independently —
    /// many `ReadOnly` properties still report `CanSave: true` — so both
    /// signals feed the one read-only treatment rather than two separate
    /// checks in callers.
    pub fn is_read_only(&self) -> bool {
        !self.can_save || self.tags.iter().any(|tag| tag == "ReadOnly")
    }
}

/// Metadata for a Roblox class, including its superclass and properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassDescriptor {
    pub name: String,
    pub superclass: Option<String>,
    pub properties: Vec<PropertyDescriptor>,
    /// Class tags from the API dump (e.g. `Service`, `NotCreatable`,
    /// `NotBrowsable`). Empty when the class carries none.
    pub tags: Vec<String>,
}
