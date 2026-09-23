use std::collections::HashSet;

use super::super::diff;
use super::{cap, expand_tabs, plain_rows, unified_rows, CodeRow, Source};

#[test]
fn tabs_land_on_four_column_stops() {
    assert_eq!(expand_tabs("\ta"), "    a");
    assert_eq!(expand_tabs("ab\tc"), "ab  c");
    assert_eq!(expand_tabs("abcd\te"), "abcd    e");
}

#[test]
fn a_function_line_is_found_above_a_hunk_but_a_word_starting_with_function_is_not() {
    let source = Source::new("local x = 1\nlocal function tick()\n  return x\nfunctional = 2\nend");
    assert_eq!(
        source.enclosing_function(3).as_deref(),
        Some("local function tick()")
    );
    assert_eq!(source.enclosing_function(0), None);
}

#[test]
fn the_cap_keeps_the_first_rows_and_says_how_many_are_left() {
    let source = Source::new("a\nb\nc\nd\ne");
    let rows = cap(plain_rows(&source), 3);
    assert_eq!(rows.len(), 4);
    assert!(matches!(rows[3], CodeRow::Limit { more: 2, limit: 3 }));
    assert_eq!(cap(plain_rows(&source), 5).len(), 5);
}

#[test]
fn the_fixture_update_has_two_hidden_lines_then_its_first_hunk() {
    let old = Source::new(include_str!(
        "../../../../../../assets/tests/argon_diff/RoundController.old.luau"
    ));
    let new = Source::new(include_str!(
        "../../../../../../assets/tests/argon_diff/RoundController.new.luau"
    ));
    let old_lines: Vec<&str> = old.lines.iter().map(String::as_str).collect();
    let new_lines: Vec<&str> = new.lines.iter().map(String::as_str).collect();
    let diff = diff::diff_lines(&old_lines, &new_lines);
    let rows = unified_rows(&old, &new, &diff, &HashSet::new());
    assert!(
        matches!(&rows[0], CodeRow::Hunk { hidden: 2, header, .. } if header == "@@ \u{2212}3,14 +3,12 @@")
    );
    let expanded: HashSet<usize> = [0].into_iter().collect();
    let opened = unified_rows(&old, &new, &diff, &expanded);
    assert!(matches!(
        opened[0],
        CodeRow::Line {
            old: Some(1),
            new: Some(1),
            ..
        }
    ));
}
