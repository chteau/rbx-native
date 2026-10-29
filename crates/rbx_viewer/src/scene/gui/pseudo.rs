//! Pseudo-localization: every string a text object draws, made longer and
//! accented the way a translation tends to come back, so a layout that only
//! fits its English text shows it before a translator ever sees the game.
//!
//! It is display-only. The planned [`Screen`] is copied and its copy's spans
//! rewritten, so nothing here can reach the DOM, a save, or the undo stack.
//! Spans already have their `RichText` tags parsed away (see `plan::text`),
//! so a tag is never touched; format specifiers (`%d`, `%.2f`) and
//! placeholders (`{name}`, `{1}`) are kept whole too, since a translation
//! has to keep them.

use super::plan::{Align, Group, Node, Screen, TextSpan};

/// How the preview rewrites text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PseudoLocale {
    /// Right to left as well: each string reversed and its alignment
    /// mirrored, how an Arabic or Hebrew translation sits in the box.
    pub mirror: bool,
}

/// What a string grows by, before the brackets' own fixed four characters
/// (`"[! "` and `" !]"` less the `!`s): about what German or Finnish adds.
const GROWTH: f32 = 0.4;
const OPEN: &str = "[!";
const CLOSE: &str = "!]";

/// The accented look-alike of an ASCII letter. Only Latin-1 and Latin
/// Extended-A forms (and `ƒ`), which every Roblox face carries; a letter with
/// no such form stays as it is.
fn accented(c: char) -> char {
    const FROM: &str = "ACDEGHIJKLNOPRSTUWYZacdeghijklnoprstuwyzf";
    const TO: &str = "ÅÇÐÉĜĤÎĴĶĻÑÖÞŔŠŢÛŴÝŽåçðéĝĥîĵķļñöþŕšţûŵýžƒ";
    FROM.chars()
        .position(|from| from == c)
        .and_then(|at| TO.chars().nth(at))
        .unwrap_or(c)
}

/// Splits `text` into runs a translation keeps as they are (`true`): a
/// `RichText` tag, an entity, a `printf` specifier, a `{placeholder}`; and
/// runs of plain text (`false`).
fn tokens(text: &str) -> Vec<(&str, bool)> {
    let mut out = Vec::new();
    let mut plain = 0;
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let kept = match rest.as_bytes()[0] {
            b'<' => rest.find('>').map(|end| end + 1),
            b'{' => rest
                .find('}')
                .filter(|&end| !rest[1..end].contains('{'))
                .map(|end| end + 1),
            b'&' => rest
                .find(';')
                .filter(|&end| {
                    end > 1
                        && rest[1..end]
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'#')
                })
                .map(|end| end + 1),
            b'%' => specifier(rest),
            _ => None,
        };
        match kept {
            Some(length) => {
                if plain < at {
                    out.push((&text[plain..at], false));
                }
                out.push((&rest[..length], true));
                at += length;
                plain = at;
            }
            None => at += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    if plain < text.len() {
        out.push((&text[plain..], false));
    }
    out
}

/// The length of the `printf` specifier `text` starts with, `%%` included.
fn specifier(text: &str) -> Option<usize> {
    let body = text[1..]
        .find(|c: char| !(c.is_ascii_digit() || "-+ #.".contains(c)))
        .map(|end| end + 1)?;
    text[body..]
        .chars()
        .next()
        .filter(|c| c.is_ascii_alphabetic() || *c == '%')
        .map(|c| body + c.len_utf8())
}

/// `text` with every plain letter accented, kept runs untouched.
fn accent(text: &str) -> String {
    tokens(text)
        .into_iter()
        .map(|(run, kept)| match kept {
            true => run.to_string(),
            false => run.chars().map(accented).collect(),
        })
        .collect()
}

/// How many characters of `text` a translator would actually translate.
fn plain_length(text: &str) -> usize {
    tokens(text)
        .into_iter()
        .filter(|(_, kept)| !kept)
        .map(|(run, _)| run.chars().count())
        .sum()
}

/// The two ends that pad `length` plain characters out by [`GROWTH`]: never
/// fewer than one `!` a side, so even a one-word button grows.
fn padding(length: usize) -> (String, String) {
    let growth = (length as f32 * GROWTH).ceil() as usize;
    let bangs = growth.saturating_sub(4).div_ceil(2).max(1);
    let bangs = "!".repeat(bangs - 1);
    (format!("{OPEN}{bangs} "), format!(" {bangs}{CLOSE}"))
}

fn already(text: &str) -> bool {
    text.starts_with(OPEN) && text.ends_with(CLOSE)
}

/// One whole string pseudo-localized: accented and bracketed. Idempotent;
/// blank text comes back as it is.
pub fn pseudo_localize(text: &str) -> String {
    if text.trim().is_empty() || already(text) {
        return text.to_string();
    }
    let (open, close) = padding(plain_length(text));
    format!("{open}{}{close}", accent(text))
}

/// `text` read right to left: plain characters reversed and bracket pairs
/// turned round, each kept run moved whole.
fn mirrored(text: &str) -> String {
    let flip = |c: char| match c {
        '[' => ']',
        ']' => '[',
        '(' => ')',
        ')' => '(',
        other => other,
    };
    tokens(text)
        .into_iter()
        .rev()
        .map(|(run, kept)| match kept {
            true => run.to_string(),
            false => run.chars().rev().map(flip).collect(),
        })
        .collect()
}

/// A text's spans pseudo-localized as one string: each accented, the whole
/// bracketed across the first and last, all of it reversed when mirrored.
pub(in crate::scene::gui) fn localize_spans(spans: &mut [TextSpan], locale: PseudoLocale) {
    let whole: String = spans.iter().map(|span| span.text.as_str()).collect();
    if whole.trim().is_empty() || already(&whole) {
        return;
    }
    let (open, close) = padding(plain_length(&whole));
    for span in spans.iter_mut() {
        span.text = accent(&span.text);
    }
    if let Some(first) = spans.first_mut() {
        first.text.insert_str(0, &open);
    }
    if let Some(last) = spans.last_mut() {
        last.text.push_str(&close);
    }
    if locale.mirror {
        spans.reverse();
        for span in spans.iter_mut() {
            span.text = mirrored(&span.text);
        }
    }
}

impl Screen {
    /// A copy of this tree with every text `AutoLocalize` leaves on
    /// pseudo-localized.
    pub(crate) fn pseudo_localized(&self, locale: PseudoLocale) -> Screen {
        let mut screen = self.clone();
        if screen.auto_localize {
            for node in &mut screen.roots {
                localize_node(node, locale);
            }
            for group in &mut screen.groups {
                localize_group(group, locale);
            }
        }
        screen
    }
}

fn localize_node(node: &mut Node, locale: PseudoLocale) {
    if !node.auto_localize {
        return;
    }
    if let Some(text) = &mut node.text {
        localize_spans(&mut text.spans, locale);
        if locale.mirror {
            text.x_align = match text.x_align {
                Align::Start => Align::End,
                Align::End => Align::Start,
                Align::Center => Align::Center,
            };
        }
    }
    for child in &mut node.children {
        localize_node(child, locale);
    }
    for group in &mut node.groups {
        localize_group(group, locale);
    }
}

fn localize_group(group: &mut Group, locale: PseudoLocale) {
    for child in &mut group.children {
        localize_node(child, locale);
    }
    for inner in &mut group.groups {
        localize_group(inner, locale);
    }
}
