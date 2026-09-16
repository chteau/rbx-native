//! A Luau lexer: the token kinds the script editor picks its colours from.
//!
//! Hand-written rather than grammar-driven. `gpui-kit` does ship an optional
//! `tree-sitter-lua` feature, but Luau is not Lua 5.1: type annotations
//! (`local n: number`), `continue`, compound assignment (`n += 1`), `0b`
//! literals and backtick string interpolation are all syntax errors to that
//! grammar, and a parse error swallows the highlighting of everything after
//! it. Nothing here needs a parse tree either, so a grammar's C sources and
//! incremental-reparse machinery would be weight spent for nothing.
//!
//! Most of what a highlighter distinguishes is lexical, and the loop below
//! settles it one token at a time. Three things are not, and each gets a pass
//! of its own over the finished tokens rather than a rule bolted onto that
//! loop: [`calls`] marks the identifiers that name something callable,
//! [`types`] tells a type annotation's names from an expression's, and
//! [`interpolation`] lexes the `{...}` holes inside a backtick string as the
//! Luau expressions they are, by re-entering this lexer on them. None of the
//! three builds a tree; each only needs to know where one construct ends and
//! the next begins, which is a much smaller thing to know.

mod calls;
mod interpolation;
#[cfg(test)]
mod tests;
mod types;

use std::ops::Range;

/// Luau's reserved words, plus the three contextual ones (`continue`,
/// `export`, `type`) Studio also paints as keywords.
const KEYWORDS: [&str; 21] = [
    "and", "break", "continue", "do", "else", "elseif", "end", "export", "for", "function", "if",
    "in", "local", "not", "or", "repeat", "return", "then", "type", "until", "while",
];

/// Names every Roblox script has in scope without declaring them, which
/// Studio distinguishes from an ordinary local.
const SPECIAL_VARIABLES: [&str; 7] = [
    "_G",
    "game",
    "plugin",
    "script",
    "self",
    "shared",
    "workspace",
];

/// Longest match first: `..` must not shadow `...`, nor `/` shadow `//=`.
///
/// `|` and `&` are here for Luau's union and intersection types (`A | B`,
/// `A & B`), which is the only place either appears — Lua has no bitwise
/// operators and spells its logical ones `or`/`and`.
const OPERATORS: [&str; 30] = [
    "...", "//=", "..=", "==", "~=", "<=", ">=", "..", "::", "->", "+=", "-=", "*=", "/=", "%=",
    "^=", "//", "+", "-", "*", "/", "%", "^", "#", "=", "<", ">", "?", "|", "&",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Comment,
    String,
    Number,
    Keyword,
    Boolean,
    Nil,
    /// An identifier in call position — see [`calls`].
    Function,
    /// `self` and the Roblox globals; see [`SPECIAL_VARIABLES`].
    SpecialVariable,
    /// An identifier naming a type rather than a value — see [`types`].
    Type,
    /// A type Luau has without anyone declaring it (`number`, `string`).
    BuiltinType,
    Identifier,
    Operator,
    Bracket,
    Delimiter,
}

impl TokenKind {
    /// The semantic highlight name the active theme resolves to a colour (see
    /// `script_editor::highlight`). These are the names GPUI Kit's own theme
    /// files key their `syntax` section by, so a theme swap recolours this
    /// editor with no further work.
    ///
    /// `None` leaves the run in the editor's plain foreground: an ordinary
    /// identifier is uncoloured in Studio too, and painting every word some
    /// colour is what makes a highlighter unreadable.
    pub(crate) fn highlight_name(self) -> Option<&'static str> {
        match self {
            TokenKind::Comment => Some("comment"),
            TokenKind::String => Some("string"),
            TokenKind::Number => Some("number"),
            TokenKind::Keyword => Some("keyword"),
            TokenKind::Boolean => Some("boolean"),
            TokenKind::Nil => Some("constant"),
            TokenKind::Function => Some("function"),
            TokenKind::SpecialVariable => Some("variable.special"),
            TokenKind::Type => Some("type"),
            // GPUI Kit's shipped themes define no `type.builtin`, and its
            // resolver falls back on the prefix before the dot, so this paints
            // as `type` until some theme does define it. Kept distinct here
            // rather than collapsed into `Type`: the two are a real
            // distinction in the language, and one a theme can act on.
            TokenKind::BuiltinType => Some("type.builtin"),
            TokenKind::Operator => Some("operator"),
            TokenKind::Bracket => Some("punctuation.bracket"),
            TokenKind::Delimiter => Some("punctuation.delimiter"),
            TokenKind::Identifier => None,
        }
    }
}

