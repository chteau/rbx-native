//! Search over a script's source. [`matches`] and [`replace_all`] are the
//! plain-text pair (ASCII case-insensitive, so a match's byte length always
//! equals the query's); [`Pattern`] is Find in Place's, with Studio's match
//! case, whole word and regex options.

use std::ops::Range;

use regex::{Captures, Regex, RegexBuilder};

#[cfg(test)]
mod tests;

/// Every non-overlapping match of `query` in `text`, in order. Empty for an
/// empty query, which would otherwise match between every character.
pub(crate) fn matches(text: &str, query: &str) -> Vec<Range<usize>> {
    let (haystack, needle) = (text.as_bytes(), query.as_bytes());
    let mut found = Vec::new();
    if needle.is_empty() {
        return found;
    }
    let mut at = 0;
    while at + needle.len() <= haystack.len() {
        // Both ends on `char` boundaries: an ASCII-case-insensitive compare of
        // a query that starts or ends mid-character can't match, but a
        // multi-byte query must not be reported at a boundary it splits.
        if text.is_char_boundary(at)
            && text.is_char_boundary(at + needle.len())
            && haystack[at..at + needle.len()].eq_ignore_ascii_case(needle)
        {
            found.push(at..at + needle.len());
            at += needle.len();
        } else {
            at += 1;
        }
    }
    found
}

/// `text` with every match of `query` replaced, or `None` if nothing matched.
pub(crate) fn replace_all(text: &str, query: &str, replacement: &str) -> Option<String> {
    let found = matches(text, query);
    if found.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for range in found {
        out.push_str(&text[last..range.start]);
        out.push_str(replacement);
        last = range.end;
    }
    out.push_str(&text[last..]);
    Some(out)
}

/// Find in Place's three toggles. All off is Studio's default: plain text,
/// any case, anywhere in a word.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Options {
    pub(crate) match_case: bool,
    pub(crate) whole_word: bool,
    pub(crate) regex: bool,
}

/// A query compiled under [`Options`].
pub(crate) struct Pattern {
    regex: Regex,
    whole_word: bool,
    /// `$1`-style groups in a replacement, only meaningful for a regex query.
    groups: bool,
}

impl Pattern {
    /// The reason a regex query does not compile, in one line, for the status row.
    pub(crate) fn compile(query: &str, options: Options) -> Result<Pattern, String> {
        let source = match options.regex {
            true => query.to_owned(),
            false => regex::escape(query),
        };
        let regex = RegexBuilder::new(&source)
            .case_insensitive(!options.match_case)
            .multi_line(true)
            .build()
            .map_err(|error| {
                let text = error.to_string();
                let last = text.lines().last().unwrap_or_default();
                last.strip_prefix("error: ").unwrap_or(last).to_owned()
            })?;
        Ok(Pattern {
            regex,
            whole_word: options.whole_word,
            groups: options.regex,
        })
    }

    /// Non-empty matches only, and with whole word on only those with no
    /// identifier character on either side, as Studio's whole word is.
    fn captures<'a>(&'a self, text: &'a str) -> impl Iterator<Item = Captures<'a>> {
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        self.regex.captures_iter(text).filter(move |caps| {
            let found = caps.get(0).map(|m| m.range()).unwrap_or_default();
            !found.is_empty()
                && (!self.whole_word
                    || (!word(text[..found.start].chars().next_back())
                        && !word(text[found.end..].chars().next())))
        })
    }

    /// Every match in `text`, in order.
    pub(crate) fn matches(&self, text: &str) -> Vec<Range<usize>> {
        self.captures(text)
            .filter_map(|caps| caps.get(0).map(|m| m.range()))
            .collect()
    }

    /// What `replacement` becomes for the match at `range` (which must be
    /// one of [`Pattern::matches`]): itself, or with `$1` groups filled in.
    pub(crate) fn expansion(&self, text: &str, range: &Range<usize>, replacement: &str) -> String {
        let mut out = String::new();
        let caps = self
            .groups
            .then(|| self.regex.captures_at(text, range.start))
            .flatten()
            .filter(|caps| caps.get(0).is_some_and(|m| m.range() == *range));
        match caps {
            Some(caps) => caps.expand(replacement, &mut out),
            None => out.push_str(replacement),
        }
        out
    }

