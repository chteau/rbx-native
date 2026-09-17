//! The `UIGridStyleLayout` family read off a container's children: the one
//! layout instance among them decides where every sibling goes, replacing the
//! `Position` (and, under flex, the `Size`) each sibling asks for.
//!
//! Only the properties are read here; the arithmetic lives in
//! [`crate::scene::gui::layout`], which is where a pixel rect is known.

use std::collections::BTreeMap;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{enum_of, flag, integer, span, Span};

/// Every layout class this viewer resolves. Roblox honours exactly one layout
/// per container, so a node holds at most one of these.
///
/// TODO: `UIPageLayout`, which shows one child at a time and animates between
/// them; neither the page transition nor `CurrentPage`'s referent is read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::scene::gui) enum Layout {
    List(List),
    Grid(Grid),
    Table(Table),
}

/// Where a layout puts its content along one axis, or each item across the
/// other. `Start` is Left/Top, `End` Right/Bottom; the two Roblox enums share
/// ordinals (Center = 0, Left/Top = 1, Right/Bottom = 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::scene::gui) enum Align {
    Center,
    Start,
    End,
}

/// `Enum.UIFlexAlignment`: what a `UIListLayout` does with the space left over
/// once every sibling has taken its own size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::scene::gui) enum Flex {
    None,
    Fill,
    SpaceAround,
    SpaceBetween,
    SpaceEvenly,
}

/// `Enum.ItemLineAlignment`: where an item sits across the line it landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::scene::gui) enum LineAlign {
    /// Follow the layout's own cross-axis alignment.
    Automatic,
    Start,
    Center,
    End,
    Stretch,
}

/// A `UIListLayout` read off its siblings: they are stacked along one axis in
/// a sort order of their own, their `Position` ignored and their `Size` kept
/// unless flex resizes them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::scene::gui) struct List {
    pub(in crate::scene::gui) vertical: bool,
    /// `Padding`, a `UDim` resolved against the parent's extent along the
    /// fill direction.
    pub(in crate::scene::gui) padding: (f32, f32),
    pub(in crate::scene::gui) horizontal: Align,
    pub(in crate::scene::gui) vertical_align: Align,
    /// `SortOrder.Name`; otherwise `LayoutOrder`, ties in tree order.
    pub(in crate::scene::gui) by_name: bool,
    /// `HorizontalFlex`/`VerticalFlex`, already mapped onto the fill axis and
    /// the cross axis: which of the two properties is which depends only on
    /// `FillDirection`, so the layout arithmetic never has to ask again.
    pub(in crate::scene::gui) flex: Flex,
    pub(in crate::scene::gui) cross_flex: Flex,
    pub(in crate::scene::gui) wraps: bool,
    pub(in crate::scene::gui) line_align: LineAlign,
}

/// A `UIFlexItem` under one of a `UIListLayout`'s siblings, overriding that
/// sibling's share of the line's free space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::scene::gui) struct FlexItem {
    /// `FlexMode` as the grow:shrink ratio pair the docs define it by —
    /// Grow 1:0, Shrink 0:1, Fill 1:1, None 0:0, Custom the two ratios.
    pub(in crate::scene::gui) grow: f32,
    pub(in crate::scene::gui) shrink: f32,
    pub(in crate::scene::gui) line_align: LineAlign,
}

/// A `UIGridLayout`: uniform cells, filled a line at a time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::scene::gui) struct Grid {
    /// `CellSize` and `CellPadding`, both `UDim2`s against the parent.
    pub(in crate::scene::gui) cell: Span,
    pub(in crate::scene::gui) cell_padding: Span,
    pub(in crate::scene::gui) vertical: bool,
    /// `FillDirectionMaxCells`; 0 means "as many as fit".
    pub(in crate::scene::gui) max_cells: usize,
    /// `StartCorner` as a mirror per axis: TopRight flips x, BottomLeft y.
    pub(in crate::scene::gui) flip: [bool; 2],
    pub(in crate::scene::gui) horizontal: Align,
    pub(in crate::scene::gui) vertical_align: Align,
    pub(in crate::scene::gui) by_name: bool,
}

