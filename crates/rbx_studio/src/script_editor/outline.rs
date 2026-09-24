//! What a script declares, read off the lexer's tokens: the functions the
//! Script Function Filter lists, and the declaration Go to Declaration jumps
//! to.
//!
//! No parse tree — the editor has none, and `luau-lsp` is the place a real
//! one belongs. Scoping is tracked by block keywords alone (`function`, `do`,
//! `then`, `repeat` open a block; `end`, `until`, `elseif`, `else` close
//! one), which is exact for well-formed code and degrades to "nearest earlier
//! declaration" for code mid-edit.

use std::ops::Range;

use super::luau::{self, Token, TokenKind};

#[cfg(test)]
mod tests;

/// One named function: `function a.b:c()`, `local function f()`, or a
/// function expression assigned to a name (`local f = function()`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Function {
    /// The whole name path as written, `a.b:c`.
    pub(crate) name: String,
    /// Where the path's last segment sits, which is what a jump selects.
    pub(crate) range: Range<usize>,
}

/// Every named function in `source`, in source order.
pub(crate) fn functions(source: &str) -> Vec<Function> {
    let tokens = significant(source);
    let mut found = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !is_word(source, token, "function") {
            continue;
        }
        let path = name_path_after(&tokens, index, source);
        let path = if path.is_empty() {
            assigned_path_before(&tokens, index, source)
        } else {
            path
        };
        let (Some(first), Some(last)) = (path.first(), path.last()) else {
            continue;
        };
        found.push(Function {
            name: source[tokens[*first].range.start..tokens[*last].range.end].to_string(),
            range: tokens[*last].range.clone(),
        });
    }
    found
}

/// Where the name under `offset` was declared, or `None` if `offset` is not
/// on a name or nothing in `source` declares it.
///
/// A local visible from `offset` wins, innermost and latest first; failing
/// that, any `function` statement whose name path ends in the same word — the
/// case for a global function defined further down, or `M.helper()` /
/// `self:method()` naming a function declared as `function M.helper`.
pub(crate) fn declaration(source: &str, offset: usize) -> Option<Range<usize>> {
    let tokens = significant(source);
    let at = tokens.iter().position(|token| {
        is_name(token) && token.range.start <= offset && offset <= token.range.end
    })?;
    let name = &source[tokens[at].range.clone()];
    // `obj.x` is a field, never the local `x`, so only the function
    // statements are candidates for it.
    let member = at > 0
        && (is_text(source, tokens.get(at - 1), ".") || is_text(source, tokens.get(at - 1), ":"));
    let walk = Walk::run(&tokens, source, at);

    walk.declarations
        .iter()
        .rev()
        .find(|decl| {
            !member
                && decl.range.start <= tokens[at].range.start
                && walk.visible.contains(&decl.scope)
                && &source[decl.range.clone()] == name
        })
        .map(|decl| decl.range.clone())
        .or_else(|| {
            functions(source)
                .into_iter()
                .find(|function| &source[function.range.clone()] == name)
                .map(|function| function.range)
        })
}

struct Declaration {
    range: Range<usize>,
    scope: usize,
}

/// One pass over the tokens, recording each local declaration against the
/// block it lives in.
struct Walk {
    declarations: Vec<Declaration>,
    /// The blocks open at the queried token — the only ones whose locals it
    /// can see.
    visible: Vec<usize>,
}

impl Walk {
    fn run(tokens: &[Token], source: &str, until: usize) -> Self {
        let mut declarations = Vec::new();
        let mut stack = vec![0];
        let mut next_scope = 1;
        // `for` loop variables, waiting for the `do` whose block they belong to.
        let mut pending: Vec<Range<usize>> = Vec::new();

        for index in 0..until {
            let token = &tokens[index];
            if token.kind != TokenKind::Keyword {
                continue;
            }
            let top = *stack.last().unwrap_or(&0);
            match &source[token.range.clone()] {
                "local" => {
                    for name in local_names(tokens, index, source) {
                        declarations.push(Declaration {
                            range: tokens[name].range.clone(),
                            scope: top,
                        });
                    }
                }
                "for" => pending.extend(for_names(tokens, index, source)),
                "function" => {
                    // A `local function`'s name was already declared by the
                    // `local` before it, in the enclosing block.
                    stack.push(next_scope);
                    for name in parameters(tokens, index, source) {
                        declarations.push(Declaration {
                            range: tokens[name].range.clone(),
                            scope: next_scope,
                        });
                    }
                    next_scope += 1;
                }
                word @ ("do" | "then" | "repeat" | "else") => {
                    if word == "else" && stack.len() > 1 {
                        stack.pop();
                    }
                    stack.push(next_scope);
                    if word == "do" {
                        declarations.extend(pending.drain(..).map(|range| Declaration {
                            range,
                            scope: next_scope,
                        }));
                    }
                    next_scope += 1;
                }
                "end" | "until" | "elseif" => {
                    if stack.len() > 1 {
                        stack.pop();
                    }
                }
                _ => {}
            }
        }
        Walk {
            declarations,
            visible: stack,
        }
    }
}

