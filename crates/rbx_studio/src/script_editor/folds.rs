//! Fold ranges for the script editor, from a real parse.
//!
//! The highlighter's lexer sees tokens, not blocks, so block extents come
//! from `full_moon`'s error-tolerant parse (the same crate Format Document
//! and the script graph use). Comments and long strings are single tokens, so
//! they are read straight off the token stream.
//!
//! A [`FoldRange`] here is `(opener line, closer line)`, 0-based. The editor
//! hides the lines strictly between them, so both the opener and the closer
//! stay visible, which is what Roblox Studio shows (`end`, `}`, `until` and
//! `]]` stay put).

pub(crate) mod saved;
#[cfg(test)]
mod tests;

use full_moon::ast;
use full_moon::tokenizer::{Lexer, LexerResult, StringLiteralQuoteType, TokenReference, TokenType};
use full_moon::visitors::Visitor;
use full_moon::LuaVersion;
use gpui_kit::base::input::FoldRange;

use super::highlight::MAX_HIGHLIGHTED_BYTES;

/// Every block of `text` spanning at least one line to hide, sorted by
/// opener line, one per line (the outermost wins when several open on it).
///
/// A half-typed script still folds whatever parsed: full_moon rebuilds the
/// rest with phantom tokens, which carry no position and are skipped.
#[cfg(test)]
pub(crate) fn fold_ranges(text: &str) -> Vec<FoldRange> {
    try_fold_ranges(text).unwrap_or_default()
}

/// [`fold_ranges`], or `None` when the parse failed and found no block at
/// all: the caller then keeps what it had, since a script mid-edit is far
/// likelier than one that just lost every block.
pub(crate) fn try_fold_ranges(text: &str) -> Option<Vec<FoldRange>> {
    if text.len() > MAX_HIGHLIGHTED_BYTES {
        return Some(Vec::new());
    }
    let result = full_moon::parse_fallible(text, LuaVersion::luau());
    let mut collector = Collector::default();
    collector.visit_ast(result.ast());
    // Comments and long strings are single tokens; the lexer reads them
    // whether or not the rest of the file parsed.
    let tokens = match Lexer::new(text, LuaVersion::luau()).collect() {
        LexerResult::Ok(tokens) | LexerResult::Recovered(tokens, _) => tokens,
        LexerResult::Fatal(_) => Vec::new(),
    };
    for token in &tokens {
        collector.token(token);
    }
    let folds = collector.finish();
    (!folds.is_empty() || result.errors().is_empty()).then_some(folds)
}

#[derive(Default)]
struct Collector {
    /// `(opener line, closer line)`, 0-based.
    spans: Vec<(usize, usize)>,
}

impl Collector {
    fn finish(self) -> Vec<FoldRange> {
        let mut spans = self.spans;
        // Outermost first on a shared opener line, then keep that one.
        spans.sort_by_key(|&(start, end)| (start, std::cmp::Reverse(end)));
        spans.dedup_by_key(|&mut (start, _)| start);
        spans
            .into_iter()
            .map(|(start, end)| FoldRange::new(start, end))
            .collect()
    }

    fn block(&mut self, opener: &TokenReference, closer: &TokenReference) {
        if let (Some(start), Some(end)) = (line_of(opener), line_of(closer)) {
            self.span(start, end);
        }
    }

    fn span(&mut self, start: usize, end: usize) {
        // Adjacent lines leave nothing to hide.
        if end >= start + 2 {
            self.spans.push((start, end));
        }
    }

    fn token(&mut self, token: &full_moon::tokenizer::Token) {
        let multi_line = match token.token_type() {
            TokenType::MultiLineComment { .. } => true,
            TokenType::StringLiteral { quote_type, .. } => {
                *quote_type == StringLiteralQuoteType::Brackets
            }
            _ => false,
        };
        let (start, end) = (token.start_position(), token.end_position());
        if multi_line && start.line() > 0 {
            self.span(start.line() - 1, end.line().saturating_sub(1));
        }
    }
}

/// The 0-based line a token starts on, or `None` for a phantom token.
fn line_of(token: &TokenReference) -> Option<usize> {
    token.start_position().line().checked_sub(1)
}

impl Visitor for Collector {
    fn visit_function_declaration(&mut self, node: &ast::FunctionDeclaration) {
        self.block(node.function_token(), node.body().end_token());
    }

    fn visit_local_function(&mut self, node: &ast::LocalFunction) {
        self.block(node.function_token(), node.body().end_token());
    }

    fn visit_anonymous_function(&mut self, node: &ast::AnonymousFunction) {
        self.block(node.function_token(), node.body().end_token());
    }

    /// Each branch folds on its own, up to the token that starts the next.
    fn visit_if(&mut self, node: &ast::If) {
        let mut openers = vec![node.if_token()];
        openers.extend(
            node.else_if()
                .into_iter()
                .flatten()
                .map(|b| b.else_if_token()),
        );
        openers.extend(node.else_token());
        let closers = openers
            .iter()
            .skip(1)
            .copied()
            .chain(std::iter::once(node.end_token()));
        for (opener, closer) in openers.iter().zip(closers) {
            self.block(opener, closer);
        }
    }

    fn visit_numeric_for(&mut self, node: &ast::NumericFor) {
        self.block(node.for_token(), node.end_token());
    }

    fn visit_generic_for(&mut self, node: &ast::GenericFor) {
        self.block(node.for_token(), node.end_token());
    }

    fn visit_while(&mut self, node: &ast::While) {
        self.block(node.while_token(), node.end_token());
    }

    fn visit_repeat(&mut self, node: &ast::Repeat) {
        self.block(node.repeat_token(), node.until_token());
    }

    fn visit_do(&mut self, node: &ast::Do) {
        self.block(node.do_token(), node.end_token());
    }

    fn visit_table_constructor(&mut self, node: &ast::TableConstructor) {
        let (open, close) = node.braces().tokens();
        self.block(open, close);
    }
}