/// A `UITableLayout`: the siblings are the rows (or columns), and *their*
/// children are the cells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::scene::gui) struct Table {
    /// `MajorAxis.RowMajor`, the default: siblings are rows.
    ///
    /// `UITableLayout` also inherits `FillDirection`, which the docs describe
    /// as controlling the very same thing ("the default FillDirection of
    /// Vertical means that siblings are first positioned into rows"). With no
    /// documented tie-break between the two, `MajorAxis` — the property the
    /// class defines for exactly this — wins and `FillDirection` is ignored.
    pub(in crate::scene::gui) row_major: bool,
    /// `Padding`, a `UDim2` between cells on both axes.
    pub(in crate::scene::gui) padding: Span,
    /// `FillEmptySpaceColumns`/`FillEmptySpaceRows`, indexed by axis (x, y).
    pub(in crate::scene::gui) fill: [bool; 2],
    pub(in crate::scene::gui) horizontal: Align,
    pub(in crate::scene::gui) vertical_align: Align,
    pub(in crate::scene::gui) by_name: bool,
}

const LIST_CLASS: &str = "UIListLayout";
const GRID_CLASS: &str = "UIGridLayout";
const TABLE_CLASS: &str = "UITableLayout";
const FLEX_ITEM_CLASS: &str = "UIFlexItem";

/// `Enum.FillDirection.Vertical`; `Horizontal` is 0.
const FILL_VERTICAL: u32 = 1;
/// `Enum.SortOrder.Name`; `LayoutOrder` (the default) is 2.
const SORT_NAME: u32 = 0;
const SORT_LAYOUT_ORDER: u32 = 2;

/// The layout among `children` that arranges them, or `None`.
///
/// Roblox honours the first instance of a given layout class; where a
/// container somehow holds two different ones this settles it in tree order,
/// which is as good a rule as any — the docs describe no such case at all.
pub(in crate::scene::gui) fn layout_of(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    children: &[Ref],
) -> Option<Layout> {
    children.iter().find_map(|&child| {
        let instance = dom.get(child)?;
        let properties = instance.properties();
        let class = instance.class();
        if is(database, class, LIST_CLASS) {
            Some(Layout::List(list(properties)))
        } else if is(database, class, GRID_CLASS) {
            Some(Layout::Grid(grid(properties)))
        } else if is(database, class, TABLE_CLASS) {
            Some(Layout::Table(table(properties)))
        } else {
            None
        }
    })
}

/// Whether `class` is `target` or a subclass of it.
///
/// The name is checked first because the bundled API dump is a snapshot: it
/// has never heard of `UIFlexItem`, and an unknown class is not a subclass of
/// anything. Every class matched here is a leaf in Roblox's own hierarchy, so
/// the name alone would in fact do — the database lookup only keeps a future
/// subclass working.
fn is(database: &ReflectionDatabase, class: &str, target: &str) -> bool {
    class == target || database.is_subclass_of(class, target)
}

/// The `UIFlexItem` among an item's own children, if it has one.
pub(in crate::scene::gui) fn flex_item(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    children: &[Ref],
) -> Option<FlexItem> {
    let item = children.iter().find_map(|&child| {
        let instance = dom.get(child)?;
        is(database, instance.class(), FLEX_ITEM_CLASS).then_some(instance)
    })?;
    let properties = item.properties();
    // `FlexMode` as a grow:shrink pair, per `UIFlexItem.FlexMode`'s own docs.
    let (grow, shrink) = match enum_of(properties, "FlexMode", 0) {
        1 => (1.0, 0.0),
        2 => (0.0, 1.0),
        3 => (1.0, 1.0),
        4 => (
            number(properties, "GrowRatio").max(0.0),
            number(properties, "ShrinkRatio").max(0.0),
        ),
        _ => (0.0, 0.0),
    };
    Some(FlexItem {
        grow,
        shrink,
        line_align: line_align(properties),
    })
}

