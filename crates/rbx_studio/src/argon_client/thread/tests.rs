use super::*;

#[test]
fn matching_major_minor_is_compatible_even_with_a_different_patch() {
    assert!(version_compatible("2.0.29"));
}

#[test]
fn a_different_minor_is_not_compatible() {
    assert!(!version_compatible("2.1.0"));
}

#[test]
fn a_different_major_is_not_compatible() {
    assert!(!version_compatible("3.0.0"));
}

#[test]
fn a_malformed_version_string_is_not_compatible() {
    assert!(!version_compatible("not-a-version"));
}
