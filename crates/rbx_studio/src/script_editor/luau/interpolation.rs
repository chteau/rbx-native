//! Backtick string interpolation: `` `Hello {player.Name}` ``.
//!
//! A hole holds an ordinary Luau expression, so it is lexed by the same
//! [`super::tokenize`] the file itself is, recursively — which is also what
//! makes a hole containing another interpolated string colour correctly, and
//! what stops a backtick *inside* a hole from being mistaken for the outer
//! literal's closing delimiter. [`MAX_DEPTH`] bounds the recursion so a
//! pathologically nested literal cannot drive the lexer off the stack.

#[cfg(test)]
mod tests;

use std::ops::Range;

use super::{comment_end, long_bracket, long_string_end, quoted_string_end, Token, TokenKind};

/// How many interpolated strings may nest before a hole stops being descended
/// into and stays plain string text.
///
/// Three is well past what anyone writes deliberately: `` `a {`b {c}`}` `` is
/// already two, and a third level is harder to read than whatever it replaced.
/// The cap exists for pasted or generated source, where the nesting is bounded
/// only by the file's length and each level costs a stack frame.
const MAX_DEPTH: usize = 3;

/// One piece of a backtick string.
///
/// A `Literal` carries the delimiters and the braces around each hole as well
/// as the text between them — on screen those read as part of the string, the
/// way a quote does, and it means the pieces tile the whole literal with no
/// gap for the highlighter to leave uncoloured.
enum Part {
    Literal(Range<usize>),
    Hole(Range<usize>),
}

/// The byte offset just past the backtick string opening at `at`, which is
/// where the lexer's main loop resumes.
pub(super) fn end(source: &str, at: usize, depth: usize) -> usize {
    scan(source, at, depth).0
}

/// Replaces every interpolated string in `tokens` with its literal runs and
/// the lexed contents of its holes.
///
/// Deliberately run *after* `mark_calls`/`mark_definitions` rather than during
/// lexing: while each literal is still one opaque `String` token, `f\`x\`` is
/// the parenthesis-free call sugar it looks like, and an identifier at the end
/// of a hole is not mistaken for a call on the string run that follows it.
pub(super) fn expand(source: &str, tokens: &mut Vec<Token>, depth: usize) {
    if depth >= MAX_DEPTH || !tokens.iter().any(|token| is_interpolated(source, token)) {
        return;
    }
    let mut out = Vec::with_capacity(tokens.len());
    for token in std::mem::take(tokens) {
        if !is_interpolated(source, &token) {
            out.push(token);
            continue;
        }
        for part in scan(source, token.range.start, depth).1 {
            match part {
                Part::Literal(range) => out.push(Token {
                    range,
                    kind: TokenKind::String,
                }),
                Part::Hole(range) => {
                    let base = range.start;
                    let inner = super::tokenize_at(&source[range], depth + 1);
                    out.extend(inner.into_iter().map(|token| Token {
                        range: base + token.range.start..base + token.range.end,
                        kind: token.kind,
                    }));
                }
            }
        }
    }
    *tokens = out;
}

fn is_interpolated(source: &str, token: &Token) -> bool {
    token.kind == TokenKind::String && source.as_bytes().get(token.range.start) == Some(&b'`')
}

/// Walks the backtick string opening at `at`, returning where it ends and the
/// pieces it is made of.
fn scan(source: &str, at: usize, depth: usize) -> (usize, Vec<Part>) {
    let bytes = source.as_bytes();
    let mut parts = Vec::new();
    let mut literal = at;
    let mut cursor = at + 1;

    while cursor < bytes.len() {
        match bytes[cursor] {
            // Skipping the escaped byte is also what makes `\{` a literal
            // brace and ``\` `` a literal backtick, neither of which opens or
            // closes anything.
            b'\\' => cursor += 2,
            // An unterminated literal stops at its newline rather than
            // colouring the rest of the file, exactly as `quoted_string_end`
            // does for the other two quote styles.
            b'\n' => {
                push_literal(&mut parts, literal..cursor);
                return (cursor, parts);
            }
            b'`' => {
                push_literal(&mut parts, literal..cursor + 1);
                return (cursor + 1, parts);
            }
            b'{' => {
                let body = cursor + 1;
                let Some(close) = hole_end(source, body, depth) else {
                    // An unclosed hole: the rest of the line is string text,
                    // which is what it looks like while it is being typed.
                    let stop = line_end(bytes, cursor);
                    push_literal(&mut parts, literal..stop);
                    return (stop, parts);
                };
                push_literal(&mut parts, literal..body);
                parts.push(Part::Hole(body..close));
                literal = close;
                cursor = close + 1;
            }
            _ => cursor += 1,
        }
    }

    push_literal(&mut parts, literal..bytes.len());
    (bytes.len(), parts)
}

/// The `}` closing the hole whose body starts at `from`, if it is on this
/// line.
///
/// Every construct that can hold a brace or a backtick without meaning one is
/// skipped whole, so a table constructor's braces nest correctly and a string
/// inside the hole cannot end either the hole or the literal around it.
fn hole_end(source: &str, from: usize, depth: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut nesting = 1usize;
    let mut at = from;

    while at < bytes.len() {
        match bytes[at] {
            b'{' => {
                nesting += 1;
                at += 1;
            }
            b'}' => {
                nesting -= 1;
                if nesting == 0 {
                    return Some(at);
                }
                at += 1;
            }
            b'"' | b'\'' => at = quoted_string_end(bytes, at),
            // A nested interpolated string has holes of its own to step over;
            // past the cap it is measured as a plain quoted run instead.
            b'`' if depth + 1 < MAX_DEPTH => at = scan(source, at, depth + 1).0,
            b'`' => at = quoted_string_end(bytes, at),
            b'-' if bytes.get(at + 1) == Some(&b'-') => at = comment_end(bytes, at),
            b'[' if long_bracket(bytes, at).is_some() => at = long_string_end(bytes, at),
            // The literal this hole belongs to cannot span a line, so an
            // unclosed hole ends here rather than swallowing the file.
            b'\n' => return None,
            _ => at += 1,
        }
    }
    None
}

fn line_end(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && bytes[at] != b'\n' {
        at += 1;
    }
    at
}

fn push_literal(parts: &mut Vec<Part>, range: Range<usize>) {
    if !range.is_empty() {
        parts.push(Part::Literal(range));
    }
}
