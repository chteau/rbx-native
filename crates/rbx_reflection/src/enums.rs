//! Enum descriptors from the Roblox API dump.

/// Metadata for a Roblox enum, mapping value names to their numeric codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDescriptor {
    pub name: String,
    pub items: Vec<(String, u32)>,
}
