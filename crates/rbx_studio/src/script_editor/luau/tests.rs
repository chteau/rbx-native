use super::{tokenize, TokenKind};

/// Every token as `(text, kind)`, which is what the assertions below are
/// actually about — a raw byte range says nothing to a reader.
fn lexed(source: &str) -> Vec<(&str, TokenKind)> {
    tokenize(source)
        .into_iter()
        .map(|token| (&source[token.range], token.kind))
        .collect()
}

/// Every token of one kind, in source order.
fn of_kind(source: &str, kind: TokenKind) -> Vec<&str> {
    lexed(source)
        .into_iter()
        .filter(|(_, found)| *found == kind)
        .map(|(text, _)| text)
        .collect()
}

#[test]
fn an_empty_source_lexes_to_nothing() {
    assert!(tokenize("").is_empty());
    assert!(tokenize("   \n\t\n ").is_empty());
}

#[test]
fn keywords_are_told_apart_from_identifiers_that_merely_contain_them() {
    let source = "local ending = iffy";
    assert_eq!(of_kind(source, TokenKind::Keyword), ["local"]);
    assert_eq!(
        of_kind(source, TokenKind::Identifier),
        ["ending", "iffy"],
        "`ending` and `iffy` only start with a keyword, they are not one"
    );
}

#[test]
fn luau_only_keywords_are_keywords() {
    // None of these three are reserved words in Lua 5.1, which is the whole
    // reason this lexer exists instead of a Lua grammar.
    for word in ["continue", "export", "type"] {
        assert_eq!(
            of_kind(word, TokenKind::Keyword),
            [word],
            "{word} should be highlighted as a keyword"
        );
    }
}

#[test]
fn nil_and_the_booleans_are_their_own_kinds() {
    let source = "if a == nil or b == true or c == false then end";
    assert_eq!(of_kind(source, TokenKind::Nil), ["nil"]);
    assert_eq!(of_kind(source, TokenKind::Boolean), ["true", "false"]);
}

#[test]
fn a_line_comment_runs_to_the_end_of_its_line_and_no_further() {
    let source = "-- a note\nlocal x = 1";
    assert_eq!(of_kind(source, TokenKind::Comment), ["-- a note"]);
    assert_eq!(
        of_kind(source, TokenKind::Keyword),
        ["local"],
        "the line after the comment must still be lexed"
    );
}

#[test]
fn a_long_comment_spans_lines_and_stops_at_its_own_close() {
    let source = "--[[ line one\nline two ]] local x";
    assert_eq!(
        of_kind(source, TokenKind::Comment),
        ["--[[ line one\nline two ]]"]
    );
    assert_eq!(of_kind(source, TokenKind::Keyword), ["local"]);
}

#[test]
fn a_long_comment_at_a_level_is_not_closed_by_a_shallower_bracket() {
    let source = "--[==[ contains ]] still inside ]==] local x";
    assert_eq!(
        of_kind(source, TokenKind::Comment),
        ["--[==[ contains ]] still inside ]==]"]
    );
    assert_eq!(of_kind(source, TokenKind::Keyword), ["local"]);
}

