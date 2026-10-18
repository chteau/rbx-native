use rbx_dom::merge::{merge_text, TextChunk};

fn clean(s: &str) -> TextChunk {
    TextChunk::Clean(s.to_owned())
}

fn conflict(base: &str, ours: &str, theirs: &str) -> TextChunk {
    TextChunk::Conflict {
        base: base.to_owned(),
        ours: ours.to_owned(),
        theirs: theirs.to_owned(),
    }
}

const BASE: &str = "a\nb\nc\n";

#[test]
fn all_three_equal_is_one_clean_chunk() {
    assert_eq!(merge_text(BASE, BASE, BASE), vec![clean(BASE)]);
}

#[test]
fn only_ours_changed_a_line_takes_ours() {
    assert_eq!(
        merge_text(BASE, "a\nB\nc\n", BASE),
        vec![clean("a\nB\nc\n")]
    );
}

#[test]
fn only_theirs_changed_a_line_takes_theirs() {
    assert_eq!(
        merge_text(BASE, BASE, "a\nb\nC\n"),
        vec![clean("a\nb\nC\n")]
    );
}

#[test]
fn different_non_adjacent_lines_both_apply() {
    let base = "1\n2\n3\n4\n5\n";
    let ours = "1\nTWO\n3\n4\n5\n";
    let theirs = "1\n2\n3\nFOUR\n5\n";
    assert_eq!(
        merge_text(base, ours, theirs),
        vec![clean("1\nTWO\n3\nFOUR\n5\n")]
    );
}

#[test]
fn same_change_on_both_sides_is_clean() {
    assert_eq!(
        merge_text(BASE, "a\nX\nc\n", "a\nX\nc\n"),
        vec![clean("a\nX\nc\n")]
    );
}

#[test]
fn same_line_changed_differently_is_conflict() {
    assert_eq!(
        merge_text(BASE, "a\nO\nc\n", "a\nT\nc\n"),
        vec![clean("a\n"), conflict("b\n", "O\n", "T\n"), clean("c\n"),]
    );
}

#[test]
fn different_insertions_at_same_spot_conflict_with_empty_base() {
    assert_eq!(
        merge_text("a\nc\n", "a\nX\nc\n", "a\nY\nc\n"),
        vec![clean("a\n"), conflict("", "X\n", "Y\n"), clean("c\n"),]
    );
}

#[test]
fn deletion_on_one_side_is_applied() {
    assert_eq!(merge_text(BASE, "a\nc\n", BASE), vec![clean("a\nc\n")]);
}

#[test]
fn deletion_on_one_side_against_modification_is_conflict() {
    assert_eq!(
        merge_text(BASE, "a\nc\n", "a\nB\nc\n"),
        vec![clean("a\n"), conflict("b\n", "", "B\n"), clean("c\n"),]
    );
}

#[test]
fn crlf_line_endings_are_preserved() {
    let base = "a\r\nb\r\nc\r\n";
    assert_eq!(
        merge_text(base, "a\r\nB\r\nc\r\n", base),
        vec![clean("a\r\nB\r\nc\r\n")]
    );
}

#[test]
fn crlf_conflict_keeps_terminators_in_each_side() {
    assert_eq!(
        merge_text("a\r\nb\r\n", "a\r\nO\r\n", "a\r\nT\r\n"),
        vec![clean("a\r\n"), conflict("b\r\n", "O\r\n", "T\r\n")]
    );
}

#[test]
fn last_line_without_newline_takes_the_edit() {
    let base = "a\nb";
    assert_eq!(merge_text(base, "a\nB", base), vec![clean("a\nB")]);
}

#[test]
fn last_line_without_newline_conflicts_as_written() {
    assert_eq!(
        merge_text("a\nb", "a\nO", "a\nT"),
        vec![clean("a\n"), conflict("b", "O", "T")]
    );
}

#[test]
fn empty_base_with_different_additions_is_one_conflict() {
    assert_eq!(
        merge_text("", "x\n", "y\n"),
        vec![conflict("", "x\n", "y\n")]
    );
}

#[test]
fn ours_emptied_while_theirs_unchanged_takes_empty() {
    assert_eq!(merge_text(BASE, "", BASE), Vec::<TextChunk>::new());
}