    /// `text` with every match replaced, or `None` if nothing matched.
    pub(crate) fn replace_all(&self, text: &str, replacement: &str) -> Option<String> {
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for caps in self.captures(text) {
            let Some(found) = caps.get(0) else { continue };
            out.push_str(&text[last..found.start()]);
            match self.groups {
                true => caps.expand(replacement, &mut out),
                false => out.push_str(replacement),
            }
            last = found.end();
        }
        if last == 0 && out.is_empty() {
            return None;
        }
        out.push_str(&text[last..]);
        Some(out)
    }

    /// `text` with only the match at `range` replaced.
    pub(crate) fn replace_one(
        &self,
        text: &str,
        range: &Range<usize>,
        replacement: &str,
    ) -> String {
        let mut out = String::with_capacity(text.len());
        out.push_str(&text[..range.start]);
        out.push_str(&self.expansion(text, range, replacement));
        out.push_str(&text[range.end..]);
        out
    }
}

/// The 1-based line number `offset` sits on, and that line's text with its
/// leading indentation dropped, what a result row shows.
pub(crate) fn line_at(text: &str, offset: usize) -> (usize, &str) {
    let start = text[..offset].rfind('\n').map_or(0, |at| at + 1);
    let end = text[offset..]
        .find('\n')
        .map_or(text.len(), |at| offset + at);
    let number = text[..start].matches('\n').count() + 1;
    (number, text[start..end].trim_start())
}

/// What Ctrl+D / Shift+Alt+L do to the editor's selections.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Extend {
    /// Replaces every selection first: the word under a bare cursor, which is
    /// what the first press selects when nothing is selected yet.
    pub(crate) primary: Option<Range<usize>>,
    /// Added alongside whatever the selections then are.
    pub(crate) add: Vec<Range<usize>>,
}

/// Ctrl+D, Studio's "add cursor to next matching selection". `selections`
/// is every current selection, primary first. The match is the first one
/// past the furthest selection, wrapping to the top; `None` once every
/// match is selected.
pub(crate) fn next_match(text: &str, selections: &[Range<usize>]) -> Option<Extend> {
    let (needle, fresh) = needle(text, selections)?;
    if fresh {
        return Some(Extend {
            primary: Some(needle),
            add: Vec::new(),
        });
    }
    let after = selections.iter().map(|range| range.end).max().unwrap_or(0);
    let free = unselected(text, &needle, selections, false);
    let next = free
        .iter()
        .find(|range| range.start >= after)
        .or_else(|| free.first())?;
    Some(Extend {
        primary: None,
        add: vec![next.clone()],
    })
}

/// Shift+Alt+L, "add cursor to every matching selection".
pub(crate) fn every_match(text: &str, selections: &[Range<usize>]) -> Option<Extend> {
    let (needle, fresh) = needle(text, selections)?;
    let existing = if fresh {
        std::slice::from_ref(&needle)
    } else {
        selections
    };
    let add = unselected(text, &needle, existing, fresh);
    Some(Extend {
        primary: fresh.then(|| needle.clone()),
        add,
    })
}

/// The text to match, as a range: the primary selection, or the word under
/// a bare cursor (`fresh`). `None` for a bare cursor on no word.
fn needle(text: &str, selections: &[Range<usize>]) -> Option<(Range<usize>, bool)> {
    let primary = selections.first()?.clone();
    if !primary.is_empty() {
        return Some((primary, false));
    }
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let start = text[..primary.start]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| word(c))
        .last()
        .map_or(primary.start, |(at, _)| at);
    let end = text[primary.start..]
        .char_indices()
        .find(|&(_, c)| !word(c))
        .map_or(text.len(), |(at, _)| primary.start + at);
    (start < end).then_some((start..end, true))
}

/// Exact-case matches of `needle`'s text that overlap no selection. A needle
/// taken from a bare cursor matches whole words only, so Ctrl+D on `count`
/// never picks up `counter`: the same rule VS Code and Studio follow.
fn unselected(
    text: &str,
    needle: &Range<usize>,
    selections: &[Range<usize>],
    whole_word: bool,
) -> Vec<Range<usize>> {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    text.match_indices(&text[needle.clone()])
        .map(|(start, found)| start..start + found.len())
        .filter(|range| {
            !whole_word
                || (!word(text[..range.start].chars().next_back())
                    && !word(text[range.end..].chars().next()))
        })
        .filter(|range| {
            !selections
                .iter()
                .any(|sel| range.start < sel.end.max(sel.start + 1) && sel.start < range.end)
        })
        .collect()
}
