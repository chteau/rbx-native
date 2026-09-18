//! The dock's building blocks: one Explorer row and one Properties row. The
//! panel header these used to sit under is gone — the dock's own tab title
//! (see `shell::dock`) draws that now.

use gpui_kit::assets::IconName;
use gpui_kit::component::color_picker::ColorPicker;
use gpui_kit::component::input::Input;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::select::Select;
use gpui_kit::component::tree::TreeEntry;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::explorer::ClassIcon;
use crate::properties::PropertyRow;

use super::edit::RowEditor;

const INDENT: f32 = 12.0;
const CHEVRON_WIDTH: f32 = 14.0;
const CLASS_ICON_SIZE: f32 = 14.0;
const PROPERTY_NAME_WIDTH: f32 = 130.0;
/// Out of 255 — how strongly a tagged row's hover/selected background reads
/// against the row behind it. Selected is the stronger of the two, the same
/// relationship the theme's own default blue hover/selected pair has.
const HOVER_ALPHA: u8 = 46;
const SELECTED_ALPHA: u8 = 82;

/// One instance: its depth as indentation, a chevron when it has children, the
/// class icon (already recolored to a tagged `Folder`'s own tag, if any — see
/// `explorer::items`) and the instance's name.
///
/// `tint` is that same tag, reused here for the row's hover/selected
/// background and selection outline: the vendored `ListItem` hardcodes both
/// to the theme's accent colour with no way to override them per row, so a
/// tagged row skips `ListItem` entirely and paints its own — see
/// [`tagged_row`]. Every other row keeps `ListItem` unchanged.
pub(super) fn row(
    index: usize,
    entry: &TreeEntry,
    selected: bool,
    icon: ClassIcon,
    tint: Option<(u8, u8, u8)>,
) -> AnyElement {
    let item = entry.item();
    let chevron = if entry.is_expanded() {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    };
    let class_icon = match icon {
        ClassIcon::Sprite(image) => img(image).size(px(CLASS_ICON_SIZE)).into_any_element(),
        ClassIcon::Lucide(name) => Icon::new(name).small().into_any_element(),
    };

    let content = h_flex()
        .gap_1p5()
        .py_0p5()
        .pl(px(entry.depth() as f32 * INDENT))
        .text_sm()
        .child(
            div()
                .w(px(CHEVRON_WIDTH))
                .flex_shrink_0()
                .when(entry.is_folder(), |this| {
                    this.child(Icon::new(chevron).xsmall())
                }),
        )
        .child(class_icon)
        .child(item.label.clone());

    match tint {
        None => ListItem::new(index)
            .selected(selected)
            .child(content)
            .into_any_element(),
        Some(color) => tagged_row(index, selected, color, content),
    }
}

/// A tagged `Folder`'s own row chrome, replacing `ListItem`'s hover/selected
/// painting rather than layering on top of it (painting both would double up
/// wherever they overlap): a faded tint of the tag colour for hover, a
/// stronger one plus a solid-tint outline for selected — the same
/// `.hover()`/absolutely-positioned-border technique `ListItem` itself uses
/// internally, just parameterized on this row's own colour instead of the
/// theme's fixed accent.
///
/// Built with a plain `if`/intermediate bindings rather than chained
/// `.when()`/`.when_else()` closures: nesting one of those inside another
/// (a `.hover()` closure inside a branch closure) on top of GPUI Kit's own
/// already-deep `Div` builder type overflowed rustc's type-checker during
/// this crate's own test build — a real compiler limitation, not a style
/// preference, so this shape is deliberate.
fn tagged_row(
    index: usize,
    selected: bool,
    color: (u8, u8, u8),
    content: impl IntoElement,
) -> AnyElement {
    let container = h_flex()
        .id(index)
        .relative()
        .items_center()
        .gap_x_1()
        .py_1()
        .px_3()
        .child(content);

    if selected {
        let outline = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .border_1()
            .border_color(tag_color(color, u8::MAX));
        container
            .bg(tag_color(color, SELECTED_ALPHA))
            .child(outline)
            .into_any_element()
    } else {
        let hover_bg = tag_color(color, HOVER_ALPHA);
        container
            .hover(move |this| this.bg(hover_bg))
            .into_any_element()
    }
}

