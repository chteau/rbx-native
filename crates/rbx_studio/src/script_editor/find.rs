//! Plain-text search over a script's source, for Find All / Replace All
//! across every open tab. Case-insensitive, as Studio's own Find All is by
//! default; ASCII case only, so a match's byte length always equals the
//! query's and a replacement lands exactly where the match was.

use std::ops::Range;

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

/// The 1-based line number `offset` sits on, and that line's text with its
/// leading indentation dropped — what a result row shows.
pub(crate) fn line_at(text: &str, offset: usize) -> (usize, &str) {
    let start = text[..offset].rfind('\n').map_or(0, |at| at + 1);
    let end = text[offset..]
        .find('\n')
        .map_or(text.len(), |at| offset + at);
    let number = text[..start].matches('\n').count() + 1;
    (number, text[start..end].trim_start())
}
