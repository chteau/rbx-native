//! What scripts print, warn and fail with, in order.

use crate::printed::{self, PrintPart};

/// Which Output stream a line belongs to, as Studio colours them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Print,
    Warn,
    /// An error a script survived: one raised in a `task.spawn`ed thread,
    /// an event handler, a `task.delay` callback.
    Error,
}

/// Everything a script printed, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    lines: Vec<String>,
    printed: Vec<Vec<PrintPart>>,
    kinds: Vec<LineKind>,
}

impl Output {
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Each line's arguments, tables kept as trees, one per line of
    /// [`Output::lines`].
    pub fn printed(&self) -> &[Vec<PrintPart>] {
        &self.printed
    }

    /// Each line's stream, one per line of [`Output::lines`].
    pub fn kinds(&self) -> &[LineKind] {
        &self.kinds
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Appends `other`'s lines after these.
    pub fn extend(&mut self, other: Output) {
        self.lines.extend(other.lines);
        self.printed.extend(other.printed);
        self.kinds.extend(other.kinds);
    }

    pub(crate) fn push(&mut self, kind: LineKind, parts: Vec<PrintPart>) {
        self.lines.push(printed::text(&parts));
        self.printed.push(parts);
        self.kinds.push(kind);
    }

    pub(crate) fn push_text(&mut self, kind: LineKind, text: String) {
        self.push(kind, vec![PrintPart::Text(text)]);
    }

    /// Drains the lines, dropping their parts (the debugger's view).
    pub(crate) fn take_lines(&mut self) -> Vec<String> {
        std::mem::take(self).lines
    }
}
