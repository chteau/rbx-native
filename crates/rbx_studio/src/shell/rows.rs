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

/// One instance: its depth as indentation, a chevron when it has children, the
/// class icon and the instance's name; highlighted when it is the selection.
pub(super) fn row(index: usize, entry: &TreeEntry, selected: bool, icon: ClassIcon) -> ListItem {
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

    ListItem::new(index).selected(selected).child(
        h_flex()
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
            .child(item.label.clone()),
    )
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
