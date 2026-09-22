//! Roblox class and property metadata.
//!
//! Provides reflection information about Roblox classes, properties, and enums,
//! parsed from embedded JSON schema.

mod class;
mod database;
mod defaults;
#[cfg(feature = "embedded-dump")]
mod embedded;
mod enums;
mod error;
mod parse;
mod stored;

pub use class::{ClassDescriptor, PropertyDescriptor};
pub use database::ReflectionDatabase;
pub use enums::EnumDescriptor;
pub use error::ReflectionError;
