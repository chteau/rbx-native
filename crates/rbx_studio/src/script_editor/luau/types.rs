//! Telling a type annotation's names apart from an expression's.
//!
//! `local n: Vector3` and `local n = Vector3` put the same identifier either
//! side of the same token, so nothing local to one token decides this — but a
//! full parse is not needed either. What is needed is only enough structure to
//! find where a type *starts* (the four places Luau lets one: after a `local`
//! name, after a parameter name, after a parameter list for a return type, and
//! after `type Name =`, plus the `::` cast operator) and to walk one type
//! expression to its end. That is a small recursive descent over the token
//! stream the lexer already produced, run before the holes of any interpolated
//! string are expanded — a hole re-enters the lexer, so it gets its own pass.
//!
//! Nothing outside those four entry points is ever looked at, which is what
//! keeps an ordinary `obj:method()` from reading as an annotation: its `:` is
//! not in any of them.

#[cfg(test)]
mod tests;

use super::{Token, TokenKind};

/// The type names Luau has without anyone declaring them.
///
/// `nil`, `true` and `false` are also legal types, and are deliberately absent:
/// they lex as the literals they are and keep the colour they have everywhere
/// else, rather than changing appearance based on where they sit.
const BUILTIN_TYPES: [&str; 12] = [
    "any", "boolean", "buffer", "never", "nil", "number", "string", "table", "thread", "unknown",
    "userdata", "vector",
];

/// Marks every identifier that names a type rather than a value.
pub(super) fn mark(tokens: &mut [Token], source: &str) {
    let mut pass = Pass { tokens, source };
    let mut at = 0;
    while at < pass.tokens.len() {
        let next = match pass.kind(at) {
            TokenKind::Keyword => match pass.text(at) {
                "local" => pass.local_statement(at),
                "type" => pass.type_declaration(at),
                "function" => pass.function_signature(at),
                _ => at + 1,
            },
            // `x :: number`, the cast operator.
            TokenKind::Operator if pass.text(at) == "::" => pass.type_expression(at + 1),
            _ => at + 1,
        };
        at = next.max(at + 1);
    }
}

struct Pass<'a> {
    tokens: &'a mut [Token],
    source: &'a str,
}

