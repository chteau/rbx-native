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

fn pattern(query: &str, match_case: bool, whole_word: bool, regex: bool) -> Pattern {
    Pattern::compile(
        query,
        Options {
            match_case,
            whole_word,
            regex,
        },
    )
    .unwrap()
}

#[test]
fn a_pattern_is_case_insensitive_unless_asked() {
    let text = "Foo foo FOO";
    assert_eq!(
        pattern("foo", false, false, false).matches(text),
        [0..3, 4..7, 8..11]
    );
    assert_eq!(
        pattern("foo", true, false, false).matches(text),
        vec![(4..7)]
    );
}

#[test]
fn a_plain_query_is_not_a_regex() {
    assert_eq!(
        pattern("a.b", false, false, false).matches("a.b axb"),
        vec![(0..3)]
    );
    assert_eq!(
        pattern("a.b", false, false, true).matches("a.b axb"),
        [0..3, 4..7]
    );
}

#[test]
fn whole_word_needs_no_identifier_character_either_side() {
    let text = "count counter _count count1 (count) count";
    let found = pattern("count", false, true, false).matches(text);
    assert_eq!(found, [0..5, 29..34, 36..41]);
    // The rule is about the characters beside the match, not the query's own.
    assert!(pattern(".", false, true, false).matches("a.b").is_empty());
    assert_eq!(
        pattern(".", false, true, false).matches("(.)"),
        vec![(1..2)]
    );
}

#[test]
fn regex_and_whole_word_combine_and_skip_empty_matches() {
    assert_eq!(
        pattern(r"\d+", false, true, true).matches("a1 22 b3b"),
        vec![(3..5)]
    );
    assert!(pattern("x*", false, false, true).matches("abc").is_empty());
    assert!(pattern("", false, false, false).matches("abc").is_empty());
}

#[test]
fn regex_anchors_work_per_line() {
    assert_eq!(
        pattern("^local", false, false, true).matches(
            "local a
local b"
        ),
        [0..5, 8..13]
    );
}

#[test]
fn an_invalid_regex_says_why_in_one_line() {
    let error = Pattern::compile(
        "(unclosed",
        Options {
            regex: true,
            ..Options::default()
        },
    )
    .err()
    .expect("does not compile");
    assert!(!error.contains('\n') && !error.is_empty(), "{error:?}");
    // The same text is fine as plain text.
    assert!(Pattern::compile("(unclosed", Options::default()).is_ok());
}

#[test]
fn a_pattern_handles_multi_byte_text() {
    let text = "héllo Héllo 日本語";
    assert_eq!(
        pattern("héllo", false, false, false).matches(text),
        [0..6, 7..13]
    );
    assert_eq!(
        pattern("héllo", true, false, false).matches(text),
        vec![(0..6)]
    );
    assert_eq!(
        pattern("日本", false, false, false).matches(text),
        vec![(14..20)]
    );
    assert_eq!(pattern("é", false, true, false).matches("é"), vec![(0..2)]);
}

#[test]
fn replacement_fills_groups_only_for_a_regex() {
    let text = "a = 1\nb = 2\n";
    let regex = pattern(r"(\w) = (\d)", false, false, true);
    assert_eq!(
        regex.replace_all(text, "$2 = $1").as_deref(),
        Some("1 = a\n2 = b\n")
    );
    let plain = pattern("a = 1", false, false, false);
    assert_eq!(plain.replace_all("a = 1", "$1").as_deref(), Some("$1"));
    assert_eq!(plain.replace_all("zzz", "y"), None);
}

#[test]
fn replacing_one_match_leaves_the_others() {
    let text = "foo foo foo";
    let plain = pattern("foo", false, false, false);
    assert_eq!(plain.replace_one(text, &(4..7), "bar"), "foo bar foo");
    let regex = pattern(r"f(o+)", false, false, true);
    assert_eq!(regex.replace_one("fo foo", &(3..6), "<$1>"), "fo <oo>");
    assert_eq!(plain.expansion(text, &(0..3), "x"), "x");
}