/// One lexed token. `range` is a byte range into the source it was lexed
/// from, always on `char` boundaries, and tokens come back in source order
/// without overlapping — whitespace is simply not covered by any of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) range: Range<usize>,
    pub(crate) kind: TokenKind,
}

/// Lexes `source`. Never fails: unterminated strings and comments run to the
/// end of their line or of the file, because a highlighter has to keep
/// colouring a file that is mid-edit and therefore usually not yet valid.
pub(crate) fn tokenize(source: &str) -> Vec<Token> {
    tokenize_at(source, 0)
}

/// `depth` is how many interpolated strings this source sits inside — 0 for a
/// whole file, and one more for each hole `interpolation::expand` descends
/// into. It bounds that recursion and nothing else.
fn tokenize_at(source: &str, depth: usize) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;

    while at < bytes.len() {
        let start = at;
        let kind = match bytes[at] {
            b' ' | b'\t' | b'\r' | b'\n' => {
                at += 1;
                continue;
            }
            b'-' if bytes.get(at + 1) == Some(&b'-') => {
                at = comment_end(bytes, at);
                TokenKind::Comment
            }
            b'"' | b'\'' => {
                at = quoted_string_end(bytes, at);
                TokenKind::String
            }
            // One opaque token for now; `interpolation::expand` below splits
            // it once the call-position passes have seen the literal whole.
            b'`' => {
                at = interpolation::end(source, at, depth);
                TokenKind::String
            }
            // `[[` opens a long string even directly after a name, which is
            // why `t[[1]]` indexes nothing in Lua either — the lexer decides
            // this before any parser sees it, and so does this one.
            b'[' if long_bracket(bytes, at).is_some() => {
                at = long_string_end(bytes, at);
                TokenKind::String
            }
            b'0'..=b'9' => {
                at = number_end(bytes, at);
                TokenKind::Number
            }
            b'.' if bytes.get(at + 1).is_some_and(u8::is_ascii_digit) => {
                at = number_end(bytes, at);
                TokenKind::Number
            }
            byte if is_word_start(byte) => {
                at = word_end(bytes, at);
                word_kind(&source[start..at])
            }
            b'(' | b')' | b'{' | b'}' | b'[' | b']' => {
                at += 1;
                TokenKind::Bracket
            }
            _ => match punctuation(bytes, at) {
                Some((end, kind)) => {
                    at = end;
                    kind
                }
                // A byte the language has no token for at all — a stray
                // non-ASCII one outside any string, say. Stepping to the next
                // `char` boundary rather than by one byte is what keeps every
                // range returned here sliceable.
                None => {
                    at = next_boundary(source, at);
                    continue;
                }
            },
        };
        tokens.push(Token {
            range: start..at,
            kind,
        });
    }

    calls::mark_definitions(&mut tokens, source);
    calls::mark_calls(&mut tokens, source);
    types::mark(&mut tokens, source);
    interpolation::expand(source, &mut tokens, depth);
    tokens
}

fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn word_end(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
        at += 1;
    }
    at
}

fn word_kind(word: &str) -> TokenKind {
    match word {
        "nil" => TokenKind::Nil,
        "true" | "false" => TokenKind::Boolean,
        _ if KEYWORDS.contains(&word) => TokenKind::Keyword,
        _ if SPECIAL_VARIABLES.contains(&word) => TokenKind::SpecialVariable,
        _ => TokenKind::Identifier,
    }
}

/// `at` is the first of the two dashes. A `--[[` long comment runs to its
/// matching close, anything else to the end of the line.
fn comment_end(bytes: &[u8], at: usize) -> usize {
    let body = at + 2;
    if long_bracket(bytes, body).is_some() {
        return long_string_end(bytes, body);
    }
    let mut end = body;
    while end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    end
}

