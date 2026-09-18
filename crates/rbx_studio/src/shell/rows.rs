//! The Explorer's own rows — guide lines, indentation, states — and the
//! Properties panel's two row shapes.
//!
//! The Explorer row paints its own chrome rather than leaning on the
//! toolkit's `ListItem`: §3.5's state matrix and §3.2's hierarchy guides
//! both need control over the row's own box, and a guide line drawn inside
//! a component that adds its own padding lands at the wrong x.

use gpui_kit::assets::IconName;
use gpui_kit::component::color_picker::ColorPicker;
use gpui_kit::component::input::Input;
use gpui_kit::component::select::Select;
use gpui_kit::component::tree::TreeEntry;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::explorer::ClassIcon;
use crate::properties::PropertyRow;
use crate::tokens;

use super::edit::RowEditor;

/// §3.1 — one indent step per depth level.
const INDENT: f32 = 16.0;
/// §3.2 — the guide line sits half an indent into its own level.
const GUIDE_OFFSET: f32 = 8.0;
/// §3.3 — how far the connector reaches from the guide toward the row.
const CONNECTOR_WIDTH: f32 = 6.0;
/// §3.4 — the disc that keeps guide lines from running through a chevron.
const CHEVRON_BACKING: f32 = 14.0;
const CHEVRON_WIDTH: f32 = 14.0;
const CLASS_ICON_SIZE: f32 = 14.0;
const PROPERTY_NAME_WIDTH: f32 = 130.0;
/// Out of 255 — how strongly a tagged row's hover/selected background reads
/// against the row behind it. Selected is the stronger of the two, matching
/// the relationship `accent-soft-bg` has with its own hover step.
const HOVER_ALPHA: u8 = 46;
const SELECTED_ALPHA: u8 = 82;

/// Which ancestor levels still have a sibling below a given row, as one bit
/// per level. Computed once per render for every visible row (see
/// [`guide_mask`]) — a row can't work this out alone, since "does my
/// grandparent have another child further down" is a question about rows it
/// never sees.
pub(super) type Guides = u64;

/// Builds [`Guides`] for a whole visible tree, from each row's depth.
///
/// Walked backwards: going up the list, a level is "still open" if a row at
/// the level below it has already been seen and no shallower row has closed
/// the subtree since. That is exactly the condition for drawing a guide line
/// through a row rather than ending it there.
pub(super) fn guide_mask(depths: &[usize]) -> Vec<Guides> {
    let mut masks = vec![0; depths.len()];
    let mut open: Guides = 0;

    for (index, &depth) in depths.iter().enumerate().rev() {
        // Anything deeper than this row belongs to this row's own subtree,
        // not to a sibling of the rows above it.
        if depth + 1 < 64 {
            open &= (1 << (depth + 1)) - 1;
        }

        let mut mask = 0;
        for level in 0..depth.min(63) {
            if open & (1 << (level + 1)) != 0 {
                mask |= 1 << level;
            }
        }
        masks[index] = mask;

        if depth < 64 {
            open |= 1 << depth;
        }
    }

    masks
}

/// One instance: its hierarchy guides, its depth as indentation, a chevron
/// when it has children, the class icon (already recoloured to a tagged
/// `Folder`'s own tag, if any — see `explorer::items`) and its name.
///
/// `tint` is that same tag, reused for the row's hover and selected
/// backgrounds so a tagged folder's subtree stays visually its own.
pub(super) fn row(
    index: usize,
    entry: &TreeEntry,
    selected: bool,
    icon: ClassIcon,
    tint: Option<(u8, u8, u8)>,
    guides: Guides,
) -> AnyElement {
    let item = entry.item();
    let depth = entry.depth();
    let chevron = if entry.is_expanded() {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    };
    let class_icon = match icon {
        ClassIcon::Sprite(image) => img(image).size(px(CLASS_ICON_SIZE)).into_any_element(),
        ClassIcon::Lucide(name) => Icon::new(name).small().into_any_element(),
    };

    let (hover_bg, selected_bg) = match tint {
        Some(color) => (
            tag_color(color, HOVER_ALPHA),
            tag_color(color, SELECTED_ALPHA),
        ),
        None => (tokens::bg_2(), tokens::accent_soft_bg()),
    };

    h_flex()
        .id(index)
        .relative()
        .w_full()
        .items_center()
        .gap_x_1()
        .py_1()
        .px_2()
        .rounded(tokens::RADIUS_SM)
        .text_size(tokens::TREE_ROW_SIZE)
        .line_height(tokens::TREE_ROW_LINE_HEIGHT)
        .text_color(tokens::text_primary())
        .cursor_pointer()
        .when(selected, |this| this.bg(selected_bg))
        .when(!selected, |this| this.hover(move |this| this.bg(hover_bg)))
        .children(guide_lines(depth, guides))
        .child(
            h_flex()
                .items_center()
                .gap_1p5()
                .pl(px(depth as f32 * INDENT))
                .child(
                    div()
                        .relative()
                        .w(px(CHEVRON_WIDTH))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(entry.is_folder(), |this| {
                            // §3.4: the chevron sits on its own disc of panel
                            // colour, so a guide line passing behind it reads
                            // as passing *behind* rather than through.
                            this.child(
                                div()
                                    .absolute()
                                    .size(px(CHEVRON_BACKING))
                                    .rounded(px(CHEVRON_BACKING / 2.))
                                    .bg(tokens::bg_1()),
                            )
                            .child(Icon::new(chevron).xsmall())
                        }),
                )
                .child(class_icon)
                .child(item.label.clone()),
        )
        .into_any_element()
}