fn list(properties: &BTreeMap<String, Variant>) -> List {
    let vertical = enum_of(properties, "FillDirection", FILL_VERTICAL) == FILL_VERTICAL;
    let horizontal_flex = flex(properties, "HorizontalFlex");
    let vertical_flex = flex(properties, "VerticalFlex");
    List {
        vertical,
        padding: udim(properties, "Padding"),
        horizontal: align(properties, "HorizontalAlignment"),
        vertical_align: align(properties, "VerticalAlignment"),
        by_name: by_name(properties),
        flex: match vertical {
            true => vertical_flex,
            false => horizontal_flex,
        },
        cross_flex: match vertical {
            true => horizontal_flex,
            false => vertical_flex,
        },
        wraps: flag(properties, "Wraps", false),
        line_align: line_align(properties),
    }
}

fn grid(properties: &BTreeMap<String, Variant>) -> Grid {
    let start = enum_of(properties, "StartCorner", 0);
    Grid {
        // The two `UDim2` defaults the docs quote for a grid built in code.
        cell: udim2_or(properties, "CellSize", [100.0, 100.0]),
        cell_padding: udim2_or(properties, "CellPadding", [5.0, 5.0]),
        vertical: enum_of(properties, "FillDirection", 0) == FILL_VERTICAL,
        max_cells: integer(properties, "FillDirectionMaxCells", 0).max(0) as usize,
        // TopLeft 0, TopRight 1, BottomLeft 2, BottomRight 3.
        flip: [start == 1 || start == 3, start >= 2],
        horizontal: align(properties, "HorizontalAlignment"),
        vertical_align: align(properties, "VerticalAlignment"),
        by_name: by_name(properties),
    }
}

fn table(properties: &BTreeMap<String, Variant>) -> Table {
    Table {
        row_major: enum_of(properties, "MajorAxis", 0) == 0,
        padding: span(properties, "Padding"),
        fill: [
            flag(properties, "FillEmptySpaceColumns", false),
            flag(properties, "FillEmptySpaceRows", false),
        ],
        horizontal: align(properties, "HorizontalAlignment"),
        vertical_align: align(properties, "VerticalAlignment"),
        by_name: by_name(properties),
    }
}

fn by_name(properties: &BTreeMap<String, Variant>) -> bool {
    enum_of(properties, "SortOrder", SORT_LAYOUT_ORDER) == SORT_NAME
}

/// `HorizontalAlignment`/`VerticalAlignment`, both defaulting to Center.
fn align(properties: &BTreeMap<String, Variant>, name: &str) -> Align {
    match enum_of(properties, name, 0) {
        1 => Align::Start,
        2 => Align::End,
        _ => Align::Center,
    }
}

fn flex(properties: &BTreeMap<String, Variant>, name: &str) -> Flex {
    match enum_of(properties, name, 0) {
        1 => Flex::Fill,
        2 => Flex::SpaceAround,
        3 => Flex::SpaceBetween,
        4 => Flex::SpaceEvenly,
        _ => Flex::None,
    }
}

fn line_align(properties: &BTreeMap<String, Variant>) -> LineAlign {
    match enum_of(properties, "ItemLineAlignment", 0) {
        1 => LineAlign::Start,
        2 => LineAlign::Center,
        3 => LineAlign::End,
        4 => LineAlign::Stretch,
        _ => LineAlign::Automatic,
    }
}

/// One `UDim`, zero where absent.
fn udim(properties: &BTreeMap<String, Variant>, name: &str) -> (f32, f32) {
    match properties.get(name) {
        Some(Variant::UDim(value)) => (value.scale, value.offset as f32),
        _ => (0.0, 0.0),
    }
}

/// A `UDim2` whose absence means a non-zero pixel default rather than zero,
/// which is the case for both of `UIGridLayout`'s cell spans.
fn udim2_or(properties: &BTreeMap<String, Variant>, name: &str, offset: [f32; 2]) -> Span {
    match properties.contains_key(name) {
        true => span(properties, name),
        false => Span {
            scale: [0.0, 0.0],
            offset,
        },
    }
}

/// A `float` property, 0 where absent.
fn number(properties: &BTreeMap<String, Variant>, name: &str) -> f32 {
    match properties.get(name) {
        Some(&Variant::Float32(value)) => value,
        Some(&Variant::Float64(value)) => value as f32,
        _ => 0.0,
    }
}