/// `at` is the opening quote (or backtick). An unterminated literal stops at
/// the newline rather than running on, so one stray quote cannot colour the
/// whole rest of the file as a string while it is being typed.
fn quoted_string_end(bytes: &[u8], at: usize) -> usize {
    let quote = bytes[at];
    let mut end = at + 1;
    while end < bytes.len() {
        match bytes[end] {
            // Skips whatever follows, which is also what makes a backslash
            // at end of line continue the literal onto the next one.
            b'\\' => end += 2,
            b'\n' => return end,
            byte if byte == quote => return end + 1,
            _ => end += 1,
        }
    }
    bytes.len()
}

/// The `=` count of the long bracket opening at `at`, if one does — `[[` is
/// level 0, `[==[` level 2.
fn long_bracket(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'[') {
        return None;
    }
    let mut level = 0;
    while bytes.get(at + 1 + level) == Some(&b'=') {
        level += 1;
    }
    (bytes.get(at + 1 + level) == Some(&b'[')).then_some(level)
}

/// `at` is the opening `[`. Runs to the matching `]=*]`, or to the end of the
/// file when there is none.
fn long_string_end(bytes: &[u8], at: usize) -> usize {
    let Some(level) = long_bracket(bytes, at) else {
        return at + 1;
    };
    let delimiter = level + 2;
    let mut end = at + delimiter;
    while end < bytes.len() {
        if bytes[end] == b']' && closes(bytes, end, level) {
            return end + delimiter;
        }
        end += 1;
    }
    bytes.len()
}

/// Whether the `]` at `at` begins the `]=*]` closing a level-`level` bracket.
fn closes(bytes: &[u8], at: usize, level: usize) -> bool {
    (1..=level).all(|offset| bytes.get(at + offset) == Some(&b'='))
        && bytes.get(at + level + 1) == Some(&b']')
}

fn number_end(bytes: &[u8], at: usize) -> usize {
    let mut end = at;
    // A `0x`/`0b` prefix changes which digits are legal, and neither form
    // takes a decimal point or an exponent afterwards.
    if bytes[end] == b'0' {
        if bytes
            .get(end + 1)
            .is_some_and(|b| b.eq_ignore_ascii_case(&b'x'))
        {
            end += 2;
            while end < bytes.len() && (bytes[end].is_ascii_hexdigit() || bytes[end] == b'_') {
                end += 1;
            }
            return end;
        }
        if bytes
            .get(end + 1)
            .is_some_and(|b| b.eq_ignore_ascii_case(&b'b'))
        {
            end += 2;
            while end < bytes.len() && matches!(bytes[end], b'0' | b'1' | b'_') {
                end += 1;
            }
            return end;
        }
    }
    while end < bytes.len() {
        match bytes[end] {
            // Luau's digit separator: `1_000_000`.
            b'0'..=b'9' | b'_' => end += 1,
            // Not a second dot: `1..2` is a concatenation of two numbers, not
            // one malformed number.
            b'.' if bytes.get(end + 1) != Some(&b'.') => end += 1,
            b'e' | b'E' => {
                end += 1;
                // The sign belongs to the exponent: `1e-3` is one number,
                // where `1-3` is a subtraction.
                if matches!(bytes.get(end), Some(b'+' | b'-')) {
                    end += 1;
                }
            }
            _ => break,
        }
    }
    end
}

fn punctuation(bytes: &[u8], at: usize) -> Option<(usize, TokenKind)> {
    for operator in OPERATORS {
        if bytes[at..].starts_with(operator.as_bytes()) {
            return Some((at + operator.len(), TokenKind::Operator));
        }
    }
    matches!(bytes[at], b',' | b';' | b':' | b'.').then_some((at + 1, TokenKind::Delimiter))
}

fn next_boundary(source: &str, at: usize) -> usize {
    let mut next = at + 1;
    while next < source.len() && !source.is_char_boundary(next) {
        next += 1;
    }
    next
}
