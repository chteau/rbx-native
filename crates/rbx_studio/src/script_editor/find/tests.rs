use super::*;

#[test]
fn matches_ignore_ascii_case_and_never_overlap() {
    assert_eq!(matches("Foo foo FOO", "foo"), [0..3, 4..7, 8..11]);
    assert_eq!(matches("aaaa", "aa"), [0..2, 2..4]);
    assert!(matches("anything", "").is_empty());
    assert!(matches("ab", "abc").is_empty());
}

#[test]
fn multi_byte_text_is_matched_on_char_boundaries() {
    assert_eq!(matches("é-é", "é"), [0..2, 3..5]);
    assert_eq!(matches("print(\"héllo\")", "llo"), vec![(10..13)]);
}

#[test]
fn replace_all_rewrites_every_match_or_reports_none() {
    assert_eq!(
        replace_all("local Foo = foo", "foo", "bar").as_deref(),
        Some("local bar = bar")
    );
    assert_eq!(replace_all("nothing here", "zzz", "y"), None);
}

#[test]
fn line_at_numbers_from_one_and_drops_indentation() {
    let text = "a\n\tlocal x = 1\nb";
    assert_eq!(line_at(text, 0), (1, "a"));
    assert_eq!(line_at(text, text.find('x').unwrap()), (2, "local x = 1"));
    assert_eq!(line_at(text, text.len()), (3, "b"));
}
