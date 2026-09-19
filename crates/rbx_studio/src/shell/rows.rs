//! The Explorer's own rows — guide lines, indentation, states — and the
//! Properties panel's two row shapes.
//!
//! The Explorer row paints its own chrome rather than leaning on the
//! toolkit's `ListItem`: §3.5's state matrix and §3.2's hierarchy guides
//! both need control over the row's own box, and a guide line drawn inside
//! a component that adds its own padding lands at the wrong x.

use gpui_kit::assets::IconName;
use gpui_kit::component::color_picker::ColorPicker;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::select::Select;
use gpui_kit::component::tree::TreeEntry;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::explorer::ClassIcon;
use std::rc::Rc;

use crate::properties::{Field, FieldKind, PropertyRow};
use crate::tokens;

use super::edit::RowEditor;
use super::roving::TabOrder;

/// One indent step per depth level.
const INDENT: f32 = 12.0;
/// The guide line sits half an indent into its own level.
const GUIDE_OFFSET: f32 = 6.0;
/// How far the connector reaches from the guide toward the row.
const CONNECTOR_WIDTH: f32 = 5.0;
const CHEVRON_WIDTH: f32 = 12.0;
const CLASS_ICON_SIZE: f32 = 12.0;
/// Out of 255 — how strongly a tagged row's hover/selected background reads
/// against the row behind it. Selected is the stronger of the two, matching
/// the relationship the untagged selection has with its own hover step.
const HOVER_ALPHA: u8 = 20;
const SELECTED_ALPHA: u8 = 89;

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
        None => (tokens::hover(), tokens::selection()),
    };

    h_flex()
        .id(index)
        .relative()
        .w_full()
        .h(tokens::tree_row_height())
        .flex_none()
        .items_center()
        .gap_x_1()
        .px(px(4.))
        .rounded(tokens::RADIUS_TINY)
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .text_color(tokens::text_strong())
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
                        // No disc behind the chevron. It was there so a
                        // guide line could pass *behind* it rather than
                        // through — but a filled circle on every folder row
                        // is a lot of visual noise to buy one hairline's
                        // correctness, and the guides are quiet enough now
                        // that the collision does not read.
                        .when(entry.is_folder(), |this| {
                            this.child(Icon::new(chevron).xsmall())
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
                    .bg(tokens::divider())
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
                    .bg(tokens::divider())
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
                    .bg(tokens::divider())
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

/// `Name = value` as two columns: a fixed name column so every value in a
/// section starts at the same x, and a value column that clips rather than
/// wraps so every row stays exactly one line tall.
///
/// A read-only property dims *both* columns, not just the value — the name
/// of something you cannot set is as unavailable as the setting is.
pub(super) fn property_row(row: &PropertyRow) -> impl IntoElement {
    property_shell(
        row,
        true,
        // `text_muted`, not the dimmest step in the ramp: a read-only
        // property's *value* is real data — `SecurityCapabilities(0x0)`, a
        // `HistoryId` — and dimming it to match its dimmed label put actual
        // content at 1.7:1. Only the name reads as unavailable.
        div()
            .flex_1()
            .truncate()
            .pr(tokens::row_padding())
            .text_color(tokens::text_muted())
            .child(SharedString::from(row.value.clone())),
        None,
    )
}

/// The editable twin of [`property_row`]: the value column becomes whichever
/// widget `control` is (an `Input`, a `Checkbox`, a `ColorPicker`, a
/// `Select`, or a row of `Input`s — see [`render_editor`]) rather than the
/// read-only text column; a failed commit's error shows below it and the old
/// value stays in the DOM.
pub(super) fn property_row_control(
    row: &PropertyRow,
    control: impl IntoElement,
    composite: bool,
    error: Option<&str>,
) -> impl IntoElement {
    if composite {
        return property_stack(row, control, error).into_any_element();
    }
    property_shell(
        row,
        false,
        div()
            .flex_1()
            .overflow_hidden()
            .pr(tokens::row_padding())
            .child(control),
        error,
    )
    .into_any_element()
}

/// A composite value's row: the property's name on its own line, its fields
/// beneath at the dock's full width.
fn property_stack(
    row: &PropertyRow,
    control: impl IntoElement,
    error: Option<&str>,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .flex_none()
        .gap(tokens::label_gap())
        .px(tokens::row_padding())
        .py(tokens::label_gap())
        .rounded(tokens::RADIUS)
        .text_size(tokens::text_md())
        .line_height(tokens::line_md())
        .text_color(tokens::text_strong())
        .hover(|this| this.bg(tokens::hover()))
        .child(
            div()
                .w_full()
                .truncate()
                .text_color(tokens::text_muted())
                .child(SharedString::from(row.name.clone())),
        )
        .child(control)
        .when_some(error, |this, message| {
            this.child(
                div()
                    .text_size(tokens::text_sm())
                    .line_height(tokens::line_sm())
                    .text_color(tokens::text_error())
                    .child(SharedString::from(message.to_owned())),
            )
        })
}

/// The row both shapes share: name column, value column, and — only when a
/// commit has just failed — the reason underneath.
///
/// The names are **left-aligned** on a common edge. Right alignment binds a
/// label to its field more tightly and was tried first, but it leaves a
/// ragged left edge that makes a long list of properties hard to scan —
/// which is what this panel is mostly used for. The binding is carried
/// instead by the gaps: `label_gap` separates a label from its own input,
/// and `row_gap`, twice as large, separates one row from the next.
fn property_shell(
    row: &PropertyRow,
    read_only: bool,
    value: impl IntoElement,
    error: Option<&str>,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .flex_none()
        .text_size(tokens::text_md())
        .line_height(tokens::line_md())
        .text_color(tokens::text_strong())
        .child(
            h_flex()
                .w_full()
                // `min_h`, not `h`: a wrapped composite value (see
                // `render_editor`) is two or three lines tall, and a fixed
                // height would clip it.
                .min_h(tokens::row_height())
                .flex_none()
                .items_center()
                .rounded(tokens::RADIUS)
                .hover(|this| this.bg(tokens::hover()))
                .child(
                    div()
                        .flex_none()
                        .w(tokens::row_label_width())
                        .truncate()
                        .pl(tokens::row_padding())
                        .pr(tokens::label_gap())
                        .text_color(if read_only {
                            tokens::text_disabled()
                        } else {
                            tokens::text_muted()
                        })
                        .child(SharedString::from(row.name.clone())),
                )
                .child(value),
        )
        .when_some(error, |this, message| {
            this.child(
                div()
                    .pl(tokens::row_label_width())
                    .pr(tokens::row_padding())
                    .pb(tokens::label_gap())
                    .text_size(tokens::text_sm())
                    .line_height(tokens::line_sm())
                    .text_color(tokens::text_error())
                    .child(SharedString::from(message.to_owned())),
            )
        })
}

/// The `InputsStyle` frame's checkbox: a 26px square at [`tokens::RADIUS`],
/// filled with the accent when ticked and with [`tokens::chrome`] when not.
///
/// Two things it does that the frame doesn't. It **outlines** the unticked
/// state — the frame's borderless `#111` box is 1.11:1 against a black
/// dock, which fails WCAG 1.4.11 for a control whose entire job is to show
/// a state. And it is **26px**, which is the frame's own number and, not by
/// coincidence, over WCAG 2.5.8's 24x24 target floor; the 10px box this
/// used to draw was less than a fifth of the required area.
pub(super) fn checkbox(
    id: impl Into<ElementId>,
    checked: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id.into())
        // The target, which never goes under 24px …
        .flex_none()
        .size(tokens::checkbox_target())
        .flex()
        .items_center()
        // … left-aligned inside it, so the box's own left edge lands on the
        // same line as every field box in the column. Centring the box in a
        // larger target inset it by half the difference, which read as the
        // checkbox rows being indented relative to the rest.
        .justify_start()
        .cursor_pointer()
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .on_click(on_click)
        .child(
            // … and the box, which is smaller on purpose.
            div()
                .flex_none()
                .size(tokens::checkbox_size())
                .flex()
                .items_center()
                .justify_center()
                .rounded(tokens::RADIUS)
                .map(|this| {
                    if checked {
                        this.bg(tokens::check_on())
                    } else {
                        this.bg(tokens::check_off())
                            .border(px(1.))
                            .border_color(tokens::check_off_border())
                    }
                })
                .when(checked, |this| {
                    this.text_color(tokens::black())
                        .child(Icon::new(IconName::Check).size(tokens::text_xs()))
                }),
        )
}

/// A property section's header: the frame's chevron-and-name strip, which
/// is also the control that collapses the section.
pub(super) fn section_header(
    label: SharedString,
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    h_flex()
        .id(SharedString::from(format!("section-{label}")))
        .w_full()
        .h(tokens::section_height())
        .flex_none()
        .items_center()
        .gap(tokens::label_gap())
        .px(tokens::row_padding())
        .cursor_pointer()
        // One step above the dock, and the only filled thing in the panel
        // that is not an input: a category is a tile you click, so it looks
        // like a surface rather than a line of text floating on the dock.
        // Rounded because everything else that carries a fill here is.
        .bg(tokens::chrome())
        .rounded(tokens::RADIUS)
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .font_weight(tokens::WEIGHT_BOLD)
        // The brightest step in the ramp, used nowhere else in the panel:
        // a category is not a property, and on a neutral palette weight and
        // brightness are the only axes left to say so — the one saturated
        // colour in the design is spent on the accent and stays there.
        .text_color(tokens::text_full())
        .hover(|this| this.bg(tokens::hover()))
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .on_click(on_click)
        .child(
            div().flex_none().text_color(tokens::text_label()).child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(tokens::text_xs()),
            ),
        )
        .child(div().flex_1().truncate().child(label))
}

