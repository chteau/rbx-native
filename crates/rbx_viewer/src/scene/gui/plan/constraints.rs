//! The `UIComponent`s that change how big a `GuiObject` ends up — `UIPadding`,
//! `UIScale` and the `UIConstraint` family — plus the `GuiObject` properties
//! that decide what its `Size` is even measured against.
//!
//! Roblox honours one instance of each class per parent; where several are
//! present the first in tree order wins, matching how `plan::list_layout`
//! already picks a `UIListLayout`.

use std::collections::BTreeMap;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::props::{enum_of, float, integer, udim, vector2};
use crate::scene::gui::style::Styled;

/// `UIPadding`, four `UDim`s that shrink the box this element's children —
/// and any layout arranging them — resolve against.
///
/// Each side is "relative to the parent's normal size" per the docs, which do
/// not name an axis; the scale is taken against the matching axis of the
/// element's own resolved size, the only reading under which left/right and
/// top/bottom stay symmetric.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::scene::gui) struct Padding {
    pub(in crate::scene::gui) left: (f32, f32),
    pub(in crate::scene::gui) right: (f32, f32),
    pub(in crate::scene::gui) top: (f32, f32),
    pub(in crate::scene::gui) bottom: (f32, f32),
}

impl Padding {
    /// The four sides in pixels — left, right, top, bottom — inside a box of
    /// `extent`.
    pub(in crate::scene::gui) fn against(&self, extent: [f32; 2]) -> [f32; 4] {
        [
            self.left.0 * extent[0] + self.left.1,
            self.right.0 * extent[0] + self.right.1,
            self.top.0 * extent[1] + self.top.1,
            self.bottom.0 * extent[1] + self.bottom.1,
        ]
    }
}

/// `UIAspectRatioConstraint`: a width-to-height ratio the element keeps
/// "regardless of its core size".
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::scene::gui) struct Aspect {
    /// `AspectRatio`, width over height. The docs require it to be positive.
    pub(in crate::scene::gui) ratio: f32,
    /// `AspectType.ScaleWithParentSize`, where the box the ratio has to fit
    /// inside is the parent's; `FitWithinMaxSize` (the default) fits it
    /// inside the element's own resolved size.
    pub(in crate::scene::gui) with_parent: bool,
    /// `DominantAxis.Height`; `Width` is the default.
    pub(in crate::scene::gui) height_dominant: bool,
}

/// `UISizeConstraint`, a hard pixel clamp on the resolved size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::scene::gui) struct SizeBounds {
    pub(in crate::scene::gui) min: [f32; 2],
    pub(in crate::scene::gui) max: [f32; 2],
}

/// `GuiObject.SizeConstraint`: which of the parent's axes this element's own
/// `Size` *scales* are taken against. Offsets are pixels and never remapped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::scene::gui) enum SizeAxes {
    #[default]
    RelativeXY,
    /// "The scale of X is shared with Y" — both axes resolve against width.
    RelativeXX,
    RelativeYY,
}

impl SizeAxes {
    /// The extent a `Size` scale is multiplied by, per axis.
    pub(in crate::scene::gui) fn against(self, parent: [f32; 2]) -> [f32; 2] {
        match self {
            SizeAxes::RelativeXY => parent,
            SizeAxes::RelativeXX => [parent[0], parent[0]],
            SizeAxes::RelativeYY => [parent[1], parent[1]],
        }
    }
}

/// `GuiObject.BorderMode`: where the `BorderSizePixel` bands sit relative to
/// the element's own edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::scene::gui) enum Border {
    /// The border grows outward, the box itself untouched.
    #[default]
    Outline,
    /// The border grows evenly inward and outward, so the drawn area it eats
    /// into shrinks by one pixel per pixel of width — the docs' "1:1 ratio".
    Middle,
    /// The border grows inward only: two pixels of the box per pixel of
    /// width, the docs' "1:2 ratio".
    Inset,
}

impl Border {
    /// How far inside the element's own rect the border's outer edge sits.
    pub(in crate::scene::gui) fn inset(self, width: f32) -> f32 {
        match self {
            Border::Outline => 0.0,
            Border::Middle => width * 0.5,
            Border::Inset => width,
        }
    }
}