#[test]
fn both_quote_styles_and_their_escapes_lex_as_one_string() {
    let source = r#"local a = "he said \"hi\"" local b = 'it\'s' "#;
    assert_eq!(
        of_kind(source, TokenKind::String),
        [r#""he said \"hi\"""#, r"'it\'s'"]
    );
}

#[test]
fn an_unterminated_string_stops_at_the_newline() {
    // Typing an opening quote must not repaint the rest of the file as a
    // string while the closing one has yet to be typed.
    let source = "local a = \"oops\nlocal b = 2";
    assert_eq!(of_kind(source, TokenKind::String), ["\"oops"]);
    assert_eq!(
        of_kind(source, TokenKind::Keyword),
        ["local", "local"],
        "the following line must still lex normally"
    );
}

#[test]
fn a_long_string_spans_lines() {
    let source = "local a = [[one\ntwo]]";
    assert_eq!(of_kind(source, TokenKind::String), ["[[one\ntwo]]"]);
}

#[test]
fn an_interpolated_string_lexes_as_one_string() {
    let source = "local greeting = `hello {name}`";
    assert_eq!(of_kind(source, TokenKind::String), ["`hello {name}`"]);
}

#[test]
fn a_comment_marker_inside_a_string_does_not_start_a_comment() {
    let source = r#"local a = "-- not a comment""#;
    assert_eq!(
        of_kind(source, TokenKind::String),
        [r#""-- not a comment""#]
    );
    assert!(of_kind(source, TokenKind::Comment).is_empty());
}

#[test]
fn a_quote_inside_a_comment_does_not_start_a_string() {
    let source = "-- it's fine\nlocal x = 1";
    assert_eq!(of_kind(source, TokenKind::Comment), ["-- it's fine"]);
    assert!(of_kind(source, TokenKind::String).is_empty());
}

#[test]
fn decimal_hex_binary_and_exponent_numbers_all_lex_whole() {
    let source = "local n = {1, 2.5, 0xFF, 0b1011, 1e10, 1.5e-3, 1_000_000}";
    assert_eq!(
        of_kind(source, TokenKind::Number),
        ["1", "2.5", "0xFF", "0b1011", "1e10", "1.5e-3", "1_000_000"]
    );
}

#[test]
fn a_concatenation_of_two_numbers_is_not_one_number() {
    // `1..2` must lex as number, `..`, number — consuming the second dot into
    // the first number would swallow the operator.
    assert_eq!(
        lexed("1..2"),
        [
            ("1", TokenKind::Number),
            ("..", TokenKind::Operator),
            ("2", TokenKind::Number),
        ]
    );
}

#[test]
fn a_called_name_is_a_function_and_a_plain_one_is_not() {
    let source = "print(value)";
    assert_eq!(of_kind(source, TokenKind::Function), ["print"]);
    assert_eq!(of_kind(source, TokenKind::Identifier), ["value"]);
}

#[test]
fn a_defined_function_name_is_a_function() {
    let source = "local function greet(who) end\nfunction Class.method(self) end";
    assert_eq!(of_kind(source, TokenKind::Function), ["greet", "method"]);
}

#[test]
fn luas_parenthesis_free_call_sugar_still_reads_as_a_call() {
    assert_eq!(
        of_kind(r#"require "module""#, TokenKind::Function),
        ["require"]
    );
    assert_eq!(of_kind("setup {a = 1}", TokenKind::Function), ["setup"]);
}

#[test]
fn a_method_call_on_a_roblox_global_marks_only_the_method() {
    let source = "game:GetService(\"Players\")";
    assert_eq!(of_kind(source, TokenKind::SpecialVariable), ["game"]);
    assert_eq!(of_kind(source, TokenKind::Function), ["GetService"]);
}

#[test]
fn compound_assignment_and_type_punctuation_lex_as_operators() {
    let source = "n += 1 local v: number = x :: any";
    let operators = of_kind(source, TokenKind::Operator);
    for expected in ["+=", "=", "::"] {
        assert!(
            operators.contains(&expected),
            "{expected} missing from {operators:?}"
        );
    }
}

#[test]
fn tokens_are_ordered_non_overlapping_and_on_char_boundaries() {
    // The editor slices the source by these ranges to paint it; an
    // out-of-order, overlapping or mid-`char` range would panic or misprint.
    let source = "local emoji = \"héllo 🌍\" -- accentué\nprint(emoji) --[[ fin ]]";
    let tokens = tokenize(source);
    assert!(!tokens.is_empty());

    let mut previous_end = 0;
    for token in &tokens {
        assert!(
            token.range.start >= previous_end,
            "{token:?} overlaps or precedes the token before it"
        );
        assert!(token.range.start < token.range.end, "{token:?} is empty");
        assert!(
            token.range.end <= source.len(),
            "{token:?} runs past the source"
        );
        assert!(
            source.is_char_boundary(token.range.start) && source.is_char_boundary(token.range.end),
            "{token:?} splits a char"
        );
        previous_end = token.range.end;
    }
}

#[test]
fn a_representative_script_highlights_every_kind_the_roadmap_asks_for() {
    let source = r#"-- Doubles every part's size.
local Workspace = game:GetService("Workspace")
local FACTOR = 2.5

local function grow(part: BasePart)
	part.Size = part.Size * FACTOR
end

for _, part in Workspace:GetChildren() do
	if part:IsA("BasePart") and part.Anchored ~= true then
		grow(part)
	end
end
"#;

    assert_eq!(
        of_kind(source, TokenKind::Comment),
        ["-- Doubles every part's size."]
    );
    assert_eq!(
        of_kind(source, TokenKind::String),
        [r#""Workspace""#, r#""BasePart""#]
    );
    assert_eq!(of_kind(source, TokenKind::Number), ["2.5"]);
    assert_eq!(
        of_kind(source, TokenKind::Function),
        ["GetService", "grow", "GetChildren", "IsA", "grow"]
    );
    assert_eq!(of_kind(source, TokenKind::Boolean), ["true"]);
    assert_eq!(of_kind(source, TokenKind::SpecialVariable), ["game"]);

    let keywords = of_kind(source, TokenKind::Keyword);
    for expected in ["local", "function", "for", "in", "if", "and", "then", "end"] {
        assert!(
            keywords.contains(&expected),
            "{expected} missing from {keywords:?}"
        );
    }
}