/// Turns a row's live widget (see `shell::edit::RowEditor`) into the element
/// [`property_row_control`] should show for it. `Bool` has no `RowEditor` —
/// its `Checkbox` is built directly where it renders, since a checkbox needs
/// no persistent entity (see `shell::panels::properties`).
/// Builds the mouse-down handler that starts a scrub on one field — see
/// `shell::scrub`. Taken as a factory rather than a closure per field so the
/// row can capture the property's name once.
pub(super) type OnScrub =
    Rc<dyn Fn(usize, FieldKind) -> Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>>;

///
/// `stops` is the window's own Tab order: the two editors that are whole
/// widgets rather than toolkit elements — `Select` for an enum, `ColorPicker`
/// for a `Color3` — take no `tab_index`, so they can only be reached by
/// recording their focus handle here, at the point in paint order the row is
/// built. That has to happen per row per render, unlike the graphics-quality
/// dropdown's one-off registration in `Shell::quality_control`, because a
/// property row's widget is rebuilt whenever the selection changes.
pub(super) fn render_editor(
    tab_index: isize,
    stops: &TabOrder,
    editor: RowEditor,
    on_flag: impl Fn(usize, bool) -> Box<dyn Fn(&ClickEvent, &mut Window, &mut App)> + 'static,
    on_scrub: OnScrub,
    cx: &mut App,
) -> AnyElement {
    render_row_editor(tab_index, stops, editor, &on_flag, on_scrub, cx)
}

