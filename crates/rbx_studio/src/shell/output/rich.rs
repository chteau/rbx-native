//! Output's opt-in colour (Settings › Explorer & Output › Rich text
//! colours): a printed string's `RichText` tags drawn as the style they ask
//! for, parsed the way a `RichText` label parses them
//! (`rbx_viewer::rich_runs`), instead of shown as typed.

use std::ops::Range;

use gpui_kit::*;

/// `text` as typed, or with its markup drawn when `rich` is on. Runs with
/// no colour of their own keep the row's.
pub(super) fn text(text: SharedString, rich: bool) -> AnyElement {
    if !rich {
        return text.into_any_element();
    }
    let (plain, highlights) = styled(&text);
    StyledText::new(plain)
        .with_highlights(highlights)
        .into_any_element()
}

/// The markup's text with the tags gone, and the style of each run that has
/// one.
fn styled(markup: &str) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let mut plain = String::new();
    let mut highlights = Vec::new();
    for run in rbx_viewer::rich_runs(markup) {
        let start = plain.len();
        plain.push_str(&run.text);
        let style = HighlightStyle {
            color: run
                .color
                .map(|[r, g, b]| Rgba { r, g, b, a: 1.0 }.into()),
            font_weight: run.bold.then_some(FontWeight::BOLD),
            font_style: run.italic.then_some(FontStyle::Italic),
            underline: run.underline.then_some(UnderlineStyle {
                thickness: px(1.),
                ..UnderlineStyle::default()
            }),
            strikethrough: run.strike.then_some(StrikethroughStyle {
                thickness: px(1.),
                ..StrikethroughStyle::default()
            }),
            ..HighlightStyle::default()
        };
        if style != HighlightStyle::default() {
            highlights.push((start..plain.len(), style));
        }
    }
    (plain, highlights)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{FontWeight, Rgba};

    use super::styled;

    #[test]
    fn tags_become_styles_and_leave_the_text() {
        let (plain, highlights) =
            styled(r##"<font color="#ff0000">hi</font> <b>bold</b> <u>u</u> &lt;3"##);
        assert_eq!(plain, "hi bold u <3");
        assert_eq!(highlights.len(), 3);
        assert_eq!(highlights[0].0, 0..2);
        // `#ff0000` comes back through the label parser's linear colour.
        let red = Rgba::from(highlights[0].1.color.expect("coloured"));
        assert!(red.r > 0.999 && red.g < 0.001 && red.b < 0.001);
        assert_eq!(highlights[1].0, 3..7);
        assert_eq!(highlights[1].1.font_weight, Some(FontWeight::BOLD));
        assert!(highlights[2].1.underline.is_some());
    }

    #[test]
    fn plain_text_has_no_runs() {
        assert_eq!(styled("just text"), ("just text".to_owned(), Vec::new()));
    }
}
