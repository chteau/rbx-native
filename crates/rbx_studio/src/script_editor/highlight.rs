//! Bridges [`crate::script_editor::luau`]'s tokens onto GPUI Kit's editor.
//!
//! `gpui_base` keeps syntax highlighting behind a trait rather than binding it
//! to tree-sitter, which is the whole reason this feature needs no grammar
//! crate: an [`InputHighlighter`] hands back styled byte ranges and the editor
//! paints them. Colours come from the active theme's own `syntax` palette, so
//! the editor follows a light/dark switch with nothing further to do here.

#[cfg(test)]
mod tests;

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
    InputHighlighterFactory, Rope,
};
use gpui_kit::{Context, HighlightStyle, SharedString, Window};

use super::luau::{self, Token};

/// The language name the editor is built with, and the key the factory below
/// answers to. Not `"lua"`: the editor would then pick up GPUI Kit's own Lua
/// grammar instead, were the `tree-sitter-lua` feature ever switched on for
/// some other reason.
pub(crate) const LANGUAGE: &str = "luau";

/// Past this, the editor shows plain unhighlighted text. Lexing is a single
/// linear pass, but it runs on the UI thread for every keystroke, and a
/// multi-megabyte `Source` is far likelier to be generated data pasted into a
/// script than something anyone is reading colour in.
const MAX_HIGHLIGHTED_BYTES: usize = 512 * 1024;

pub(crate) fn factory() -> InputHighlighterFactory {
    Rc::new(|language| {
        (language == LANGUAGE)
            .then(|| Box::new(LuauHighlighter::default()) as Box<dyn InputHighlighter>)
    })
}

#[derive(Default)]
struct LuauHighlighter {
    tokens: Vec<Token>,
}

impl InputHighlighter for LuauHighlighter {
    fn language(&self) -> SharedString {
        SharedString::new_static(LANGUAGE)
    }

    /// Re-lexes the whole buffer, ignoring the incremental `edit` hint: the
    /// lexer has no reusable state to patch, and re-running it costs less
    /// than the bookkeeping an incremental path would need to stay correct
    /// across an edit landing inside a long string or comment.
    fn update(
        &mut self,
        _edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        self.tokens = if text.len() > MAX_HIGHLIGHTED_BYTES {
            Vec::new()
        } else {
            luau::tokenize(&text.to_string())
        };
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        runs(&self.tokens, range)
            .into_iter()
            .map(|(span, name)| {
                let style = name
                    .and_then(|name| resolver.style(name))
                    .unwrap_or_default();
                (span, style)
            })
            .collect()
    }

    /// No code folding: it needs a parse tree's block extents, which a lexer
    /// does not have. The editor is built with folding off to match.
    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

/// Splits `range` into the ordered, non-overlapping runs that cover it
/// exactly — the contract [`InputHighlighter::styles`] states. A run carries
/// the highlight name of the token covering it, or `None` for the stretches
/// between tokens and for tokens with no colour of their own.
///
/// Separate from `styles` above, and yielding names rather than resolved
/// `HighlightStyle`s, so the clipping arithmetic can be tested without a
/// window or a theme.
fn runs(tokens: &[Token], range: &Range<usize>) -> Vec<(Range<usize>, Option<&'static str>)> {
    let mut runs = Vec::new();
    let mut at = range.start;

    for token in tokens {
        if at >= range.end || token.range.start >= range.end {
            break;
        }
        // Already behind the cursor (clipped into the previous run), or a
        // kind that takes the plain foreground and so belongs to the gap.
        if token.range.end <= at {
            continue;
        }
        let Some(name) = token.kind.highlight_name() else {
            continue;
        };

        let start = token.range.start.max(at);
        let end = token.range.end.min(range.end);
        if start > at {
            runs.push((at..start, None));
        }
        runs.push((start..end, Some(name)));
        at = end;
    }

    if at < range.end {
        runs.push((at..range.end, None));
    }
    runs
}
