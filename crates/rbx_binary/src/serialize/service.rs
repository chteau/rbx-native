//! Looks up whether a class is a Roblox "service" singleton (Workspace, Lighting, ...).
//!
//! `chunks::inst` treats INST's per-instance rooted-flag array as padding to skip, since
//! nothing in the DOM consumes it. The IsService bit that gates that array is different:
//! real Studio uses it to merge an instance into the DataModel's existing singleton
//! instead of creating a duplicate, so writing it wrong risks a file Studio mishandles.

use std::collections::HashSet;
use std::sync::OnceLock;

use serde_json::Value;

const API_DUMP_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/API-Dump.json"
));

fn services() -> &'static HashSet<String> {
    static SERVICES: OnceLock<HashSet<String>> = OnceLock::new();
    SERVICES.get_or_init(|| {
        let dump: Value =
            serde_json::from_str(API_DUMP_JSON).expect("bundled API-Dump.json must parse");

        dump["Classes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|class| {
                class["Tags"]
                    .as_array()
                    .is_some_and(|tags| tags.iter().any(|tag| tag == "Service"))
            })
            .filter_map(|class| class["Name"].as_str().map(str::to_owned))
            .collect()
    })
}

/// Whether `class_name` is tagged `Service` in the embedded API dump.
pub(crate) fn is_service(class_name: &str) -> bool {
    services().contains(class_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_and_lighting_are_services() {
        assert!(is_service("Workspace"));
        assert!(is_service("Lighting"));
    }

    #[test]
    fn part_is_not_a_service() {
        assert!(!is_service("Part"));
        assert!(!is_service("NotARealClass"));
    }
}
