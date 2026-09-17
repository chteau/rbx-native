//! The text half of a `TextLabel`/`TextButton`/`TextBox`, read into a
//! resolution-independent [`Text`]: what to draw, in which face, colour and
//! size, and how to fit it into the box `super::super::layout` resolves. The
//! string itself is already split into styled [`Span`]s here — one for plain
//! text, several once `RichText` markup is parsed (see [`rich`]) — so nothing
//! downstream ever sees a tag.

mod rich;

use std::collections::BTreeMap;

use rbx_assets::AssetRef;
use rbx_dom::{Font, FontStyle, Variant};

use super::props::{alpha, color, enum_of, flag, float, integer, string};
use super::Align;
use crate::fonts::Face;

/// Roblox's own default `TextColor3`, `Color3.fromRGB(27, 42, 53)` — the
/// same as its default `BorderColor3`. A place file serializes the property,
/// so this only shows on a tree built in code.
const DEFAULT_TEXT: [f32; 3] = [27.0 / 255.0, 42.0 / 255.0, 53.0 / 255.0];

/// `TextBox.PlaceholderColor3`'s default, `Color3.fromRGB(178, 178, 178)`.
/// Roblox's docs give no default; this is what Studio's Properties window
/// shows for a fresh `TextBox`.
const DEFAULT_PLACEHOLDER: [f32; 3] = [178.0 / 255.0; 3];

/// `TextXAlignment`: Left = 0, Right = 1, Center = 2.
const X_RIGHT: u32 = 1;
const X_CENTER: u32 = 2;
/// `TextYAlignment`: Top = 0, Center = 1, Bottom = 2.
const Y_CENTER: u32 = 1;
const Y_BOTTOM: u32 = 2;
/// `TextTruncate.None`; `AtEnd` (1) and `SplitWord` (2) both end in `...`.
const TRUNCATE_NONE: u32 = 0;
/// `AutomaticSize`: None = 0, X = 1, Y = 2, XY = 3 — a bit per axis.
const AUTOMATIC_X: u32 = 1;
const AUTOMATIC_Y: u32 = 2;

/// One run of characters sharing a style. Plain text is a single span with
/// every override unset; rich text markup sets them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Span {
    pub(crate) text: String,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) strike: bool,
    /// `<font color>`, linearised like every other colour read here.
    pub(crate) color: Option<[f32; 3]>,
    /// `<font transparency>` as an alpha, in place of `TextTransparency`.
    pub(crate) alpha: Option<f32>,
    /// `<font size>`, in place of `TextSize`.
    pub(crate) size: Option<f32>,
    /// `<font face>`/`<font family>`, in place of `FontFace`'s family.
    pub(crate) family: Option<AssetRef>,
    /// `<font weight>`, in place of `FontFace`'s weight.
    pub(crate) weight: Option<u16>,
}

impl Span {
    pub(crate) fn plain(text: &str) -> Self {
        Span {
            text: text.to_string(),
            ..Span::default()
        }
    }

    /// Whether the two would shape identically, text aside.
    fn same_style(&self, other: &Span) -> bool {
        let strip = |span: &Span| Span {
            text: String::new(),
            ..span.clone()
        };
        strip(self) == strip(other)
    }
}

/// Everything a text object says about its text, short of pixels.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Text {
    pub(crate) spans: Vec<Span>,
    /// `TextColor3` — or `PlaceholderColor3` while a `TextBox` shows its
    /// placeholder.
    pub(crate) color: [f32; 3],
    /// `1 - TextTransparency`.
    pub(crate) alpha: f32,
    /// `TextSize`: the height of one line, in pixels. Ignored with `scaled`.
    pub(crate) size: f32,
    /// `TextScaled`: the size is the largest that fits the box instead, and
    /// the text wraps whether or not `TextWrapped` says so.
    pub(crate) scaled: bool,
    pub(crate) wrapped: bool,
    /// `TextXAlignment`/`TextYAlignment`, `Start` being Left/Top.
    pub(crate) x_align: Align,
    pub(crate) y_align: Align,
    pub(crate) face: Face,
    /// `LineHeight`: line spacing as a multiple of the text size.
    pub(crate) line_height: f32,
    /// `TextStrokeColor3` with `1 - TextStrokeTransparency`, `None` once the
    /// stroke is fully transparent (the default).
    pub(crate) stroke: Option<([f32; 3], f32)>,
    /// `TextTruncate` other than `None`: overflow ends in an ellipsis.
    pub(crate) truncate: bool,
    /// `MaxVisibleGraphemes`, `None` for -1 (everything).
    pub(crate) max_graphemes: Option<usize>,
    /// `AutomaticSize` per axis: the box grows to the text along it.
    pub(crate) automatic: [bool; 2],
    /// `UITextSizeConstraint`'s (`MinTextSize`, `MaxTextSize`), which bounds
    /// the size `TextScaled` searches and clamps a plain `TextSize` too.
    pub(crate) size_bounds: Option<(f32, f32)>,
}

