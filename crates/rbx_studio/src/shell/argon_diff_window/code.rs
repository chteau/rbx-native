//! The code card: a script's unified diff (context, added and removed
//! rows with both line numbers, hunk rows that expand what they hide) or
//! its whole source (an added or removed script), virtualised with
//! gpui's variable-height `list()` — code rows are 20, hunk and tail rows
//! 28 — and cut off at Diff Lines Limit.

use std::collections::HashSet;
use std::ops::Range;

use gpui_kit::*;

use crate::script_editor::{highlight, luau};

use super::diff::{self, LineDiff, Op};

pub(super) const CODE_ROW: f32 = 20.;
pub(super) const HUNK_ROW: f32 = 28.;
/// A line number cell, and the narrow layout's.
pub(super) const GUTTER: f32 = 44.;
pub(super) const GUTTER_NARROW: f32 = 36.;
pub(super) const MARKER: f32 = 20.;
/// JetBrains Mono at 12 px advances this much per column.
pub(super) const COLUMN: f32 = 7.2;

/// A line's byte runs and their highlight names.
pub(super) type Runs = Vec<(Range<usize>, Option<&'static str>)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LineKind {
    Context,
    Added,
    Removed,
}

/// One row of the card.
#[derive(Debug, Clone)]
pub(super) enum CodeRow {
    Line {
        old: Option<usize>,
        new: Option<usize>,
        kind: LineKind,
        text: SharedString,
        /// Byte runs of `text` and their highlight names.
        runs: Runs,
    },
    /// `@@ −a,b +c,d @@` and the enclosing function; `hidden` equal
    /// lines sit before it, expandable by `key`.
    Hunk {
        key: usize,
        hidden: usize,
        header: String,
        function: Option<String>,
    },
    Tail {
        key: usize,
        hidden: usize,
    },
    Limit {
        more: usize,
        limit: usize,
    },
}

/// A source with tabs laid out on 4 columns, lexed once.
pub(super) struct Source {
    lines: Vec<String>,
    /// Each line's byte range in the joined text the tokens index.
    ranges: Vec<Range<usize>>,
    tokens: Vec<luau::Token>,
}

impl Source {
    pub(super) fn new(text: &str) -> Self {
        let lines: Vec<String> = text.lines().map(expand_tabs).collect();
        let mut ranges = Vec::with_capacity(lines.len());
        let mut at = 0;
        for line in &lines {
            ranges.push(at..at + line.len());
            at += line.len() + 1;
        }
        let joined = lines.join("\n");
        Source {
            lines,
            ranges,
            tokens: luau::tokenize(&joined),
        }
    }

    pub(super) fn line_count(&self) -> usize {
        self.lines.len()
    }

    fn line(&self, index: usize) -> (SharedString, Runs) {
        let range = &self.ranges[index];
        let runs = highlight::runs(&self.tokens, range)
            .into_iter()
            .map(|(span, name)| (span.start - range.start..span.end - range.start, name))
            .collect();
        (SharedString::from(self.lines[index].clone()), runs)
    }

    /// The nearest line at or above `index` (zero-based) that opens a
    /// function, trimmed.
    fn enclosing_function(&self, index: usize) -> Option<String> {
        self.lines[..index.min(self.lines.len())]
            .iter()
            .rev()
            .find(|line| is_function_line(line))
            .map(|line| line.trim().to_owned())
    }
}

fn is_function_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("local ")
        .map(str::trim_start)
        .unwrap_or(trimmed);
    rest.starts_with("function")
        && rest[8..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_')
}

fn expand_tabs(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut column = 0;
    for c in line.chars() {
        if c == '\t' {
            let pad = 4 - column % 4;
            out.extend(std::iter::repeat_n(' ', pad));
            column += pad;
        } else {
            out.push(c);
            column += 1;
        }
    }
    out
}

/// The unified rows of an update, `expanded` hunks shown in full.
pub(super) fn unified_rows(
    old: &Source,
    new: &Source,
    diff: &LineDiff,
    expanded: &HashSet<usize>,
) -> Vec<CodeRow> {
    let (hunks, tail) = diff::hunks(diff, 3);
    let mut rows = Vec::new();
    for (key, hunk) in hunks.iter().enumerate() {
        if !hunk.hidden_before.is_empty() {
            if expanded.contains(&key) {
                rows.extend(hunk.hidden_before.iter().map(|op| op_row(op, old, new)));
            } else {
                rows.push(CodeRow::Hunk {
                    key,
                    hidden: hunk.hidden_before.len(),
                    header: format!(
                        "@@ \u{2212}{},{} +{},{} @@",
                        hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
                    ),
                    function: old.enclosing_function(hunk.old_start.saturating_sub(1)),
                });
            }
        } else {
            rows.push(CodeRow::Hunk {
                key,
                hidden: 0,
                header: format!(
                    "@@ \u{2212}{},{} +{},{} @@",
                    hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
                ),
                function: old.enclosing_function(hunk.old_start.saturating_sub(1)),
            });
        }
        rows.extend(hunk.ops.iter().map(|op| op_row(op, old, new)));
    }
    if !tail.is_empty() {
        let key = hunks.len();
        if expanded.contains(&key) {
            rows.extend(tail.iter().map(|op| op_row(op, old, new)));
        } else {
            rows.push(CodeRow::Tail {
                key,
                hidden: tail.len(),
            });
        }
    }
    rows
}

fn op_row(op: &Op, old: &Source, new: &Source) -> CodeRow {
    match *op {
        Op::Equal(i, j) => {
            let (text, runs) = new.line(j);
            CodeRow::Line {
                old: Some(i + 1),
                new: Some(j + 1),
                kind: LineKind::Context,
                text,
                runs,
            }
        }
        Op::Delete(i) => {
            let (text, runs) = old.line(i);
            CodeRow::Line {
                old: Some(i + 1),
                new: None,
                kind: LineKind::Removed,
                text,
                runs,
            }
        }
        Op::Insert(j) => {
            let (text, runs) = new.line(j);
            CodeRow::Line {
                old: None,
                new: Some(j + 1),
                kind: LineKind::Added,
                text,
                runs,
            }
        }
    }
}

/// The plain rows of an added or removed script: one gutter, no markers.
pub(super) fn plain_rows(source: &Source) -> Vec<CodeRow> {
    (0..source.line_count())
        .map(|index| {
            let (text, runs) = source.line(index);
            CodeRow::Line {
                old: Some(index + 1),
                new: None,
                kind: LineKind::Context,
                text,
                runs,
            }
        })
        .collect()
}

/// Diff Lines Limit: at most `limit` code rows, then one row saying how
/// many more there were.
pub(super) fn cap(rows: Vec<CodeRow>, limit: usize) -> Vec<CodeRow> {
    let total = rows
        .iter()
        .filter(|row| matches!(row, CodeRow::Line { .. }))
        .count();
    if total <= limit {
        return rows;
    }
    let mut shown = 0;
    let mut out = Vec::new();
    for row in rows {
        if matches!(row, CodeRow::Line { .. }) {
            if shown == limit {
                break;
            }
            shown += 1;
        }
        out.push(row);
    }
    out.push(CodeRow::Limit {
        more: total - limit,
        limit,
    });
    out
}

/// The widest code column, for the card's horizontal extent.
pub(super) fn widest(rows: &[CodeRow]) -> usize {
    rows.iter()
        .map(|row| match row {
            CodeRow::Line { text, .. } => text.chars().count(),
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

mod render;

#[cfg(test)]
mod tests;
