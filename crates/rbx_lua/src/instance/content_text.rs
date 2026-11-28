//! `ContentText`: a text object's `Text` as it shows, rich text markup
//! taken out (read only, never stored).

use rbx_dom::{Ref, Variant, WeakDom};

/// `referent`'s `ContentText`, `None` for a class without one.
pub(super) fn of(dom: &WeakDom, referent: Ref) -> Option<String> {
    let instance = dom.get(referent)?;
    if !matches!(instance.class(), "TextLabel" | "TextButton" | "TextBox") {
        return None;
    }
    let properties = instance.properties();
    let text = match properties.get("Text") {
        Some(Variant::String(text)) => text.as_str(),
        _ => "",
    };
    Some(match properties.get("RichText") {
        Some(Variant::Bool(true)) => plain(text),
        _ => text.to_owned(),
    })
}

/// `text` without its `<tags>`, its escapes read back.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        match rest[at..].find('>') {
            Some(end) => rest = &rest[at + end + 1..],
            None => {
                rest = &rest[at..];
                break;
            }
        }
    }
    out.push_str(rest);
    [
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&apos;", "'"),
        ("&amp;", "&"),
    ]
    .iter()
    .fold(out, |out, (from, to)| out.replace(from, to))
}

#[cfg(test)]
mod tests {
    #[test]
    fn markup_is_taken_out() {
        assert_eq!(
            super::plain("<b>bold</b> &amp; <font color=\"#f00\">red</font> 1 &lt; 2"),
            "bold & red 1 < 2"
        );
    }
}
