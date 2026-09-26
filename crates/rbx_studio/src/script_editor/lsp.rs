//! The script editor's side of `luau-lsp`: one [`Document`] per open tab,
//! kept in step with the tab's text, and the completion and hover providers
//! GPUI Kit's editor calls into.
//!
//! The server counts columns in bytes (see `luau_lsp::start`); the editor's
//! own LSP types count them in characters. Every position crosses between
//! the two here and nowhere else.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use gpui_kit::base::input::{CompletionProvider, HoverProvider, Point, Rope, RopeExt as _};
use gpui_kit::{App, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionResponse, CompletionTextEdit, Diagnostic, Hover,
    Position, Range, TextEdit,
};
use serde_json::{json, Value};

use crate::luau_lsp::{self, Client};

/// Plenty to scroll through; the rest are one more typed letter away.
const MAX_COMPLETIONS: usize = 200;

/// An open tab as the server knows it. Closed on the server when the last
/// handle goes — the tab's and its editor's providers'.
pub(crate) struct Document {
    client: Arc<Client>,
    uri: String,
    /// The version and text last sent, so an unchanged text sends nothing.
    sent: RefCell<(i32, String)>,
}

impl Document {
    pub(crate) fn open(client: Arc<Client>, uri: String, text: String) -> Rc<Document> {
        client.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": "luau", "version": 1, "text": text}}),
        );
        Rc::new(Document {
            client,
            uri,
            sent: RefCell::new((1, text)),
        })
    }

    /// Sends the whole text if it moved since the last send. Whole rather
    /// than incremental: a script is small, and a full sync can never drift.
    pub(crate) fn sync(&self, text: &str) {
        let mut sent = self.sent.borrow_mut();
        if sent.1 == text {
            return;
        }
        sent.0 += 1;
        sent.1 = text.to_owned();
        self.client.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": self.uri, "version": sent.0},
                "contentChanges": [{"text": text}],
            }),
        );
    }

    fn at(&self, text: &Rope, offset: usize) -> Value {
        self.sync(&text.to_string());
        json!({"textDocument": {"uri": self.uri}, "position": server_position(text, offset)})
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        self.client.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": self.uri}}),
        );
    }
}

pub(crate) struct Completions(pub(crate) Rc<Document>);

impl CompletionProvider for Completions {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        let reply = self
            .0
            .client
            .request("textDocument/completion", self.0.at(text, offset));
        let source = text.to_string();
        let start = identifier_start(&source, offset);
        let prefix = source[start..offset].to_owned();
        let replace = Range {
            start: text.offset_to_position(start),
            end: text.offset_to_position(offset),
        };
        cx.background_executor().spawn(async move {
            let reply = luau_lsp::wait(reply).map_err(anyhow::Error::msg)?;
            Ok(CompletionResponse::Array(shape(reply, &prefix, replace)))
        })
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        new_text
            .chars()
            .last()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | ':'))
    }
}

pub(crate) struct Hovers(pub(crate) Rc<Document>);

impl HoverProvider for Hovers {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Option<Hover>>> {
        let reply = self
            .0
            .client
            .request("textDocument/hover", self.0.at(text, offset));
        cx.background_executor().spawn(async move {
            let reply = luau_lsp::wait(reply).map_err(anyhow::Error::msg)?;
            let hover: Option<Hover> = serde_json::from_value(reply)?;
            // Its range is in the server's columns and only narrows what the
            // popover anchors to; the hovered word does as well.
            Ok(hover.map(|hover| Hover {
                range: None,
                ..hover
            }))
        })
    }
}

/// Where the identifier being typed at `offset` starts: what the server's
/// completions are narrowed by, and what picking one replaces.
fn identifier_start(source: &str, offset: usize) -> usize {
    source[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .last()
        .map_or(offset, |(index, _)| index)
}

/// The server's list narrowed to `prefix` — the editor's menu shows what it
/// is given, unfiltered — in the server's own order, each item rewritten to
/// replace `replace` (the typed prefix, in editor positions) with its text.
fn shape(reply: Value, prefix: &str, replace: Range) -> Vec<CompletionItem> {
    let items = match serde_json::from_value(reply) {
        Ok(CompletionResponse::Array(items)) => items,
        Ok(CompletionResponse::List(list)) => list.items,
        Err(_) => Vec::new(),
    };
    let prefix = prefix.to_lowercase();
    let mut kept: Vec<CompletionItem> = items
        .into_iter()
        .filter(|item| {
            let key = item.filter_text.as_deref().unwrap_or(&item.label);
            key.to_lowercase().starts_with(&prefix)
        })
        .map(|mut item| {
            let new_text = match item.text_edit.take() {
                Some(CompletionTextEdit::Edit(edit)) => edit.new_text,
                Some(CompletionTextEdit::InsertAndReplace(edit)) => edit.new_text,
                None => item
                    .insert_text
                    .take()
                    .unwrap_or_else(|| item.label.clone()),
            };
            item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
                range: replace,
                new_text,
            }));
            item
        })
        .collect();
    kept.sort_by(|a, b| {
        let key = |item: &CompletionItem| (item.sort_text.clone(), item.label.clone());
        key(a).cmp(&key(b))
    });
    kept.truncate(MAX_COMPLETIONS);
    kept
}

fn server_position(text: &Rope, offset: usize) -> Position {
    let point = text.offset_to_point(offset);
    Position::new(point.row as u32, point.column as u32)
}

/// A server position as a byte offset into `text`, clamped into it: the
/// server may be answering about a text a keystroke older.
pub(crate) fn editor_offset(text: &Rope, position: Position) -> usize {
    let row = position.line as usize;
    if row >= text.lines_len() {
        return text.len();
    }
    let column = (position.character as usize).min(text.line_len(row));
    let offset = text.point_to_offset(Point::new(row, column));
    // Mid-character is possible against a stale text; `offset_to_point`
    // snaps back to the character's start.
    text.point_to_offset(text.offset_to_point(offset))
}

/// `problems` with their ranges moved into the editor's columns.
pub(crate) fn editor_diagnostics(text: &Rope, problems: &[Diagnostic]) -> Vec<Diagnostic> {
    let convert = |position| text.offset_to_position(editor_offset(text, position));
    problems
        .iter()
        .map(|problem| Diagnostic {
            range: Range {
                start: convert(problem.range.start),
                end: convert(problem.range.end),
            },
            ..problem.clone()
        })
        .collect()
}

#[cfg(test)]
mod tests;