/// §3.2/§3.3 — the vertical guides this row passes through, plus the
/// connector into the row itself.
///
/// A level whose subtree continues below draws a full-height line; the level
/// this row is the last child of stops at the row's own centre, so the guide
/// visibly closes rather than running into the next unrelated branch.
fn guide_lines(depth: usize, guides: Guides) -> Vec<AnyElement> {
    let mut lines = Vec::new();

    for level in 0..depth {
        let x = px(level as f32 * INDENT + GUIDE_OFFSET);
        let continues = guides & (1 << level) != 0;
        let last_level = level + 1 == depth;

        if continues {
            lines.push(
                div()
                    .absolute()
                    .left(x)
                    .top_0()
                    .bottom_0()
                    .w(px(1.))
                    .bg(tokens::border_soft())
                    .into_any_element(),
            );
        } else if last_level {
            lines.push(
                div()
                    .absolute()
                    .left(x)
                    .top_0()
                    .h_1_2()
                    .w(px(1.))
                    .bg(tokens::border_soft())
                    .into_any_element(),
            );
        }

        if last_level {
            lines.push(
                div()
                    .absolute()
                    .left(x)
                    .top_1_2()
                    .w(px(CONNECTOR_WIDTH))
                    .h(px(1.))
                    .bg(tokens::border_soft())
                    .into_any_element(),
            );
        }
    }

    lines
}

/// A tagged `Folder`'s stored sRGB byte triplet (the same 0-255,
/// non-linear-light space `properties::color3` already displays these in) at
/// `alpha` out of 255.
fn tag_color(color: (u8, u8, u8), alpha: u8) -> Rgba {
    let (r, g, b) = color;
    rgba(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | alpha as u32)
}

/// `Name = value` as two columns: the name fixed so values line up, the value
/// clipped rather than wrapped so every row stays one line tall.
pub(super) fn property_row(row: &PropertyRow) -> impl IntoElement {
    h_flex()
        .w_full()
        .px_2()
        .py_1p5()
        .gap_2()
        .text_size(tokens::UI_LABEL_SIZE)
        .border_b_1()
        .border_color(tokens::border_soft())
        .child(
            div()
                .w(px(PROPERTY_NAME_WIDTH))
                .flex_shrink_0()
                .truncate()
                .text_color(tokens::text_secondary())
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
) -> impl IntoElement {
    v_flex()
        .w_full()
        .px_2()
        .py_1p5()
        .gap_1()
        .text_size(tokens::UI_LABEL_SIZE)
        .border_b_1()
        .border_color(tokens::border_soft())
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
                        .text_color(tokens::text_secondary())
                        .child(SharedString::from(row.name.clone())),
                )
                .child(div().flex_1().child(control)),
        )
        .when_some(error, |this, message| {
            this.child(
                div()
                    .text_color(tokens::text_error())
                    .child(SharedString::from(message.to_owned())),
            )
        })
}

/// Turns a row's live widget (see `shell::edit::RowEditor`) into the element
/// [`property_row_control`] should show for it. `Bool` has no `RowEditor` —
/// its `Checkbox` is built directly where it renders, since a checkbox needs
/// no persistent entity (see `shell::panels::properties`).
pub(super) fn render_editor(editor: RowEditor) -> AnyElement {
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
                            .text_color(tokens::text_secondary())
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
    use super::{guide_mask, tag_color};

    /// The tree this exercises, and the guides each row should draw:
    ///
    /// ```text
    ///   A          depth 0
    ///   |- B       depth 1   — level 0 continues (D is below)
    ///   |  \- C    depth 2   — level 0 continues, level 1 ends at C
    ///   \- D       depth 1   — last child: level 0 ends at D
    ///   E          depth 0
    /// ```
    #[test]
    fn a_level_keeps_its_guide_only_while_a_sibling_is_still_below() {
        let masks = guide_mask(&[0, 1, 2, 1, 0]);

        assert_eq!(masks[0], 0, "a root row draws no guides");
        assert_eq!(masks[1], 0b1, "B's parent still has D below it");
        assert_eq!(
            masks[2], 0b1,
            "C keeps level 0 (D is below) but not level 1 (B has no more children)"
        );
        assert_eq!(masks[3], 0, "D is the last child, so level 0 stops here");
        assert_eq!(masks[4], 0, "E is a root of its own");
    }

    /// A deep subtree must not leak its own levels onto the rows above it:
    /// walking backwards, everything deeper than the current row belongs to
    /// that row's descendants, not to its siblings.
    #[test]
    fn a_subtree_does_not_leak_guides_onto_the_rows_above_it() {
        // A(0) -> B(1) -> C(2), then a second root E(0).
        let masks = guide_mask(&[0, 1, 2, 0]);

        assert_eq!(masks[1], 0, "B's parent has no further children");
        assert_eq!(masks[2], 0, "nor does B, so C draws no through-lines");
    }

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