/// Everything the `UIComponent` children of one `GuiObject` say about its size.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::scene::gui) struct Constraints {
    pub(in crate::scene::gui) padding: Option<Padding>,
    pub(in crate::scene::gui) scale: Option<f32>,
    pub(in crate::scene::gui) aspect: Option<Aspect>,
    pub(in crate::scene::gui) size_bounds: Option<SizeBounds>,
    /// `UITextSizeConstraint`'s `MinTextSize`/`MaxTextSize`. Read here because
    /// it is one of the `UIConstraint` family; nothing in this module acts on
    /// it — it is the text side that clamps a font size with it.
    pub(in crate::scene::gui) text_size_bounds: Option<(f32, f32)>,
}

/// Reads the first enabled instance of each class among `children`.
pub(in crate::scene::gui) fn constraints(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    styles: &Styled,
    children: &[Ref],
) -> Constraints {
    let mut found = Constraints::default();
    for &child in children {
        let Some(instance) = dom.get(child) else {
            continue;
        };
        let properties = styles.properties_of(instance);
        // `UIComponent.Enabled` does not exist; a constraint is disabled by
        // being removed. Only the class matters.
        match instance.class() {
            "UIPadding" if found.padding.is_none() => {
                found.padding = Some(padding(properties));
            }
            "UIScale" if found.scale.is_none() => {
                found.scale = Some(float(properties, "Scale", 1.0));
            }
            "UISizeConstraint" if found.size_bounds.is_none() => {
                found.size_bounds = Some(SizeBounds {
                    min: vector2(properties, "MinSize"),
                    // Roblox's own default: unbounded above.
                    max: bound(properties, "MaxSize"),
                });
            }
            "UITextSizeConstraint" if found.text_size_bounds.is_none() => {
                found.text_size_bounds = Some((
                    integer(properties, "MinTextSize", 1) as f32,
                    integer(properties, "MaxTextSize", 1000) as f32,
                ));
            }
            class if database.is_subclass_of(class, "UIAspectRatioConstraint") => {
                let ratio = float(properties, "AspectRatio", 1.0);
                // "This value must be greater than 0"; a place that broke that
                // rule would otherwise divide by zero below.
                if found.aspect.is_none() && ratio > 0.0 {
                    found.aspect = Some(Aspect {
                        ratio,
                        with_parent: enum_of(properties, "AspectType", 0) == SCALE_WITH_PARENT,
                        height_dominant: enum_of(properties, "DominantAxis", 0) == DOMINANT_HEIGHT,
                    });
                }
            }
            _ => {}
        }
    }
    found
}

/// `Enum.AspectType.ScaleWithParentSize`; `FitWithinMaxSize` is 0.
const SCALE_WITH_PARENT: u32 = 1;
/// `Enum.DominantAxis.Height`; `Width` is 0.
const DOMINANT_HEIGHT: u32 = 1;

/// `Enum.AutomaticSize`, read as one flag per axis: `X` is 1, `Y` is 2 and
/// `XY` is 3, so the ordinal is a bit field.
pub(in crate::scene::gui) fn automatic_size(properties: &BTreeMap<String, Variant>) -> [bool; 2] {
    let value = enum_of(properties, "AutomaticSize", 0);
    [value & 1 != 0, value & 2 != 0]
}

pub(in crate::scene::gui) fn size_axes(properties: &BTreeMap<String, Variant>) -> SizeAxes {
    match enum_of(properties, "SizeConstraint", 0) {
        1 => SizeAxes::RelativeXX,
        2 => SizeAxes::RelativeYY,
        _ => SizeAxes::RelativeXY,
    }
}

pub(in crate::scene::gui) fn border_mode(properties: &BTreeMap<String, Variant>) -> Border {
    match enum_of(properties, "BorderMode", 0) {
        1 => Border::Middle,
        2 => Border::Inset,
        _ => Border::Outline,
    }
}

