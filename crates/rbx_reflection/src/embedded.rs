//! Compile-time embedded copy of the Roblox API dump.

use crate::database::ReflectionDatabase;

// Keyed off the manifest dir, not the current working directory, so the lookup
// works identically under `cargo run`, `cargo test`, and once a binary using it
// is copied elsewhere.
const API_DUMP_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/API-Dump.json"
));
const DEFAULTS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/reflection-defaults.json"
));

impl ReflectionDatabase {
    /// Loads the API dump baked into this crate at compile time, with the
    /// class defaults beside it (see [`Self::with_defaults`]).
    ///
    /// Panics only if an embedded asset is malformed, which is a build-time
    /// invariant rather than a runtime condition a caller could recover from.
    pub fn embedded() -> Self {
        Self::from_json_str(API_DUMP_JSON)
            .and_then(|database| database.with_defaults(DEFAULTS_JSON))
            .expect("bundled API-Dump.json and reflection-defaults.json must parse")
    }
}
