//! Indented XML text builder shared by every encoder in `serializer::value`.
//!
//! Deliberately dumb: it only tracks indentation depth and escapes text/attribute
//! values. Structure (which tags nest inside which) is entirely the caller's job.

const INDENT: &str = "  ";

pub(crate) struct Writer {
    out: String,
    depth: usize,
}

impl Writer {
    pub(crate) fn new() -> Self {
        Writer {
            out: String::new(),
            depth: 0,
        }
    }

    pub(crate) fn into_string(self) -> String {
        self.out
    }

    /// Opens a tag that will contain child elements, and indents further writes.
    pub(crate) fn open(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.indent();
        self.push_start_tag(tag, attrs);
        self.out.push('\n');
        self.depth += 1;
    }

    /// Closes a tag opened with [`Self::open`].
    pub(crate) fn close(&mut self, tag: &str) {
        self.depth -= 1;
        self.indent();
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push_str(">\n");
    }

    /// Writes a leaf element with `text` as its entire content, on one line.
    ///
    /// Never adds whitespace inside the tag: some reader decoders (plain `string`
    /// properties) use the element's raw text verbatim, without trimming, so any
    /// padding here would corrupt the round-tripped value.
    pub(crate) fn leaf(&mut self, tag: &str, attrs: &[(&str, &str)], text: &str) {
        self.indent();
        self.push_start_tag(tag, attrs);
        escape_into(&mut self.out, text);
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push_str(">\n");
    }

    fn push_start_tag(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.out.push('<');
        self.out.push_str(tag);
        for (key, value) in attrs {
            self.out.push(' ');
            self.out.push_str(key);
            self.out.push_str("=\"");
            escape_into(&mut self.out, value);
            self.out.push('"');
        }
        self.out.push('>');
    }

    fn indent(&mut self) {
        for _ in 0..self.depth {
            self.out.push_str(INDENT);
        }
    }
}

// Escaping all four characters (rather than only the three XML strictly requires in
// text content) keeps this one function correct for both text and attribute values.
fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_escapes_reserved_characters() {
        let mut writer = Writer::new();
        writer.leaf("string", &[("name", "X")], "<a> & \"b\"");
        assert_eq!(
            writer.into_string(),
            "<string name=\"X\">&lt;a&gt; &amp; &quot;b&quot;</string>\n"
        );
    }

    #[test]
    fn open_close_nests_and_indents() {
        let mut writer = Writer::new();
        writer.open("Item", &[("class", "Part")]);
        writer.leaf("string", &[("name", "Name")], "Baseplate");
        writer.close("Item");
        assert_eq!(
            writer.into_string(),
            "<Item class=\"Part\">\n  <string name=\"Name\">Baseplate</string>\n</Item>\n"
        );
    }
}