/// The lexer's tokens without comments, which never take part in any of the
/// patterns below and would otherwise split them.
fn significant(source: &str) -> Vec<Token> {
    luau::tokenize(source)
        .into_iter()
        .filter(|token| token.kind != TokenKind::Comment)
        .collect()
}

/// An identifier as the lexer classifies it: plain, in call position, or the
/// name a `function` statement defines.
fn is_name(token: &Token) -> bool {
    matches!(token.kind, TokenKind::Identifier | TokenKind::Function)
}

fn is_word(source: &str, token: &Token, word: &str) -> bool {
    token.kind == TokenKind::Keyword && &source[token.range.clone()] == word
}

fn is_text(source: &str, token: Option<&Token>, text: &str) -> bool {
    token.is_some_and(|token| &source[token.range.clone()] == text)
}

/// The `a`, `b`, `c` of `function a.b:c` — token indices, empty for an
/// anonymous `function(`.
fn name_path_after(tokens: &[Token], keyword: usize, source: &str) -> Vec<usize> {
    let mut path = Vec::new();
    let mut index = keyword + 1;
    while tokens.get(index).is_some_and(is_name) {
        path.push(index);
        let separator = tokens.get(index + 1);
        if !(is_text(source, separator, ".") || is_text(source, separator, ":")) {
            break;
        }
        index += 2;
    }
    path
}

/// The `a.b` of `a.b = function` or `local a = function`, walked backwards
/// from the `=`; empty if the function expression isn't assigned to a name.
fn assigned_path_before(tokens: &[Token], keyword: usize, source: &str) -> Vec<usize> {
    if keyword < 2 || !is_text(source, tokens.get(keyword - 1), "=") {
        return Vec::new();
    }
    let mut path = Vec::new();
    let mut index = keyword - 2;
    while is_name(&tokens[index]) || tokens[index].kind == TokenKind::SpecialVariable {
        path.push(index);
        if index < 2 || !is_text(source, tokens.get(index - 1), ".") {
            break;
        }
        index -= 2;
    }
    path.reverse();
    path
}

/// The names a `local` statement declares: `local function f`'s `f`, or
/// every name of `local a: T, b = ...`.
fn local_names(tokens: &[Token], keyword: usize, source: &str) -> Vec<usize> {
    if tokens
        .get(keyword + 1)
        .is_some_and(|next| is_word(source, next, "function"))
    {
        return name_path_after(tokens, keyword + 1, source)
            .first()
            .copied()
            .into_iter()
            .collect();
    }
    name_list(tokens, keyword + 1, source)
}

/// The loop variables of `for i = ...` or `for k, v in ...`.
fn for_names(tokens: &[Token], keyword: usize, source: &str) -> Vec<Range<usize>> {
    name_list(tokens, keyword + 1, source)
        .into_iter()
        .map(|index| tokens[index].range.clone())
        .collect()
}

/// `a, b: T, c` starting at `start`: each name, skipping a type annotation up
/// to the next comma at bracket depth zero — or the end of the line, since a
/// `local x: number` with no initializer is followed by the next statement.
fn name_list(tokens: &[Token], start: usize, source: &str) -> Vec<usize> {
    let mut names = Vec::new();
    let mut index = start;
    while tokens.get(index).is_some_and(is_name) {
        names.push(index);
        index += 1;
        if is_text(source, tokens.get(index), ":") {
            let mut depth = 0usize;
            index += 1;
            while let Some(token) = tokens.get(index) {
                let text = &source[token.range.clone()];
                let line_break = source[tokens[index - 1].range.end..token.range.start]
                    .contains('\n');
                if depth == 0 && (text == "," || text == "=" || line_break) {
                    break;
                }
                match text {
                    "(" | "{" | "[" => depth += 1,
                    ")" | "}" | "]" => depth = depth.saturating_sub(1),
                    _ => {}
                }
                index += 1;
            }
        }
        if !is_text(source, tokens.get(index), ",") {
            break;
        }
        index += 1;
    }
    names
}

/// The parameter names of the function whose `function` keyword is at
/// `keyword`: each name directly after the list's `(` or a `,` at its own
/// depth, which skips over the annotations' own identifiers.
fn parameters(tokens: &[Token], keyword: usize, source: &str) -> Vec<usize> {
    let Some(open) = (keyword + 1..tokens.len()).find(|&i| is_text(source, tokens.get(i), "("))
    else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut depth = 0usize;
    for index in open..tokens.len() {
        match &source[tokens[index].range.clone()] {
            "(" | "{" | "[" => depth += 1,
            ")" | "}" | "]" => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        let after_separator = index > open
            && matches!(&source[tokens[index - 1].range.clone()], "(" | ",");
        if depth == 1 && after_separator && is_name(&tokens[index]) {
            names.push(index);
        }
    }
    names
}
