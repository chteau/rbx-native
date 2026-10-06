//! Type-ahead: jumping to a row by typing the start of its name.

use std::time::{Duration, Instant};

use gpui_kit::Keystroke;

/// How long a type-ahead buffer survives without another keystroke.
///
/// The APG recommends type-ahead for any tree with more than about seven
/// root nodes, which every real place file has. This is the usual desktop
/// value: long enough to type "Spawn" without rushing, short enough that
/// coming back a moment later starts a fresh search.
const TYPEAHEAD_TIMEOUT: Duration = Duration::from_millis(1000);

/// A printable character that should extend the type-ahead buffer.
///
/// One character, no modifiers: anything else is a command, and a tree that
/// swallowed Ctrl+C to search for "c" would be worse than one with no
/// type-ahead at all.
pub(super) fn typeahead_char(keystroke: &Keystroke) -> Option<char> {
    if keystroke.modifiers.control || keystroke.modifiers.alt || keystroke.modifiers.platform {
        return None;
    }
    let text = keystroke.key_char.as_deref().unwrap_or(&keystroke.key);
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_control() && c != ' ' => Some(c),
        _ => None,
    }
}

/// The type-ahead buffer: what has been typed, and when.
#[derive(Debug, Default)]
pub(in crate::shell) struct Typeahead {
    query: String,
    last: Option<Instant>,
}

impl Typeahead {
    /// Adds `c`, starting a fresh query if the last keystroke has gone
    /// stale, and returns the string to match rows against.
    pub(in crate::shell) fn push(&mut self, c: char, now: Instant) -> &str {
        let stale = self
            .last
            .is_none_or(|last| now.duration_since(last) > TYPEAHEAD_TIMEOUT);
        if stale {
            self.query.clear();
        }
        self.last = Some(now);
        self.query.extend(c.to_lowercase());
        &self.query
    }
}

/// Where a type-ahead search lands, searching forward from `from` and
/// wrapping once.
///
/// Wrapping is right here where it is wrong for the arrows: type-ahead is a
/// search, and a search that stops at the bottom of the list has simply
/// failed to find something that is sitting above it.
pub(super) fn typeahead_target(labels: &[String], from: usize, query: &str) -> Option<usize> {
    if query.is_empty() || labels.is_empty() {
        return None;
    }

    // A repeated single character cycles through the matches rather than
    // sticking on the first one, which is what makes "p p p" walk the Parts.
    let start = if query.chars().count() == 1 {
        from + 1
    } else {
        from
    };

    (0..labels.len())
        .map(|offset| (start + offset) % labels.len())
        .find(|&index| labels[index].to_lowercase().starts_with(query))
}
