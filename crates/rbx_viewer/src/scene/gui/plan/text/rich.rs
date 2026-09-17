//! `RichText` markup (Roblox's `ui/rich-text.md`) parsed into styled spans.
//!
//! Honoured: `<b>`, `<i>`, `<u>`, `<s>`, `<br/>`, `<font color size face
//! family weight transparency>`, `<!-- -->` comments and the five escape
//! forms. Every other tag (`<stroke>`, `<mark>`, `<uppercase>`, `<smallcaps>`
//! and anything unknown) is stripped and its content kept as plain text: a
//! tag never reaches the screen as literal characters, which is the one thing
//! Roblox and this agree on for markup neither supports the same way.

use rbx_assets::AssetRef;

use super::Span;
use crate::scene::srgb_to_linear;

/// The style in force at one point of the string: everything a [`Span`]
/// carries but its text.
#[derive(Clone, Default)]
struct Style {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    color: Option<[f32; 3]>,
    alpha: Option<f32>,
    size: Option<f32>,
    family: Option<AssetRef>,
    weight: Option<u16>,
}

impl Style {
    fn span(&self, text: String) -> Span {
        Span {
            text,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            strike: self.strike,
            color: self.color,
            alpha: self.alpha,
            size: self.size,
            family: self.family.clone(),
            weight: self.weight,
        }
    }
}

pub(super) fn parse(markup: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut text = String::new();
    // Each opening tag pushes the style it replaces, so its closing tag
    // restores exactly that, however the tags nest. Unknown tags push too, so
    // their closing tag pops the right one.
    let mut stack: Vec<Style> = Vec::new();
    let mut style = Style::default();
    let mut rest = markup;

    while let Some(open) = rest.find('<') {
        let (before, tail) = rest.split_at(open);
        text.push_str(&unescape(before));
        if let Some(comment) = tail.strip_prefix("<!--") {
            rest = match comment.find("-->") {
                Some(end) => &comment[end + 3..],
                None => "",
            };
            continue;
        }
        let Some(close) = tail.find('>') else {
            // An unterminated tag: Roblox shows nothing for it either, and
            // there is no more markup to find.
            rest = "";
            break;
        };
        let tag = &tail[1..close];
        rest = &tail[close + 1..];

        if tag.trim_end_matches('/').trim() == "br" {
            text.push('\n');
            continue;
        }
        if tag.starts_with('/') {
            if let Some(outer) = stack.pop() {
                flush(&mut spans, &style, &mut text);
                style = outer;
            }
            continue;
        }
        flush(&mut spans, &style, &mut text);
        stack.push(style.clone());
        apply(&mut style, tag);
    }
    text.push_str(&unescape(rest));
    flush(&mut spans, &style, &mut text);
    spans
}

/// Ends the current run. A tag that changed nothing (`<stroke>`, a comment's
/// neighbours) does not split a span: the text is appended to the last one
/// where the style is the same.
fn flush(spans: &mut Vec<Span>, style: &Style, text: &mut String) {
    if text.is_empty() {
        return;
    }
    let span = style.span(std::mem::take(text));
    match spans.last_mut() {
        Some(last) if last.same_style(&span) => last.text.push_str(&span.text),
        _ => spans.push(span),
    }
}