/// A tagged `Folder`'s stored sRGB byte triplet (the same 0-255,
/// non-linear-light space `properties::color3` already displays these in) at
/// `alpha` out of 255 — `u8::MAX` for the fully opaque selection outline,
/// [`HOVER_ALPHA`]/[`SELECTED_ALPHA`] for the two faded backgrounds.
fn tag_color(color: (u8, u8, u8), alpha: u8) -> Rgba {
    let (r, g, b) = color;
    rgba(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | alpha as u32)
}

/// `Name = value` as two columns: the name fixed so values line up, the value
/// clipped rather than wrapped so every row stays one line tall.
pub(super) fn property_row(row: &PropertyRow, cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .px_2()
        .py_1()
        .gap_2()
        .text_xs()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .w(px(PROPERTY_NAME_WIDTH))
                .flex_shrink_0()
                .truncate()
                .text_color(cx.theme().muted_foreground)
                .child(SharedString::from(row.name.clone())),
        )
        .child(
            div()
                .flex_1()
                .truncate()
                .child(SharedString::from(row.value.clone())),
        )
}

/// The editable twin of [`property_row`]: the value column becomes whichever
/// widget `control` is (an `Input`, a `Checkbox`, a `ColorPicker`, a
/// `Select`, or a row of `Input`s — see [`render_editor`]) rather than the
/// read-only rbxdump-style column; a failed commit's error shows below it
/// and the old value stays in the DOM.
pub(super) fn property_row_control(
    row: &PropertyRow,
    control: impl IntoElement,
    error: Option<&str>,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .px_2()
        .py_1()
        .gap_1()
        .text_xs()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(PROPERTY_NAME_WIDTH))
                        .flex_shrink_0()
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .child(SharedString::from(row.name.clone())),
                )
                .child(div().flex_1().child(control)),
        )
        .when_some(error, |this, message| {
            this.child(
                div()
                    .text_color(cx.theme().danger)
                    .child(SharedString::from(message.to_owned())),
            )
        })
}

/// Turns a row's live widget (see `shell::edit::RowEditor`) into the element
/// [`property_row_control`] should show for it. `Bool` has no `RowEditor` —
/// its `Checkbox` is built directly where it renders, since a checkbox needs
/// no persistent entity (see `shell::panels::properties`).
pub(super) fn render_editor(editor: RowEditor, cx: &App) -> AnyElement {
    match editor {
        RowEditor::Text(input) => Input::new(&input).xsmall().into_any_element(),
        RowEditor::Fields(labels, inputs) => h_flex()
            .w_full()
            .gap_2()
            .children(labels.iter().zip(inputs).map(|(label, input)| {
                h_flex()
                    .flex_1()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(SharedString::from(*label)),
                    )
                    .child(div().flex_1().child(Input::new(&input).xsmall()))
            }))
            .into_any_element(),
        RowEditor::Color(state) => ColorPicker::new(&state).xsmall().into_any_element(),
        RowEditor::Enum(state) => Select::new(&state).xsmall().into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::tag_color;

    #[test]
    fn tag_color_keeps_the_rgb_channels_and_scales_alpha_out_of_255() {
        let color = tag_color((255, 0, 128), 255);
        assert_eq!(
            (color.r, color.g, color.b, color.a),
            (1.0, 0.0, 128.0 / 255.0, 1.0)
        );

        let faded = tag_color((255, 0, 128), 0);
        assert_eq!(faded.a, 0.0);
        assert_eq!((faded.r, faded.g, faded.b), (color.r, color.g, color.b));
    }
}
