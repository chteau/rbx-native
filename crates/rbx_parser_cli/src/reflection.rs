//! Embedded reflection database for the CLI.

use rbx_reflection::ReflectionDatabase;

/// Returns a reflection database loaded from the API dump bundled in `rbx_reflection`.
pub fn default_reflection_database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}