/// One flag's click handler, by its index and the value it currently shows.
///
/// A borrowed trait object rather than [`render_editor`]'s own generic: an
/// `EditKind::Optional` draws the editor nested inside it, and a generic
/// function that calls itself with a *different* closure type has no bottom
/// to its monomorphization.
type OnFlag<'a> = &'a dyn Fn(usize, bool) -> Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

fn render_row_editor(
    tab_index: isize,
    stops: &TabOrder,
    editor: RowEditor,
    on_flag: OnFlag<'_>,
    on_scrub: OnScrub,
    cx: &mut App,
) -> AnyElement {
    match editor {
        RowEditor::Text(input) => field_box()
            .child(
                Input::new(&input)
                    .appearance(false)
                    .with_size(tokens::field_size())
                    .h_full()
                    // Without an index a toolkit input keeps the default 0
                    // and sorts ahead of every region in the window — a
                    // property field reached before the menu bar.
                    .tab_index(tab_index),
            )
            .into_any_element(),
        // A composite value — a CFrame's nine numbers, a Vector3's three —
        // **wraps** rather than dividing the value column by however many
        // fields there are. Three fields in a 150px column left each one
        // 16px wide with 8px of padding on either side, i.e. no room for a
        // digit: the cells rendered empty, and were also under WCAG 2.5.8's
        // target floor on both size and spacing. A wrapped row is taller
        // and readable, which is the right trade in an inspector.
        // Captioned lines — a `CFrame`'s Position over its Orientation.
        // Each group is its own row of fields under its own small caption,
        // which is what makes six numbers readable where one flat run of
        // six is a wall.
        RowEditor::Groups(groups, inputs) => {
            let mut taken = 0;
            v_flex()
                .w_full()
                .gap(tokens::row_gap())
                .children(groups.iter().map(|group| {
                    let mine = &inputs[taken..taken + group.fields.len()];
                    taken += group.fields.len();
                    v_flex()
                        .w_full()
                        .gap(tokens::label_gap())
                        .child(
                            div()
                                .text_size(tokens::text_sm())
                                .line_height(tokens::line_sm())
                                .text_color(tokens::text_label())
                                .child(group.caption),
                        )
                        .child(number_fields(
                            group.fields,
                            mine,
                            taken - group.fields.len(),
                            tab_index,
                            on_scrub.clone(),
                        ))
                }))
                .into_any_element()
        }
        RowEditor::Fields(fields, inputs) => {
            number_fields(fields, &inputs, 0, tab_index, on_scrub).into_any_element()
        }
        // The checkbox is the only control an absent value has: there is
        // nothing to edit until it says there is a value. Present, it reads
        // as a clear, and the editor for the value itself sits under it.
        //
        // The caption travels with the value rather than being a constant
        // here, because the two types shaped this way mean different things
        // by the box — "Has value" for an `OptionalCFrame`, "Custom" for a
        // `PhysicalProperties` (see `properties::EditKind::Optional`).
        RowEditor::Optional(present, label, inner) => {
            let toggle = on_flag(0, present);
            v_flex()
                .w_full()
                .gap(tokens::row_gap())
                .child(
                    h_flex()
                        .flex_none()
                        .items_center()
                        .gap(tokens::label_gap())
                        .child(checkbox(
                            SharedString::from("optional-present"),
                            present,
                            toggle,
                        ))
                        .child(
                            div()
                                .flex_none()
                                .text_color(tokens::text_muted())
                                .child(label),
                        ),
                )
                .children(present.then(|| {
                    // An optional's inner editor is a numeric one — the only
                    // `Variant` shaped this way is `OptionalCFrame` — never a
                    // flag set, so nothing below ever reaches `on_flag`.
                    render_row_editor(
                        tab_index,
                        stops,
                        *inner,
                        &|_, _| Box::new(|_, _, _| {}),
                        on_scrub,
                        cx,
                    )
                }))
                .into_any_element()
        }
        // A bit set is a row of checkboxes, because that is what it is:
        // six independent yes/no answers, not one value with sixty-four
        // spellings. Each click commits the whole set, since the DOM has no
        // "set only this side" write.
        RowEditor::Flags(labels, values) => h_flex()
            .w_full()
            .flex_wrap()
            .gap(tokens::row_gap())
            .children(
                labels
                    .iter()
                    .zip(values)
                    .enumerate()
                    .map(|(index, (label, checked))| {
                        h_flex()
                            .flex_none()
                            .min_w(tokens::field_min_width())
                            .items_center()
                            .gap(tokens::label_gap())
                            .child(checkbox(
                                SharedString::from(format!("flag-{label}")),
                                checked,
                                on_flag(index, checked),
                            ))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(tokens::text_muted())
                                    .child(SharedString::from(*label)),
                            )
                    }),
            )
            .into_any_element(),
        RowEditor::Color(state) => {
            stops.register(&state.read(cx).focus_handle(cx));
            ColorPicker::new(&state)
                .with_size(tokens::field_size())
                .into_any_element()
        }
        // `appearance(false)` drops the toolkit's own border and fill but
        // keeps its chevron, so this ends up as the frame's Select exactly:
        // one container, one 15px chevron.
        // `h_full` so the select fills the box and centres its own text in
        // it. Left to itself the toolkit renders a select at its own fixed
        // step height and aligns the text to the top of *that*, which reads
        // as the whole control sitting a few pixels high in its field.
        RowEditor::Enum(state) => {
            stops.register(&state.read(cx).focus_handle(cx));
            select_box()
                .child(
                    Select::new(&state)
                        .appearance(false)
                        .with_size(tokens::field_size())
                        .h_full()
                        .py_0()
                        .pt(tokens::select_inset()),
                )
                .into_any_element()
        }
    }
}

