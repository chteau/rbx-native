//! What each inspector field is: where its value lives — on the element
//! or on a modifier under it — which numbers of which properties it is,
//! and how it reads.

use super::Shell;
use crate::ui_canvas::Unit;

/// One value the inspector shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::shell::ui_editor) enum Key {
    X,
    Y,
    W,
    H,
    Rotation,
    Opacity,
    Radius,
    CornerTopLeft,
    CornerTopRight,
    CornerBottomRight,
    CornerBottomLeft,
    Fill,
    FillAlpha,
    Stroke,
    StrokeAlpha,
    StrokeWidth,
    Gap,
    GridGapX,
    GridGapY,
    Padding,
    PadTop,
    PadBottom,
    PadLeft,
    PadRight,
    GradientRotation,
    Aspect,
    MinW,
    MinH,
    MaxW,
    MaxH,
    MinText,
    MaxText,
    Scale,
}

/// Each corner's own radius, clockwise from the top left — the order the
/// canvas's handles go round in.
pub(in crate::shell::ui_editor) const CORNERS: [Key; 4] = [
    Key::CornerTopLeft,
    Key::CornerTopRight,
    Key::CornerBottomRight,
    Key::CornerBottomLeft,
];

impl Key {
    /// Whether the field stands whether or not its modifier does — a
    /// corner's radius, the padding — reading 0 without it and making it
    /// on the first edit.
    pub(super) fn made_on_edit(self) -> bool {
        matches!(
            self,
            Key::Radius
                | Key::Padding
                | Key::PadTop
                | Key::PadBottom
                | Key::PadLeft
                | Key::PadRight
        )
    }
}

/// Where a value lives: on the element, or on its first child of a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell::ui_editor) enum On {
    Own,
    Child(&'static str),
}

/// How a stored value reads in its field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Form {
    /// The number itself: a drag moves it `step` a pixel, whole numbers
    /// only when `whole`.
    Number { step: f32, whole: bool },
    /// A transparency, as the opacity percentage Figma shows.
    Percent,
    /// A `Color3`, as six hex digits.
    Hex,
}

const PIXELS: Form = Form::Number {
    step: 1.0,
    whole: true,
};
const FINE: Form = Form::Number {
    step: 0.01,
    whole: false,
};

/// A value's properties — one, or several written alike (`UIPadding`'s
/// four sides) — and which of each one's numbers it is.
#[derive(Debug, Clone, Copy)]
pub(super) struct Spec {
    pub(super) on: On,
    pub(super) properties: &'static [&'static str],
    pub(super) parts: &'static [usize],
    pub(super) form: Form,
}

impl Shell {
    pub(super) fn spec(&self, key: Key) -> Spec {
        let scale = self.ui.unit == Unit::Scale;
        let udim = match scale {
            true => Form::Number {
                step: 0.001,
                whole: false,
            },
            false => PIXELS,
        };
        let (x, y): (&[usize], &[usize]) = match scale {
            true => (&[0], &[2]),
            false => (&[1], &[3]),
        };
        let own = |properties, parts, form| Spec {
            on: On::Own,
            properties,
            parts,
            form,
        };
        let child = |class, properties, parts, form| Spec {
            on: On::Child(class),
            properties,
            parts,
            form,
        };
        match key {
            Key::X => own(&["Position"], x, udim),
            Key::Y => own(&["Position"], y, udim),
            Key::W => own(&["Size"], x, udim),
            Key::H => own(&["Size"], y, udim),
            Key::Rotation => own(
                &["Rotation"],
                &[0],
                Form::Number {
                    step: 1.0,
                    whole: false,
                },
            ),
            // A `CanvasGroup` is the one element with a layer opacity; any
            // other's is its fill's, as Sketch keeps the two in step.
            Key::Opacity if self.anchor_is("CanvasGroup") => {
                own(&["GroupTransparency"], &[0], Form::Percent)
            }
            Key::Opacity | Key::FillAlpha => own(&["BackgroundTransparency"], &[0], Form::Percent),
            Key::Fill => own(&["BackgroundColor3"], &[0], Form::Hex),
            // The four corners are what Roblox stores; `CornerRadius` is
            // written too where a place still keeps one, so the two never
            // disagree (see `rbx_viewer`'s `plan::corner`).
            Key::Radius => child(
                "UICorner",
                &[
                    "TopLeftRadius",
                    "TopRightRadius",
                    "BottomRightRadius",
                    "BottomLeftRadius",
                    "CornerRadius",
                ],
                &[1],
                PIXELS,
            ),
            Key::CornerTopLeft => child("UICorner", &["TopLeftRadius"], &[1], PIXELS),
            Key::CornerTopRight => child("UICorner", &["TopRightRadius"], &[1], PIXELS),
            Key::CornerBottomRight => child("UICorner", &["BottomRightRadius"], &[1], PIXELS),
            Key::CornerBottomLeft => child("UICorner", &["BottomLeftRadius"], &[1], PIXELS),
            Key::Stroke => child("UIStroke", &["Color"], &[0], Form::Hex),
            Key::StrokeAlpha => child("UIStroke", &["Transparency"], &[0], Form::Percent),
            Key::StrokeWidth => child(
                "UIStroke",
                &["Thickness"],
                &[0],
                Form::Number {
                    step: 0.1,
                    whole: false,
                },
            ),
            Key::Gap if self.anchor_child("UIGridLayout").is_some() => {
                child("UIGridLayout", &["CellPadding"], &[1, 3], PIXELS)
            }
            Key::Gap => child("UIListLayout", &["Padding"], &[1], PIXELS),
            Key::GridGapX => child("UIGridLayout", &["CellPadding"], &[1], PIXELS),
            Key::GridGapY => child("UIGridLayout", &["CellPadding"], &[3], PIXELS),
            Key::PadTop => child("UIPadding", &["PaddingTop"], &[1], PIXELS),
            Key::PadBottom => child("UIPadding", &["PaddingBottom"], &[1], PIXELS),
            Key::PadLeft => child("UIPadding", &["PaddingLeft"], &[1], PIXELS),
            Key::PadRight => child("UIPadding", &["PaddingRight"], &[1], PIXELS),
            Key::Padding => child(
                "UIPadding",
                &["PaddingLeft", "PaddingRight", "PaddingTop", "PaddingBottom"],
                &[1],
                PIXELS,
            ),
            Key::GradientRotation => child(
                "UIGradient",
                &["Rotation"],
                &[0],
                Form::Number {
                    step: 1.0,
                    whole: false,
                },
            ),
            Key::Aspect => child("UIAspectRatioConstraint", &["AspectRatio"], &[0], FINE),
            Key::MinW => child("UISizeConstraint", &["MinSize"], &[0], PIXELS),
            Key::MinH => child("UISizeConstraint", &["MinSize"], &[1], PIXELS),
            Key::MaxW => child("UISizeConstraint", &["MaxSize"], &[0], PIXELS),
            Key::MaxH => child("UISizeConstraint", &["MaxSize"], &[1], PIXELS),
            Key::MinText => child("UITextSizeConstraint", &["MinTextSize"], &[0], PIXELS),
            Key::MaxText => child("UITextSizeConstraint", &["MaxTextSize"], &[0], PIXELS),
            Key::Scale => child("UIScale", &["Scale"], &[0], FINE),
        }
    }
}
