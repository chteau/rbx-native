//! Find in Place's search and replace, as pure functions over a `WeakDom`:
//! what matches in which scripts, and the new `Source` each would get.

use std::collections::HashMap;
use std::ops::Range;

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::script_editor::find::Pattern;
use crate::script_editor::source;
use crate::shell::script_analysis::full_name;

/// Past this many rows the list stops growing: a one-letter search over a
/// big place is thousands of rows nobody scrolls through, and each is a
/// laid-out element. The status line still counts them all.
pub(super) const MAX_HITS: usize = 500;

/// How much of a line is kept either side of a match, so a minified script
/// does not become a megabyte row.
const BEFORE: usize = 40;
const AFTER: usize = 200;

/// One match, as a row shows it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Hit {
    /// Into the script's whole `Source`.
    pub(super) range: Range<usize>,
    /// 1-based.
    pub(super) line: usize,
    /// The line up to the match (indentation dropped), the match, and the
    /// rest of the line. A match over several lines shows its first.
    pub(super) before: String,
    pub(super) text: String,
    pub(super) after: String,
    /// What the match becomes, when a replacement is being previewed.
    pub(super) replacement: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct File {
    pub(super) script: Ref,
    pub(super) class: String,
    pub(super) name: String,
    pub(super) count: usize,
    /// Fewer than `count` once the list hits [`MAX_HITS`].
    pub(super) hits: Vec<Hit>,
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Results {
    pub(super) files: Vec<File>,
    pub(super) total: usize,
    /// A regex that does not compile, shown instead of results.
    pub(super) error: Option<String>,
}

/// What a scan reads: the place, plus the text of open tabs holding typing
/// that has not reached `Source` yet.
pub(super) struct Place<'a> {
    pub(super) dom: &'a WeakDom,
    pub(super) database: &'a ReflectionDatabase,
    pub(super) typing: &'a HashMap<Ref, String>,
}

impl Place<'_> {
    /// Every script whose full name contains `filter` (any case), with its
    /// text, in tree order.
    fn scripts(&self, filter: &str) -> Vec<(Ref, String, String)> {
        let filter = filter.trim().to_lowercase();
        let mut found = Vec::new();
        let mut stack: Vec<Ref> = self.dom.root_refs().iter().rev().copied().collect();
        while let Some(reference) = stack.pop() {
            let Some(instance) = self.dom.get(reference) else {
                continue;
            };
            stack.extend(instance.children().iter().rev());
            if !source::is_script_class(self.database, instance.class()) {
                continue;
            }
            let name = full_name(self.dom, reference);
            if !name.to_lowercase().contains(&filter) {
                continue;
            }
            let text = self
                .typing
                .get(&reference)
                .cloned()
                .or_else(|| source::read(self.dom, reference))
                .unwrap_or_default();
            found.push((reference, name, text));
        }
        found
    }
}

/// Every match of `pattern` in the scripts `filter` keeps. `replacement`
/// previews what each match becomes.
pub(super) fn scan(
    place: &Place,
    pattern: &Pattern,
    filter: &str,
    replacement: Option<&str>,
) -> Results {
    let mut results = Results::default();
    let mut shown = 0;
    for (script, name, text) in place.scripts(filter) {
        let ranges = pattern.matches(&text);
        if ranges.is_empty() {
            continue;
        }
        let room = MAX_HITS.saturating_sub(shown);
        let hits: Vec<Hit> = ranges
            .iter()
            .take(room)
            .map(|range| {
                let new = replacement.map(|r| pattern.expansion(&text, range, r));
                hit(&text, range.clone(), new)
            })
            .collect();
        shown += hits.len();
        results.total += ranges.len();
        results.files.push(File {
            script,
            class: place
                .dom
                .get(script)
                .map(|i| i.class().to_string())
                .unwrap_or_default(),
            name,
            count: ranges.len(),
            hits,
        });
    }
    results
}

fn hit(text: &str, range: Range<usize>, replacement: Option<String>) -> Hit {
    let start = text[..range.start].rfind('\n').map_or(0, |at| at + 1);
    let line = text[..start].matches('\n').count() + 1;
    let found = &text[range.clone()];
    let (shown, tail) = match found.find('\n') {
        Some(at) => (&found[..at], ""),
        None => {
            let rest = &text[range.end..];
            (found, rest.split('\n').next().unwrap_or_default())
        }
    };
    Hit {
        line,
        before: tail_chars(text[start..range.start].trim_start(), BEFORE),
        text: shown.trim_end_matches('\r').to_owned(),
        after: tail.trim_end_matches('\r').chars().take(AFTER).collect(),
        replacement,
        range,
    }
}

/// The last `count` characters of `text`, led by an ellipsis if cut.
fn tail_chars(text: &str, count: usize) -> String {
    let total = text.chars().count();
    match total > count {
        true => format!(
            "\u{2026}{}",
            text.chars().skip(total - count).collect::<String>()
        ),
        false => text.to_owned(),
    }
}

/// The new `Source` of every script `pattern` changes, replacing every
/// match.
pub(super) fn replacements(
    place: &Place,
    pattern: &Pattern,
    filter: &str,
    replacement: &str,
) -> Vec<(Ref, String)> {
    place
        .scripts(filter)
        .into_iter()
        .filter_map(|(script, _, text)| Some((script, pattern.replace_all(&text, replacement)?)))
        .collect()
}

/// Writes `edits` as the scripts' `Source`, reporting whether any changed.
/// The caller wraps this in one history step.
pub(super) fn write_all(dom: &mut WeakDom, edits: &[(Ref, String)]) -> bool {
    edits.iter().fold(false, |any, (script, text)| {
        source::write(dom, *script, text) | any
    })
}

#[cfg(test)]
mod tests;