/// A wrapping run of labelled numeric fields.
///
/// A composite value — a `CFrame`'s numbers, a `Vector3`'s three — **wraps**
/// rather than dividing the column by however many fields there are. Three
/// fields in a 150px column left each one 16px wide with 8px of padding on
/// either side, i.e. no room for a digit: the cells rendered empty, and were
/// also under WCAG 2.5.8's target floor on both size and spacing.
fn number_fields(
    fields: &'static [Field],
    inputs: &[Entity<InputState>],
    offset: usize,
    tab_index: isize,
    on_scrub: OnScrub,
) -> Div {
    h_flex()
        .w_full()
        .flex_wrap()
        .gap(tokens::label_gap())
        .children(
            fields
                .iter()
                .zip(inputs)
                .enumerate()
                .map(|(index, (field, input))| {
                    let draggable = field.kind.step_per_pixel().is_some();
                    h_flex()
                        .flex_none()
                        .min_w(tokens::field_min_width())
                        .items_center()
                        .gap(tokens::label_gap())
                        // The *label* is the drag handle, not the field: that is
                        // what leaves a plain click on the field meaning "put the
                        // caret here", and it is where every other tool with this
                        // gesture puts it.
                        .child(
                            div()
                                .id(SharedString::from(format!("scrub-{}-{index}", field.label)))
                                .flex_none()
                                .text_color(tokens::text_muted())
                                .when(draggable, |this| {
                                    this.cursor_col_resize()
                                        .hover(|this| this.text_color(tokens::text_full()))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            on_scrub(offset + index, field.kind),
                                        )
                                })
                                .child(field.label),
                        )
                        .child(
                            field_box().flex_1().child(
                                Input::new(input)
                                    .appearance(false)
                                    .with_size(tokens::field_size())
                                    .h_full()
                                    .tab_index(tab_index),
                            ),
                        )
                }),
        )
}

/// The `InputsStyle` frame's field: [`tokens::chrome`], 3px radius, 8px of
/// horizontal padding, 31px tall, and **no border** — the surface change is
/// the whole affordance. 31px also clears WCAG 2.5.8's 24x24 target floor
/// without any help from the spacing exception.
pub(super) fn field_box() -> Div {
    field_surface(tokens::chrome())
}

/// The same box, on the surface a dropdown wears (see
/// [`tokens::field_select`]).
pub(super) fn select_box() -> Div {
    field_surface(tokens::field_select())
}

fn field_surface(surface: Rgba) -> Div {
    h_flex()
        .w_full()
        .h(tokens::input_height())
        .items_center()
        .overflow_hidden()
        .px(tokens::input_padding())
        .rounded(tokens::RADIUS)
        .bg(surface)
}
