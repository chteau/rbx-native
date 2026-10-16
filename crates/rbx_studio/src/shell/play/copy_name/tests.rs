use super::{render, DEFAULT, MAX_CHARS};

#[test]
fn default_template_marks_the_place_as_a_test() {
    assert_eq!(render(DEFAULT, "Obby").as_deref(), Some("Obby (Play)"));
}

#[test]
fn blank_or_whitespace_template_yields_none() {
    assert_eq!(render("", "Obby"), None);
    assert_eq!(render("   \t\n", "Obby"), None);
}

#[test]
fn template_without_placeholder_is_used_as_is() {
    assert_eq!(render("Test run", "Obby").as_deref(), Some("Test run"));
}

#[test]
fn every_placeholder_is_replaced() {
    assert_eq!(
        render("{place} - {place}", "Obby").as_deref(),
        Some("Obby - Obby")
    );
}

#[test]
fn result_is_trimmed() {
    assert_eq!(render("  {place}  ", "  Obby  ").as_deref(), Some("Obby"));
}

#[test]
fn long_result_is_cut_to_max_chars() {
    let place = "a".repeat(60);
    let out = render("{place}", &place).unwrap();
    assert_eq!(out.chars().count(), MAX_CHARS);
    assert_eq!(out, "a".repeat(MAX_CHARS));
}

#[test]
fn multibyte_characters_are_never_split() {
    let place = "é😀".repeat(40);
    let out = render("{place}", &place).unwrap();
    assert_eq!(out.chars().count(), MAX_CHARS);
    assert!(out.chars().all(|c| c == 'é' || c == '😀'));
}
