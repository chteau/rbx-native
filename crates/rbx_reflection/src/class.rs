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
