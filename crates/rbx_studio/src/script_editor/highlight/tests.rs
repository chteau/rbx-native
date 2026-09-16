use super::runs;
use crate::script_editor::luau::tokenize;

/// The runs `runs` produces for the whole of `source`, as
/// `(text, highlight name)` — the byte arithmetic is what is under test, but
/// a raw range tells a reader nothing.
fn covering(source: &str) -> Vec<(&str, Option<&'static str>)> {
    runs(&tokenize(source), &(0..source.len()))
        .into_iter()
        .map(|(span, name)| (&source[span], name))
        .collect()
}

/// Asserts the invariant the editor relies on: ordered, non-overlapping runs
/// that together cover `range` with no hole and nothing outside it.
fn assert_covers(source: &str, range: std::ops::Range<usize>) {
    let produced = runs(&tokenize(source), &range);
    let mut at = range.start;
    for (span, _) in &produced {
        assert_eq!(
            span.start, at,
            "a hole or an overlap before {span:?} in {produced:?}"
        );
        assert!(span.start < span.end, "{span:?} is empty");
        at = span.end;
    }
    assert_eq!(at, range.end, "{produced:?} stops short of {range:?}");
}

#[test]
fn a_whole_buffer_is_covered_run_by_run() {
    assert_eq!(
        covering("local x = 1"),
        [
            ("local", Some("keyword")),
            (" x ", None),
            ("=", Some("operator")),
            (" ", None),
            ("1", Some("number")),
        ]
    );
}

#[test]
fn an_uncoloured_identifier_merges_into_the_gap_around_it() {
    // `value` is an ordinary identifier with no highlight name, so it must
    // not be split out as a run of its own — it is simply not coloured.
    assert_eq!(
        covering("a value b"),
        [("a value b", None)],
        "nothing here has a colour, so the whole line is one plain run"
    );
}

#[test]
fn a_token_straddling_the_start_of_the_range_is_clipped_to_it() {
    let source = "local x = 1";
    // Starts in the middle of the `local` keyword.
    let produced = runs(&tokenize(source), &(2..7));
    assert_eq!(
        produced,
        [(2..5, Some("keyword")), (5..7, None)],
        "the keyword run must start at the range, not at the token"
    );
    assert_covers(source, 2..7);
}

#[test]
fn a_token_straddling_the_end_of_the_range_is_clipped_to_it() {
    let source = "x = \"hello world\"";
    let produced = runs(&tokenize(source), &(0..8));
    assert_eq!(produced.last(), Some(&(4..8, Some("string"))));
    assert_covers(source, 0..8);
}

#[test]
fn an_empty_range_produces_nothing() {
    assert!(runs(&tokenize("local x = 1"), &(3..3)).is_empty());
}

#[test]
fn a_range_with_no_token_in_it_is_one_plain_run() {
    let source = "local      x";
    assert_eq!(runs(&tokenize(source), &(6..10)), [(6..10, None)]);
}

#[test]
fn every_range_of_a_real_script_is_covered_exactly() {
    let source = "-- header\nlocal n = 0xFF\nprint(`n is {n}`)\n--[[ tail ]]";
    for start in 0..source.len() {
        for end in start..=source.len() {
            assert_covers(source, start..end);
        }
    }
}

#[test]
fn every_range_of_a_script_with_types_and_interpolation_is_covered_exactly() {
    // The same contract as above over the two constructs that no longer lex
    // to one flat token each: a type annotation, whose names are reclassified
    // after the fact, and an interpolated string, which is replaced by several
    // tokens covering between them exactly what the one it replaced did.
    let source = "type Row = { n: number }\nlocal r: Row = { n = 1 }\nprint(`n is {r.n}`)";
    for start in 0..source.len() {
        for end in start..=source.len() {
            assert_covers(source, start..end);
        }
    }
}

#[test]
fn a_type_annotation_and_an_interpolation_hole_are_coloured() {
    // The visible half of the same change: neither used to resolve to a
    // highlight name at all.
    let coloured = covering("local n: Vector3 = `at {n}`");
    assert!(
        coloured.contains(&("Vector3", Some("type"))),
        "the annotation should paint as a type: {coloured:?}"
    );
    assert!(
        coloured
            .iter()
            .any(|(text, name)| *text == "n" && name.is_none()),
        "the hole's contents should be code, not string: {coloured:?}"
    );
}

#[test]
fn an_empty_buffer_produces_nothing() {
    assert!(runs(&tokenize(""), &(0..0)).is_empty());
}
