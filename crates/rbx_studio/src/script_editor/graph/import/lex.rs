//! Luau split into tokens, enough of it to find where statements and
//! expressions begin and end. Comments are kept apart, by byte range, so
//! the importer can tell when a statement holds one it must not drop.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum T {
    Name,
    Keyword,
    Number,
    /// A quoted or long-bracket string.
    Str,
    /// A backtick string: it reads names inside, so it is never a literal.
    Interp,
    Op,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Tok {
    pub(super) t: T,
    pub(super) start: usize,
    pub(super) end: usize,
}

pub(super) struct Lexed {
    pub(super) toks: Vec<Tok>,
    pub(super) comments: Vec<(usize, usize)>,
}

const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local",
    "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

const OPS: &[&str] = &[
    "...", "..=", "//=", "..", "==", "~=", "<=", ">=", "//", "+=", "-=", "*=", "/=", "%=", "^=",
    "::", "->",
];

/// `None` for text that is not Luau this lexer understands: an
/// unterminated string or comment, or a character Luau has no use for.
pub(super) fn lex(src: &str) -> Option<Lexed> {
    let bytes = src.as_bytes();
    let mut toks = Vec::new();
    let mut comments = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if src[i..].starts_with("--") {
            i = match long_bracket(bytes, i + 2) {
                Some(level) => close_long(src, i + 2, level)?,
                None => src[i..].find('\n').map_or(bytes.len(), |n| i + n),
            };
            comments.push((start, i));
            continue;
        }
        let t = if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            match KEYWORDS.contains(&&src[start..i]) {
                true => T::Keyword,
                false => T::Name,
            }
        } else if c.is_ascii_digit()
            || (c == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            while i < bytes.len() {
                let d = bytes[i];
                if (d == b'+' || d == b'-') && matches!(bytes[i - 1], b'e' | b'E') {
                    i += 1;
                } else if d.is_ascii_alphanumeric() || d == b'_' || d == b'.' {
                    i += 1;
                } else {
                    break;
                }
            }
            T::Number
        } else if c == b'"' || c == b'\'' {
            i = close_quote(bytes, i + 1, c)?;
            T::Str
        } else if c == b'`' {
            i = close_interp(bytes, i + 1)?;
            T::Interp
        } else if let Some(level) = (c == b'[').then(|| long_bracket(bytes, i)).flatten() {
            i = close_long(src, i, level)?;
            T::Str
        } else {
            let op = OPS.iter().find(|op| src[i..].starts_with(**op));
            i += op.map_or(1, |op| op.len());
            if op.is_none() && !b"+-*/%^#=<>(){}[];:,.~|&?@!".contains(&c) {
                return None;
            }
            T::Op
        };
        toks.push(Tok { t, start, end: i });
    }
    Some(Lexed { toks, comments })
}

/// The level of a long bracket opening at `i` (`[[` is 0, `[==[` is 2).
fn long_bracket(bytes: &[u8], i: usize) -> Option<usize> {
    if bytes.get(i) != Some(&b'[') {
        return None;
    }
    let level = bytes[i + 1..].iter().take_while(|&&b| b == b'=').count();
    (bytes.get(i + 1 + level) == Some(&b'[')).then_some(level)
}

/// Past the `]==]` closing a long bracket that opens at `i`.
fn close_long(src: &str, i: usize, level: usize) -> Option<usize> {
    let close = format!("]{}]", "=".repeat(level));
    let body = i + level + 2;
    src[body..].find(&close).map(|n| body + n + close.len())
}

fn close_quote(bytes: &[u8], mut i: usize, quote: u8) -> Option<usize> {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' => return None,
            b if b == quote => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// Past a backtick string, skipping the expressions in its braces.
fn close_interp(bytes: &[u8], mut i: usize) -> Option<usize> {
    let mut depth = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'"' | b'\'' if depth > 0 => i = close_quote(bytes, i + 1, bytes[i])? - 1,
            b'`' if depth == 0 => return Some(i + 1),
            b'\n' if depth == 0 => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

/// A string literal's value, for the escapes a typed literal can hold;
/// `None` for anything rarer, which then stays as written.
pub(super) fn unquote(text: &str) -> Option<String> {
    if let Some(level) = long_bracket(text.as_bytes(), 0) {
        let body = &text[level + 2..text.len() - level - 2];
        let body = body
            .strip_prefix("\r\n")
            .or_else(|| body.strip_prefix('\n'));
        return body
            .map(str::to_owned)
            .or_else(|| Some(text[level + 2..text.len() - level - 2].to_owned()));
    }
    let inner = text.get(1..text.len().checked_sub(1)?)?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        out.push(match chars.next()? {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '\\' => '\\',
            '"' => '"',
            '\'' => '\'',
            _ => return None,
        });
    }
    Some(out)
}
