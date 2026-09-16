//! Which identifiers name something callable.
//!
//! Two rules, both positional and neither needing a parse: an identifier
//! immediately before a call's arguments is being called, and the name after
//! a `function` keyword is being defined. They overlap for the common
//! `function f(x)` — the definition puts its name right before the parameter
//! list — and the second exists for the case where they do not.

use super::{Token, TokenKind};

/// Marks the name a `function` statement defines.
///
/// [`mark_calls`] below catches most of these for free, since a definition
/// usually puts its name immediately before the parameter list. A generic one
/// does not: `function f<T>(x)` puts `<T>` in between, and `f` would stay an
/// ordinary identifier. Walking forward from the keyword instead needs no rule
/// for the generics at all — the name is the last identifier of the `a.b:c`
/// path following `function`, whatever comes after it.
pub(super) fn mark_definitions(tokens: &mut [Token], source: &str) {
    for index in 0..tokens.len() {
        if tokens[index].kind != TokenKind::Keyword
            || &source[tokens[index].range.clone()] != "function"
        {
            continue;
        }
        let mut name = None;
        for (step, token) in tokens.iter().enumerate().skip(index + 1) {
            match token.kind {
                TokenKind::Identifier => name = Some(step),
                // `game` in `function game.foo()` stays the global it is; the
                // name being defined is whatever the path ends at.
                TokenKind::Comment | TokenKind::SpecialVariable => {}
                TokenKind::Delimiter
                    if matches!(source.as_bytes()[token.range.start], b'.' | b':') => {}
                // Anything else ends the path: `(` for the parameter list,
                // `<` for a generic one, or a syntax error mid-edit.
                _ => break,
            }
        }
        if let Some(name) = name {
            tokens[name].kind = TokenKind::Function;
        }
    }
}

/// Promotes an identifier in call position to [`TokenKind::Function`]: `f(`,
/// and Lua's parenthesis-free `f{...}` / `f"..."` call sugar. A definition's
/// name needs no rule of its own — `function f(`, `function a.b:c(` and
/// `local function f(` all put the name immediately before its parameter
/// list, so this already catches every one of them.
pub(super) fn mark_calls(tokens: &mut [Token], source: &str) {
    for index in 0..tokens.len() {
        if tokens[index].kind != TokenKind::Identifier {
            continue;
        }
        let called = tokens[index + 1..]
            .iter()
            .find(|token| token.kind != TokenKind::Comment)
            .is_some_and(|token| match token.kind {
                TokenKind::String => true,
                TokenKind::Bracket => matches!(source.as_bytes()[token.range.start], b'(' | b'{'),
                _ => false,
            });
        if called {
            tokens[index].kind = TokenKind::Function;
        }
    }
}