fn padding(properties: &BTreeMap<String, Variant>) -> Padding {
    Padding {
        left: side(properties, "PaddingLeft"),
        right: side(properties, "PaddingRight"),
        top: side(properties, "PaddingTop"),
        bottom: side(properties, "PaddingBottom"),
    }
}

/// One side of a `UIPadding`, no padding at all where the property is
/// missing.
fn side(properties: &BTreeMap<String, Variant>, name: &str) -> (f32, f32) {
    udim(properties, name).unwrap_or((0.0, 0.0))
}

/// A `Vector2` size limit, each axis unbounded where the property is missing.
fn bound(properties: &BTreeMap<String, Variant>, name: &str) -> [f32; 2] {
    match properties.get(name) {
        Some(Variant::Vector2(value)) => [value.x, value.y],
        _ => [f32::INFINITY; 2],
    }
}

/// The pixels a `ScreenGui` gives up at the top of the screen to Roblox's own
/// top bar.
///
/// The docs describe this inset only indirectly: `GuiService:GetGuiInset()`
/// and `GuiService.TopbarInset` hand it back at runtime and are explicitly
/// dynamic, and no page states a number. 36 is the height the desktop top bar
/// occupies at a 1:1 scale, which is what a place authored on a PC is drawn
/// against.
pub(in crate::scene::gui) const TOP_BAR_INSET: f32 = 36.0;

/// `ScreenGui.ScreenInsets`/`IgnoreGuiInset`: whether the canvas starts below
/// the top bar.
///
/// Only the core-UI modes reserve anything here. `DeviceSafeInsets` insets for
/// screen cutouts — "no inset is added for Roblox core UI elements" — and a
/// desktop window has none, so it comes to nothing; `TopbarSafeInsets` is
/// measured against the top bar area itself, so its contents still start below
/// the bar.
pub(in crate::scene::gui) fn top_bar_inset(properties: &BTreeMap<String, Variant>) -> f32 {
    // `IgnoreGuiInset` is the legacy spelling older places serialize: setting
    // it moves `ScreenInsets` from `CoreUISafeInsets` to `DeviceSafeInsets`.
    if super::props::flag(properties, "IgnoreGuiInset", false) {
        return 0.0;
    }
    match enum_of(properties, "ScreenInsets", CORE_UI_SAFE_INSETS) {
        CORE_UI_SAFE_INSETS | TOPBAR_SAFE_INSETS => TOP_BAR_INSET,
        _ => 0.0,
    }
}

/// `Enum.ScreenInsets.CoreUISafeInsets`, the property's own default, and
/// `TopbarSafeInsets`.
const CORE_UI_SAFE_INSETS: u32 = 2;
const TOPBAR_SAFE_INSETS: u32 = 3;
/// `Enum.ScreenInsets.None`.
const NO_INSETS: u32 = 0;

/// `ScreenGui.ClipToDeviceSafeArea`: whether the screen's descendants are
/// scissored to the safe area the insets leave.
///
/// "If this property is `true`, all `GuiObject` descendants of the `ScreenGui`
/// will be clipped to the device's safe area"; it defaults to true, and "will
/// be ignored if you set `ScreenInsets` to `None`, as doing so implies that
/// you intentionally want to disregard the device's safe insets"
/// (`ScreenGui.ClipToDeviceSafeArea`). `IgnoreGuiInset` is the legacy spelling
/// of that same opt-out (see [`top_bar_inset`]).
pub(in crate::scene::gui) fn clip_to_safe_area(properties: &BTreeMap<String, Variant>) -> bool {
    if super::props::flag(properties, "IgnoreGuiInset", false)
        || enum_of(properties, "ScreenInsets", CORE_UI_SAFE_INSETS) == NO_INSETS
    {
        return false;
    }
    super::props::flag(properties, "ClipToDeviceSafeArea", true)
}

/// `LayerCollector.ZIndexBehavior.Global`, where `ZIndex` is compared across
/// every descendant rather than among siblings. `Sibling` (1) is the default.
pub(in crate::scene::gui) fn global_z_index(properties: &BTreeMap<String, Variant>) -> bool {
    enum_of(properties, "ZIndexBehavior", 1) == 0
}
