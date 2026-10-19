use super::{Choices, Feature};
use std::path::PathBuf;

const ALPHA: Feature = Feature {
    id: "alpha",
    name: "Alpha",
    description: "First test feature.",
};

const BETA: Feature = Feature {
    id: "beta",
    name: "Beta",
    description: "Second test feature.",
};

const GAMMA: Feature = Feature {
    id: "gamma",
    name: "Gamma",
    description: "Third test feature, never listed.",
};

fn choices(ids: &[&str]) -> Choices {
    let mut out = Choices::default();
    for id in ids {
        out.set(id, true);
    }
    out
}

fn unique_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rbx-beta-test-{}-{}", std::process::id(), name))
}

#[test]
fn json_round_trips_enabled_ids() {
    let original = choices(&["alpha", "beta"]);
    let text = original.to_json();
    assert_eq!(Choices::from_json(&text), original);
}

#[test]
fn json_reads_the_documented_shape() {
    let parsed = Choices::from_json(r#"{"enabled":["alpha","gamma"]}"#);
    assert!(parsed.is_on("alpha"));
    assert!(parsed.is_on(GAMMA.id));
    assert!(!parsed.is_on("beta"));
}

#[test]
fn malformed_json_is_default() {
    assert_eq!(Choices::from_json("not json at all"), Choices::default());
    assert_eq!(Choices::from_json(""), Choices::default());
    assert_eq!(
        Choices::from_json(r#"{"enabled":"alpha"}"#),
        Choices::default()
    );
}

#[test]
fn missing_enabled_key_is_default() {
    assert_eq!(Choices::from_json("{}"), Choices::default());
    assert_eq!(
        Choices::from_json(r#"{"other":["alpha"]}"#),
        Choices::default()
    );
}

#[test]
fn set_on_and_off_control_is_on() {
    let mut c = Choices::default();
    assert!(!c.is_on("alpha"));

    c.set("alpha", true);
    assert!(c.is_on("alpha"));
    assert!(!c.is_on("beta"));

    c.set("alpha", false);
    assert!(!c.is_on("alpha"));
}

#[test]
fn differences_lists_only_listed_features_that_differ() {
    let stored = choices(&["alpha", "gamma"]);
    let running = choices(&["beta", "gamma"]);

    let diff = stored.differences(&running, &[ALPHA, BETA]);
    assert_eq!(diff, vec!["alpha", "beta"]);
}

#[test]
fn differences_ignores_unlisted_features() {
    let stored = choices(&["gamma"]);
    let running = Choices::default();

    assert!(stored.differences(&running, &[ALPHA, BETA]).is_empty());
}

#[test]
fn differences_is_empty_when_states_match() {
    let a = choices(&["alpha"]);
    let b = choices(&["alpha"]);

    assert!(a.differences(&b, &[ALPHA, BETA]).is_empty());
}

#[test]
fn load_of_missing_path_is_default() {
    let dir = unique_dir("missing");
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("beta.json");

    assert_eq!(Choices::load(&path), Choices::default());
}

#[test]
fn save_then_load_round_trips() {
    let dir = unique_dir("round-trip");
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("nested").join("beta.json");

    let original = choices(&["alpha", "beta"]);
    original
        .save(&path)
        .expect("save should create the directory and write");
    assert_eq!(Choices::load(&path), original);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reset_switches_listed_features_off_and_keeps_unlisted_ids() {
    let mut stored = choices(&["alpha", "beta", "gamma"]);
    stored.reset(&[ALPHA, BETA]);
    assert_eq!(stored, choices(&["gamma"]));
}