impl Text {
    /// Whether drawing this would put a pixel down.
    pub(crate) fn visible(&self) -> bool {
        (self.alpha > 0.0 || self.stroke.is_some())
            && self.spans.iter().any(|span| !span.text.trim().is_empty())
    }

    /// Every face the text needs, the base one first.
    pub(crate) fn faces(&self, into: &mut Vec<Face>) {
        let mut push = |face: Face| {
            if !into.contains(&face) {
                into.push(face);
            }
        };
        push(self.face.clone());
        for span in &self.spans {
            if span.family.is_some() || span.weight.is_some() || span.bold || span.italic {
                push(span_face(&self.face, span));
            }
        }
    }
}

/// The face one span shapes in: its own overrides over the text's face, a
/// `<b>` asking for bold weight and an `<i>` for italic.
pub(crate) fn span_face(base: &Face, span: &Span) -> Face {
    Face {
        family: span.family.clone().unwrap_or_else(|| base.family.clone()),
        weight: span
            .weight
            .unwrap_or(if span.bold { 700 } else { base.weight }),
        italic: base.italic || span.italic,
    }
}

/// Reads a text object's properties. `text_box` selects the `TextBox` rule:
/// an empty `Text` shows `PlaceholderText` in `PlaceholderColor3` instead.
pub(super) fn text(
    properties: &BTreeMap<String, Variant>,
    text_box: bool,
    size_bounds: Option<(f32, f32)>,
) -> Text {
    let mut content = string(properties, "Text");
    let mut tint = color(properties, "TextColor3", DEFAULT_TEXT);
    if text_box && content.is_empty() {
        content = string(properties, "PlaceholderText");
        tint = color(properties, "PlaceholderColor3", DEFAULT_PLACEHOLDER);
    }
    // Roblox's docs: a tab "will render as a space instead".
    content = content.replace('\t', " ");
    let spans = match flag(properties, "RichText", false) {
        true => rich::parse(&content),
        false => vec![Span::plain(&content)],
    };

    // Unlike every other transparency, this one defaults to 1: a text object
    // has no stroke until it is asked for.
    let stroke_alpha = 1.0 - float(properties, "TextStrokeTransparency", 1.0).clamp(0.0, 1.0);
    let automatic = enum_of(properties, "AutomaticSize", 0);
    Text {
        spans,
        color: tint,
        alpha: alpha(properties, "TextTransparency"),
        size: float(properties, "TextSize", 14.0).max(1.0),
        scaled: flag(properties, "TextScaled", false),
        wrapped: flag(properties, "TextWrapped", false),
        x_align: match enum_of(properties, "TextXAlignment", X_CENTER) {
            X_RIGHT => Align::End,
            X_CENTER => Align::Center,
            _ => Align::Start,
        },
        y_align: match enum_of(properties, "TextYAlignment", Y_CENTER) {
            Y_BOTTOM => Align::End,
            Y_CENTER => Align::Center,
            _ => Align::Start,
        },
        face: face(properties),
        line_height: float(properties, "LineHeight", 1.0).clamp(1.0, 3.0),
        stroke: (stroke_alpha > 0.0).then(|| {
            (
                color(properties, "TextStrokeColor3", [0.0; 3]),
                stroke_alpha,
            )
        }),
        truncate: enum_of(properties, "TextTruncate", TRUNCATE_NONE) != TRUNCATE_NONE,
        max_graphemes: usize::try_from(integer(properties, "MaxVisibleGraphemes", -1)).ok(),
        automatic: [automatic & AUTOMATIC_X != 0, automatic & AUTOMATIC_Y != 0],
        size_bounds,
    }
}

/// `FontFace` where the place carries one; the legacy `Font` enum otherwise,
/// which older places serialize alone.
fn face(properties: &BTreeMap<String, Variant>) -> Face {
    let face_of = |font: &Font| {
        let family = AssetRef::parse(&font.family).ok()?;
        (family != AssetRef::Empty).then_some(Face {
            family,
            weight: font.weight,
            italic: font.style == FontStyle::Italic,
        })
    };
    let stored = match properties.get("FontFace") {
        Some(Variant::Font(font)) => face_of(font),
        _ => None,
    };
    stored
        .or_else(|| match properties.get("Font") {
            // `Enum.Font` → face, as `Datatype.Font.fromEnum`'s table in
            // Roblox's docs lays it out; `Unknown` and anything newer fall
            // to the default.
            Some(&Variant::Enum(legacy)) => Font::from_legacy(legacy).as_ref().and_then(face_of),
            _ => None,
        })
        .unwrap_or_default()
}