/// Applies one opening tag's meaning; a tag this does not know changes
/// nothing, so its content comes through in the enclosing style.
fn apply(style: &mut Style, tag: &str) {
    let mut words = tag.split_whitespace();
    match words.next().unwrap_or("") {
        "b" => style.bold = true,
        "i" => style.italic = true,
        "u" => style.underline = true,
        "s" => style.strike = true,
        "font" => {
            for (name, value) in attributes(tag) {
                match name {
                    "color" => style.color = color(value),
                    "size" => style.size = value.parse().ok().filter(|size: &f32| *size > 0.0),
                    "face" => {
                        style.family =
                            Some(AssetRef::Native(format!("fonts/families/{value}.json")));
                    }
                    "family" => style.family = AssetRef::parse(value).ok(),
                    "weight" => style.weight = weight(value),
                    "transparency" => {
                        style.alpha = value
                            .parse()
                            .ok()
                            .map(|transparency: f32| 1.0 - transparency.clamp(0.0, 1.0));
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// `name="value"` pairs after the tag name; either quote, or none.
fn attributes(tag: &str) -> Vec<(&str, &str)> {
    let mut pairs = Vec::new();
    let mut rest = tag;
    while let Some(equals) = rest.find('=') {
        let name = rest[..equals].split_whitespace().last().unwrap_or("");
        let value = rest[equals + 1..].trim_start();
        let (value, after) = match value.chars().next() {
            Some(quote @ ('"' | '\'')) => {
                let inner = &value[1..];
                match inner.find(quote) {
                    Some(end) => (&inner[..end], &inner[end + 1..]),
                    None => (inner, ""),
                }
            }
            _ => value.split_at(value.find(char::is_whitespace).unwrap_or(value.len())),
        };
        pairs.push((name, value));
        rest = after;
    }
    pairs
}

/// `#rrggbb` or `rgb(r,g,b)`, linearised.
fn color(value: &str) -> Option<[f32; 3]> {
    let value = value.trim();
    let raw = if let Some(hex) = value.strip_prefix('#') {
        if hex.len() != 6 {
            return None;
        }
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
        [channel(0)?, channel(2)?, channel(4)?]
    } else {
        let inner = value
            .strip_prefix("rgb(")
            .and_then(|rest| rest.strip_suffix(')'))?;
        let mut channels = inner
            .split(',')
            .map(|channel| channel.trim().parse::<u8>().ok());
        [channels.next()??, channels.next()??, channels.next()??]
    };
    Some(raw.map(|channel| srgb_to_linear(f32::from(channel) / 255.0)))
}

/// A weight by name (case-insensitive, per the docs) or as a number.
fn weight(value: &str) -> Option<u16> {
    let named = match value.trim().to_ascii_lowercase().as_str() {
        "thin" => 100,
        "extralight" => 200,
        "light" => 300,
        "regular" | "normal" => 400,
        "medium" => 500,
        "semibold" => 600,
        "bold" => 700,
        "extrabold" => 800,
        "heavy" => 900,
        number => {
            return number
                .parse::<u16>()
                .ok()
                .filter(|weight| (100..=900).contains(weight))
        }
    };
    Some(named)
}

fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(spans: &[Span]) -> Vec<&str> {
        spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn plain_markup_is_one_span_and_tags_split_it() {
        assert_eq!(parse("hello"), vec![Span::plain("hello")]);

        let spans = parse("Use a <b>bold title</b> here");
        assert_eq!(texts(&spans), ["Use a ", "bold title", " here"]);
        assert!(!spans[0].bold && spans[1].bold && !spans[2].bold);
    }

    #[test]
    fn nested_tags_restore_the_enclosing_style_when_closed() {
        let spans = parse("<b><i><u>all</u> bi</i></b> none");

        assert_eq!(texts(&spans), ["all", " bi", " none"]);
        assert!(spans[0].bold && spans[0].italic && spans[0].underline);
        assert!(spans[1].bold && spans[1].italic && !spans[1].underline);
        assert!(!spans[2].bold && !spans[2].italic);
    }

    #[test]
    fn font_attributes_are_read_in_either_form() {
        let spans = parse(
            r##"<font color="#FF7800" size="40" weight="Bold">a</font><font color='rgb(0,255,0)' face="Michroma" weight="900" transparency="0.25">b</font>"##,
        );

        assert_eq!(
            spans[0].color,
            Some([1.0, srgb_to_linear(120.0 / 255.0), 0.0])
        );
        assert_eq!(spans[0].size, Some(40.0));
        assert_eq!(spans[0].weight, Some(700));
        assert_eq!(spans[1].color, Some([0.0, 1.0, 0.0]));
        assert_eq!(
            spans[1].family,
            Some(AssetRef::Native("fonts/families/Michroma.json".to_string()))
        );
        assert_eq!(spans[1].weight, Some(900));
        assert_eq!(spans[1].alpha, Some(0.75));
    }

    #[test]
    fn breaks_comments_escapes_and_unknown_tags_never_show_as_markup() {
        let spans = parse(
            "one<br/>two<!-- hidden --> <stroke color=\"#fff\">three</stroke> 1 &lt; 2 &amp;&amp; <s>x</s>",
        );

        assert_eq!(texts(&spans), ["one\ntwo three 1 < 2 && ", "x"]);
        assert!(spans[1].strike);
    }

    #[test]
    fn malformed_markup_degrades_to_text_not_tags() {
        assert_eq!(texts(&parse("a </b> b")), ["a  b"]);
        assert_eq!(texts(&parse("a <b unterminated")), ["a "]);
        assert!(parse("").is_empty());
    }

    #[test]
    fn weights_and_colours_that_do_not_parse_are_ignored() {
        assert_eq!(weight("heavy"), Some(900));
        assert_eq!(weight("1000"), None);
        assert_eq!(weight("purple"), None);
        assert_eq!(color("#12"), None);
        assert_eq!(color("rgb(1,2)"), None);
    }
}