impl Pass<'_> {
    /// `local a: T, b: U = ...`. Stops at the `=`, or at the first thing that
    /// is not part of a name list — `local function f()` among them, whose
    /// signature the driver reaches on its own next step.
    fn local_statement(&mut self, at: usize) -> usize {
        let mut at = at + 1;
        while matches!(
            self.kind(at),
            TokenKind::Identifier | TokenKind::SpecialVariable
        ) {
            at += 1;
            if self.is_delimiter(at, b':') {
                at = self.type_expression(at + 1);
            }
            if !self.is_delimiter(at, b',') {
                break;
            }
            at += 1;
        }
        at
    }

    /// `type Name<A, B> = T`, and the `export type` spelling of it — `export`
    /// is a keyword of its own, so the driver arrives here either way.
    ///
    /// Returns immediately for the other uses of a word Luau only makes a
    /// keyword in context: `type(x)` calls the global, `local type = 1` names
    /// a variable, and neither is followed by the name a declaration needs.
    fn type_declaration(&mut self, at: usize) -> usize {
        let mut cursor = at + 1;
        if !matches!(
            self.kind(cursor),
            TokenKind::Identifier | TokenKind::Function
        ) {
            return at + 1;
        }
        self.mark_type(cursor);
        cursor += 1;
        if self.is_operator(cursor, "<") {
            cursor = self.generic_list(cursor);
        }
        if self.is_operator(cursor, "=") {
            cursor = self.type_expression(cursor + 1);
        }
        cursor
    }

    /// `function a.b:c<T>(x: U, y: V): W`. The name path is skipped rather
    /// than inspected — `mark_definitions` has already dealt with it — and the
    /// generic list, parameter list and return type each have their own rule.
    fn function_signature(&mut self, at: usize) -> usize {
        let mut cursor = at + 1;
        while matches!(
            self.kind(cursor),
            TokenKind::Identifier | TokenKind::Function | TokenKind::SpecialVariable
        ) || self.is_delimiter(cursor, b'.')
            || self.is_delimiter(cursor, b':')
        {
            cursor += 1;
        }
        if self.is_operator(cursor, "<") {
            cursor = self.generic_list(cursor);
        }
        if self.is_bracket(cursor, b'(') {
            cursor = self.declaration_parameters(cursor);
            // Only a parameter list's own `)` can be followed by a return
            // type. An expression's cannot, which is why `(value):method()`
            // never reaches this.
            if self.is_delimiter(cursor, b':') {
                cursor = self.type_expression(cursor + 1);
            }
        }
        cursor
    }

    /// A declaration's `(...)`: every parameter is named, so the only types in
    /// it follow a `:` at the list's own depth.
    fn declaration_parameters(&mut self, open: usize) -> usize {
        let mut depth = 0usize;
        let mut at = open;
        while at < self.tokens.len() {
            if self.is_bracket(at, b'(') {
                depth += 1;
                at += 1;
            } else if self.is_bracket(at, b')') {
                depth -= 1;
                at += 1;
                if depth == 0 {
                    return at;
                }
            } else if depth == 1 && self.is_delimiter(at, b':') {
                at = self.type_expression(at + 1);
            } else {
                at += 1;
            }
        }
        at
    }

    /// One type: `A | B`, `A & B`, and everything either side of them.
    fn type_expression(&mut self, at: usize) -> usize {
        let mut at = self.type_term(at);
        while self.is_operator(at, "|") || self.is_operator(at, "&") {
            let next = self.type_term(at + 1);
            at = next.max(at + 1);
        }
        at
    }

    fn type_term(&mut self, at: usize) -> usize {
        let mut at = at;
        // A variadic type pack: `...number`.
        if self.is_operator(at, "...") {
            at += 1;
        }
        at = self.type_primary(at);
        // `T?` is optional, and `T??` is legal if pointless.
        while self.is_operator(at, "?") {
            at += 1;
        }
        at
    }

    fn type_primary(&mut self, at: usize) -> usize {
        if at >= self.tokens.len() {
            return at;
        }
        if self.is_bracket(at, b'(') {
            let after = self.type_parameters(at);
            return match self.is_operator(after, "->") {
                true => self.type_expression(after + 1),
                false => after,
            };
        }
        if self.is_bracket(at, b'{') {
            return self.table_type(at);
        }
        match self.kind(at) {
            // Singleton types: `"fire" | "water"`, `true`, `nil`. Each keeps
            // the colour its literal has anywhere else (see `BUILTIN_TYPES`).
            TokenKind::String | TokenKind::Boolean | TokenKind::Nil => at + 1,
            TokenKind::Identifier | TokenKind::Function | TokenKind::SpecialVariable => {
                // `typeof(x)` takes an expression, not a type, so the names
                // inside its parentheses are left exactly as they were lexed.
                if self.text(at) == "typeof" && self.is_bracket(at + 1, b'(') {
                    return self.skip_parentheses(at + 1);
                }
                let mut end = at;
                self.mark_type(end);
                // A qualified name — `Roact.Element` — is one type name in
                // two halves, so both halves are painted as one.
                while self.is_delimiter(end + 1, b'.')
                    && matches!(
                        self.kind(end + 2),
                        TokenKind::Identifier | TokenKind::Function
                    )
                {
                    end += 2;
                    self.mark_type(end);
                }
                match self.is_operator(end + 1, "<") {
                    true => self.generic_list(end + 1),
                    false => end + 1,
                }
            }
            _ => at,
        }
    }

    /// A function type's `(...)`: `(number, string) -> ()` names nothing,
    /// `(self: T, n: number) -> ()` names everything, and both are legal, so
    /// an entry is a type unless a `:` shows it was a name.
    fn type_parameters(&mut self, open: usize) -> usize {
        let mut at = open + 1;
        while at < self.tokens.len() && !self.is_bracket(at, b')') {
            if self.is_delimiter(at, b',') {
                at += 1;
                continue;
            }
            if self.names_a_field(at) {
                at = self.type_expression(at + 2);
                continue;
            }
            at = self.type_expression(at).max(at + 1);
        }
        match self.is_bracket(at, b')') {
            true => at + 1,
            false => at,
        }
    }

    /// `{ T }`, `{ name: T }`, `{ [K]: V }` — a field's name is not a type,
    /// and everything else between the braces is.
    fn table_type(&mut self, open: usize) -> usize {
        let mut at = open + 1;
        while at < self.tokens.len() && !self.is_bracket(at, b'}') {
            if self.is_delimiter(at, b',') || self.is_delimiter(at, b';') {
                at += 1;
                continue;
            }
            if self.is_bracket(at, b'[') {
                at = self.type_expression(at + 1);
                if self.is_bracket(at, b']') {
                    at += 1;
                }
                if self.is_delimiter(at, b':') {
                    at = self.type_expression(at + 1);
                }
                continue;
            }
            if self.names_a_field(at) {
                at = self.type_expression(at + 2);
                continue;
            }
            at = self.type_expression(at).max(at + 1);
        }
        match self.is_bracket(at, b'}') {
            true => at + 1,
            false => at,
        }
    }

    /// `<A, B>`, whether it declares type parameters or supplies type
    /// arguments — every name in one is a type either way.
    ///
    /// Bails at anything a generic list cannot contain, so a `<` that turned
    /// out to be a comparison cannot paint the rest of the file as types.
    fn generic_list(&mut self, open: usize) -> usize {
        let mut depth = 0usize;
        let mut at = open;
        while at < self.tokens.len() {
            if self.is_operator(at, "<") {
                depth += 1;
                at += 1;
                continue;
            }
            if self.is_operator(at, ">") {
                depth -= 1;
                at += 1;
                if depth == 0 {
                    return at;
                }
                continue;
            }
            match self.kind(at) {
                TokenKind::Keyword => return at,
                TokenKind::Bracket if matches!(self.first_byte(at), Some(b')' | b'}' | b']')) => {
                    return at
                }
                TokenKind::Identifier | TokenKind::Function => self.mark_type(at),
                _ => {}
            }
            at += 1;
        }
        at
    }

    fn skip_parentheses(&mut self, open: usize) -> usize {
        let mut depth = 0usize;
        let mut at = open;
        while at < self.tokens.len() {
            if self.is_bracket(at, b'(') {
                depth += 1;
            } else if self.is_bracket(at, b')') {
                depth -= 1;
                if depth == 0 {
                    return at + 1;
                }
            }
            at += 1;
        }
        at
    }

    fn names_a_field(&self, at: usize) -> bool {
        matches!(
            self.kind(at),
            TokenKind::Identifier | TokenKind::Function | TokenKind::SpecialVariable
        ) && self.is_delimiter(at + 1, b':')
    }

    fn mark_type(&mut self, at: usize) {
        if !matches!(self.kind(at), TokenKind::Identifier | TokenKind::Function) {
            return;
        }
        self.tokens[at].kind = match BUILTIN_TYPES.contains(&self.text(at)) {
            true => TokenKind::BuiltinType,
            false => TokenKind::Type,
        };
    }

    fn kind(&self, at: usize) -> TokenKind {
        self.tokens
            .get(at)
            .map_or(TokenKind::Delimiter, |token| token.kind)
    }

    fn text(&self, at: usize) -> &str {
        self.tokens
            .get(at)
            .map_or("", |token| &self.source[token.range.clone()])
    }

    fn first_byte(&self, at: usize) -> Option<u8> {
        self.tokens
            .get(at)
            .map(|token| self.source.as_bytes()[token.range.start])
    }

    fn is_operator(&self, at: usize, operator: &str) -> bool {
        self.kind(at) == TokenKind::Operator && self.text(at) == operator
    }

    fn is_bracket(&self, at: usize, bracket: u8) -> bool {
        self.kind(at) == TokenKind::Bracket && self.first_byte(at) == Some(bracket)
    }

    fn is_delimiter(&self, at: usize, delimiter: u8) -> bool {
        self.kind(at) == TokenKind::Delimiter && self.first_byte(at) == Some(delimiter)
    }
}
