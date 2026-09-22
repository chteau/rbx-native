//! Compile-time embedded copy of the Roblox API dump.

use std::sync::OnceLock;

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
    /// A copy of [`Self::shared`]: copying the parsed tables costs a
    /// fraction of parsing the 4 MB dump again.
    pub fn embedded() -> Self {
        Self::shared().clone()
    }

    /// The API dump baked into this crate at compile time, with the class
    /// defaults beside it (see [`Self::with_defaults`]), parsed once per
    /// process — for a caller with no database of its own to hand, such as
    /// a place file being read.
    ///
    /// Panics only if an embedded asset is malformed, which is a build-time
    /// invariant rather than a runtime condition a caller could recover from.
    pub fn shared() -> &'static Self {
        static DATABASE: OnceLock<ReflectionDatabase> = OnceLock::new();
        DATABASE.get_or_init(|| {
            Self::from_json_str(API_DUMP_JSON)
                .and_then(|database| database.with_defaults(DEFAULTS_JSON))
                .expect("bundled API-Dump.json and reflection-defaults.json must parse")
        })
    }
}
