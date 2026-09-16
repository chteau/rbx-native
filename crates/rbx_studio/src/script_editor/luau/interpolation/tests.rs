use crate::script_editor::luau::{tokenize, TokenKind};

fn lexed(source: &str) -> Vec<(&str, TokenKind)> {
    tokenize(source)
        .into_iter()
        .map(|token| (&source[token.range], token.kind))
        .collect()
}

fn of_kind(source: &str, kind: TokenKind) -> Vec<&str> {
    lexed(source)
        .into_iter()
        .filter(|(_, found)| *found == kind)
        .map(|(text, _)| text)
        .collect()
}

#[test]
fn a_plain_identifier_hole_is_lexed_as_code() {
    assert_eq!(
        lexed("`hello {name}`"),
        [
            ("`hello {", TokenKind::String),
            ("name", TokenKind::Identifier),
            ("}`", TokenKind::String),
        ],
        "the delimiters and the braces read as string; what is between them does not"
    );
}

#[test]
fn a_method_call_in_a_hole_is_lexed_as_one() {
    let source = "`hi {player:GetName()}`";
    assert_eq!(of_kind(source, TokenKind::Function), ["GetName"]);
    assert_eq!(of_kind(source, TokenKind::Identifier), ["player"]);
    assert_eq!(of_kind(source, TokenKind::String), ["`hi {", "}`"]);
}

#[test]
fn an_arithmetic_expression_in_a_hole_keeps_its_numbers_and_operators() {
    let source = "`total: {a + 1}`";
    assert_eq!(of_kind(source, TokenKind::Number), ["1"]);
    assert_eq!(of_kind(source, TokenKind::Operator), ["+"]);
    assert_eq!(
        of_kind(source, TokenKind::String),
        ["`total: {", "}`"],
        "the `:` before the hole is string text, not an annotation"
    );
}

#[test]
fn a_backtick_inside_a_hole_does_not_end_the_string() {
    // A nested string's backtick used to be read as the outer literal's
    // close, so the rest of the line painted as code.
    assert_eq!(
        lexed("`foo{\"`\"}`"),
        [
            ("`foo{", TokenKind::String),
            ("\"`\"", TokenKind::String),
            ("}`", TokenKind::String),
        ]
    );
}

#[test]
fn a_hole_may_hold_another_interpolated_string() {
    assert_eq!(
        lexed("`a {`b {c}`} d`"),
        [
            ("`a {", TokenKind::String),
            ("`b {", TokenKind::String),
            ("c", TokenKind::Identifier),
            ("}`", TokenKind::String),
            ("} d`", TokenKind::String),
        ]
    );
}

#[test]
fn a_table_constructors_braces_inside_a_hole_are_not_the_holes_close() {
    let source = "`n is {#{1, 2}}`";
    assert_eq!(of_kind(source, TokenKind::Number), ["1", "2"]);
    assert_eq!(of_kind(source, TokenKind::String), ["`n is {", "}`"]);
}

#[test]
fn a_comment_inside_a_hole_is_a_comment() {
    let source = "`{x --[[ why ]] + 1}`";
    assert_eq!(of_kind(source, TokenKind::Comment), ["--[[ why ]]"]);
}

#[test]
fn an_escaped_brace_or_backtick_opens_and_closes_nothing() {
    let source = r"`a \{not a hole\} b`";
    assert_eq!(of_kind(source, TokenKind::String), [source]);
}

#[test]
fn an_unterminated_interpolated_string_stops_at_its_newline() {
    let source = "local s = `oops\nlocal x = 1";
    assert_eq!(of_kind(source, TokenKind::String), ["`oops"]);
    assert_eq!(
        of_kind(source, TokenKind::Keyword),
        ["local", "local"],
        "typing an opening backtick must not repaint the rest of the file"
    );
}

#[test]
fn an_unclosed_hole_leaves_the_rest_of_its_line_as_string_text() {
    let source = "local s = `oops {x\nlocal y = 1";
    assert_eq!(of_kind(source, TokenKind::String), ["`oops {x"]);
    assert_eq!(of_kind(source, TokenKind::Keyword), ["local", "local"]);
}

#[test]
fn the_call_sugar_still_reads_an_interpolated_string_as_its_argument() {
    // Why holes are expanded only after call position is settled: `f` is
    // being called, and `name` — an identifier ending a hole, with a string
    // run right after it — is not.
    let source = "f`hi {name}`";
    assert_eq!(of_kind(source, TokenKind::Function), ["f"]);
    assert_eq!(of_kind(source, TokenKind::Identifier), ["name"]);
}

#[test]
fn nesting_past_the_cap_leaves_the_innermost_literal_as_plain_string() {
    // Three levels are descended into and the fourth is not. Nothing anyone
    // writes goes this deep; the cap is there so that a pasted or generated
    // file cannot drive the recursion off the stack.
    assert_eq!(
        lexed("`1{`2{`3{`4{x}`}`}`}`"),
        [
            ("`1{", TokenKind::String),
            ("`2{", TokenKind::String),
            ("`3{", TokenKind::String),
            ("`4{x}`", TokenKind::String),
            ("}`", TokenKind::String),
            ("}`", TokenKind::String),
            ("}`", TokenKind::String),
        ]
    );
}

#[test]
fn an_interpolated_strings_pieces_tile_it_with_no_gap() {
    // The highlighter contracts on tokens that do not overlap; a literal
    // whose pieces also leave no gap is what keeps a hole's braces coloured.
    let source = "print(`a {b} c {d} e`)";
    let mut at = source.find('`').unwrap();
    for token in tokenize(source) {
        if token.range.start < at {
            continue;
        }
        assert_eq!(token.range.start, at, "gap before {token:?}");
        at = token.range.end;
        if at > source.rfind('`').unwrap() {
            break;
        }
    }
    assert_eq!(at, source.rfind('`').unwrap() + 1);
}
