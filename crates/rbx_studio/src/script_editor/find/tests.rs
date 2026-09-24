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

#[test]
fn next_match_from_a_bare_cursor_selects_the_word_first() {
    let text = "local count = count + 1";
    let extend = next_match(text, std::slice::from_ref(&(8..8))).unwrap();
    assert_eq!(extend.primary, Some(6..11));
    assert!(extend.add.is_empty());
}

#[test]
fn next_match_adds_the_one_after_the_furthest_selection_and_wraps() {
    let text = "a x b x c x";
    assert_eq!(
        next_match(text, std::slice::from_ref(&(2..3))).unwrap().add,
        vec![(6..7)]
    );
    assert_eq!(next_match(text, &[6..7, 2..3]).unwrap().add, vec![(10..11)]);
    // From the last match, the next free one is back at the top.
    assert_eq!(
        next_match(text, std::slice::from_ref(&(10..11)))
            .unwrap()
            .add,
        vec![(2..3)]
    );
    // Everything selected: nothing left to add.
    assert!(next_match(text, &[2..3, 6..7, 10..11]).is_none());
}

#[test]
fn matches_are_exact_case_and_a_word_needle_is_whole_word() {
    let text = "count Count counter count";
    let every = every_match(text, std::slice::from_ref(&(0..0))).unwrap();
    assert_eq!(every.primary, Some(0..5));
    assert_eq!(every.add, vec![(20..25)]);
    // A selected needle matches inside words too.
    assert_eq!(
        every_match(text, std::slice::from_ref(&(0..5)))
            .unwrap()
            .add,
        vec![(12..17), (20..25)]
    );
}

#[test]
fn a_bare_cursor_on_no_word_does_nothing() {
    assert!(next_match("a  = b", std::slice::from_ref(&(2..2))).is_none());
    assert!(every_match("", std::slice::from_ref(&(0..0))).is_none());
}
