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

mod slider;

use crate::explorer::ClassIcon;
use std::rc::Rc;

use crate::properties::{Field, FieldKind, PropertyRow};
use crate::tokens;

use super::edit::RowEditor;
use super::explorer_edit::RowWidgets;
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
///
/// `widgets` carries the two things a row cannot build for itself — the
/// name box while it is being renamed, the `+` while it is hovered — since
/// only `shell::explorer_edit` knows which row is which.
pub(super) fn row(
    index: usize,
    entry: &TreeEntry,
    selected: bool,
    icon: ClassIcon,
    tint: Option<(u8, u8, u8)>,
    guides: Guides,
    widgets: RowWidgets,
) -> AnyElement {
    let item = entry.item();
    let depth = entry.depth();
    let chevron = if entry.is_expanded() {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    };
    let class_icon = class_icon(icon);

    let (hover_bg, selected_bg, selected_fg, selected_bar) = match tint {
        Some(color) => (
            tag_color(color, HOVER_ALPHA),
            tag_color(color, SELECTED_ALPHA),
            tokens::text(),
            tag_color(color, 0xFF),
        ),
        None => (
            tokens::hover_subtle(),
            tokens::accent_soft(),
            tokens::check_on(),
            tokens::check_on(),
        ),
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
        .rounded(tokens::radius_row())
        .text_size(tokens::text_md())
        .line_height(tokens::line_md())
        .text_color(tokens::text2())
        .cursor_pointer()
        .when(selected, |this| {
            this.bg(selected_bg)
                .text_color(selected_fg)
                .font_weight(tokens::WEIGHT_SEMIBOLD)
                // A 2 px bar down the left edge: the selection as a shape,
                // not only a wash, which is all a 12% tint over the dock
                // can't carry on its own (WCAG 1.4.11).
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .rounded_l(tokens::radius_row())
                        .bg(selected_bar),
                )
        })
        .when(!selected, |this| {
            this.hover(move |this| tokens::hover_fx(this).bg(hover_bg))
        })
        .children(guide_lines(depth, guides))
        .child(
            h_flex()
                // Takes the row's spare width so the hovered row's `+` can
                // sit at its right edge rather than immediately after a
                // short name.
                .flex_1()
                .min_w(px(0.))
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
                .child(
                    widgets
                        .name
                        .unwrap_or_else(|| div().child(item.label.clone()).into_any_element()),
                ),
        )
        .children(widgets.trailing)
        .into_any_element()
}

/// A class's identity icon at the Explorer's own size: this project's
/// rasterized kit tile, or the Lucide stand-in for a class the kit does not
/// cover (see `explorer::resolve_icon`).
///
/// Shared with the insert picker, which lists classes rather than
/// instances but has to draw them the same way — a `Part` in the tree and
/// `Part` in the picker are the same thing, and two lookups would be two
/// chances to disagree.
pub(super) fn class_icon(icon: ClassIcon) -> AnyElement {
    match icon {
        ClassIcon::Sprite(image) => img(image).size(px(CLASS_ICON_SIZE)).into_any_element(),
        ClassIcon::Lucide(name) => Icon::new(name).small().into_any_element(),
    }
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
                    .bg(tokens::border())
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
                    .bg(tokens::border())
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
                    .bg(tokens::border())
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
            .flex_none()
            .max_w(tokens::value_width())
            .truncate()
            .pr(tokens::row_padding())
            .font_family(tokens::FONT_FAMILY_MONO)
            .text_size(tokens::text_sm())
            .text_color(tokens::text_placeholder())
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
            .flex_none()
            .w(tokens::value_width())
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
    row_frame()
        .gap(tokens::label_gap())
        .px(tokens::row_padding())
        .rounded(tokens::radius())
        .text_size(tokens::text_md())
        .line_height(tokens::line_md())
        .text_color(tokens::text_strong())
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .child(
            div()
                .w_full()
                .truncate()
                // Its own name column starts where every other row's does,
                // even though this shape has nothing in it but the name.
                .pl(tokens::chevron_slot() + tokens::label_gap())
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
    let name = SharedString::from(row.name.clone());
    row_frame()
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
                .rounded(tokens::radius())
                .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                .child(
                    div()
                        .id(SharedString::from(format!("prop-name-{}", row.name)))
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .pl(name_indent(0))
                        .pr(tokens::label_gap())
                        .text_color(if read_only {
                            tokens::text_disabled()
                        } else {
                            tokens::text_muted()
                        })
                        // The longest names (`ClientAnimatorThrottlingMode`)
                        // truncate at the default dock width whatever the
                        // control's width; hovering reads them whole.
                        .tooltip(move |window, cx| super::tooltip::text(name.clone(), window, cx))
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

/// Every property row's outer box, whatever shape it takes inside: full
/// width, its own height, no separator line under it.
///
/// The reference draws no seam between rows — [`tokens::row_gap`]'s 7px
/// carries the separation instead, which is what lets a value two lines
/// tall sit next to one that is five without a line drawn hard against
/// whichever field box happens to be shorter.
pub(super) fn row_frame() -> Div {
    v_flex().w_full().flex_none()
}

/// A numeric row's name column, which is also the control that shows and
/// hides its components: the chevron, then the property's name.
///
/// The whole column is the target rather than the chevron alone — 14px of
/// icon is well under WCAG 2.5.8's floor, and a name is the obvious thing
/// to click to open what is under it.
pub(super) fn expander(
    name: &str,
    expanded: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    h_flex()
        .id(SharedString::from(format!("expand-{name}")))
        .flex_none()
        .w(tokens::row_label_width())
        .h_full()
        .min_h(tokens::row_height())
        .items_center()
        .gap(tokens::label_gap())
        .pl(tokens::row_padding())
        .pr(tokens::label_gap())
        .cursor_pointer()
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .on_click(on_click)
        .child(
            div()
                .flex_none()
                .w(tokens::chevron_slot())
                .text_color(tokens::text_label())
                .child(
                    Icon::new(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(tokens::text_xs()),
                ),
        )
        .child(
            div()
                .flex_1()
                .truncate()
                .text_color(tokens::text_muted())
                .child(SharedString::from(name.to_owned())),
        )
}

/// A numeric row: the property's name and the whole value on one line, its
/// components on their own lines underneath once the expander is open.
///
/// The summary stays editable while the components show, because it is the
/// same value in a different spelling and typing `0, 5, 0` is faster than
/// three fields — the two are kept in step by the commit itself, which
/// rebuilds the row from what the DOM ended up with (see
/// `shell::edit::Shell::commit_row`).
pub(super) fn property_expandable(
    expander: impl IntoElement,
    summary: impl IntoElement,
    fields: Option<AnyElement>,
    error: Option<&str>,
) -> impl IntoElement {
    row_frame()
        .text_size(tokens::text_md())
        .line_height(tokens::line_md())
        .text_color(tokens::text_strong())
        .child(
            h_flex()
                .w_full()
                .min_h(tokens::row_height())
                .flex_none()
                .items_center()
                .rounded(tokens::radius())
                .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                .child(expander)
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .pr(tokens::row_padding())
                        .child(summary),
                ),
        )
        .children(fields)
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

/// The reference's pill toggle: a track at [`tokens::toggle_width`] by
/// [`tokens::toggle_height`], filled with the accent and a right-parked
/// thumb when on, [`tokens::field_select`] and a left-parked one when not.
///
/// The unticked track still **outlines** — [`tokens::check_off_border`],
/// exactly as the square checkbox this replaced did, and for the same
/// reason: a borderless track this close in tone to the field surface
/// around it fails WCAG 1.4.11 for a control whose entire job is to show a
/// state. The click target stays [`tokens::checkbox_target`], the same
/// WCAG 2.5.8 floor a visually smaller pill doesn't get to shrink.
///
/// `None` is the indeterminate state a multi-selection's disagreeing
/// values show: filled like an on track, since it is not an empty one, its
/// thumb parked in the middle rather than at either end.
pub(super) fn checkbox(
    id: impl Into<ElementId>,
    checked: impl Into<Option<bool>>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let checked = checked.into();
    div()
        .id(id.into())
        // The target, which never goes under 24px …
        .flex_none()
        .h(tokens::checkbox_target())
        .min_w(tokens::checkbox_target())
        .flex()
        .items_center()
        // … left-aligned inside it, so the track's own left edge lands on
        // the same line as every field box in the column. Centring it in a
        // larger target inset it by half the difference, which read as the
        // toggle rows being indented relative to the rest.
        .justify_start()
        .cursor_pointer()
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .on_click(on_click)
        .child(toggle_pill(checked))
}

/// The pill alone, [`tokens::toggle_width`] by [`tokens::toggle_height`]:
/// the accent with a right-parked thumb when on, [`tokens::field_select`]
/// outlined with a left-parked one when off, the thumb in the middle for
/// `None`. [`checkbox`] wraps it in its click target; a row that is its
/// own target draws just this.
pub(super) fn toggle_pill(checked: Option<bool>) -> Div {
    let on = checked != Some(false);
    let inset = (tokens::toggle_height() - tokens::toggle_thumb()) / 2.;
    div()
        .relative()
        .flex_none()
        .w(tokens::toggle_width())
        .h(tokens::toggle_height())
        .rounded_full()
        .map(|this| {
            if on {
                this.bg(tokens::check_on())
            } else {
                this.bg(tokens::field_select())
                    .border(px(1.))
                    .border_color(tokens::check_off_border())
            }
        })
        .child(
            div()
                .absolute()
                // The off track wears a 1px border and offsets are measured
                // inside it, so the knob's own offsets drop by one there to
                // land 2px from the outer edge, the same as the on state's.
                .map(|this| match checked {
                    Some(false) => this.top(inset - px(1.)).left(inset - px(1.)),
                    Some(true) => this.top(inset).right(inset),
                    None => this
                        .top(inset)
                        .left(relative(0.5))
                        .ml(-(tokens::toggle_thumb() / 2.)),
                })
                .flex_none()
                .size(tokens::toggle_thumb())
                .rounded_full()
                .bg(if on {
                    tokens::knob()
                } else {
                    tokens::check_off_border()
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
        .rounded(tokens::radius_row())
        .text_size(tokens::text_xs())
        .line_height(tokens::line_xs())
        .font_weight(tokens::WEIGHT_BOLD)
        .text_color(tokens::text3())
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .on_click(on_click)
        .child(
            div()
                .flex_none()
                // The same slot an expander's chevron sits in, so a
                // category's name starts on the property names' own edge.
                .w(tokens::chevron_slot())
                .text_color(tokens::text3())
                .child(
                    Icon::new(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(tokens::text_xs()),
                ),
        )
        .child(div().flex_1().truncate().child(label.to_uppercase()))
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

/// A sequence row's click: it opens `crate::sequence_window` rather than
/// committing anything, so it is a plain handler rather than one of
/// [`OnScrub`]'s per-field factories.
pub(super) type OnOpen = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

///
/// `stops` is the window's own Tab order: the two editors that are whole
/// widgets rather than toolkit elements — `Select` for an enum, `ColorPicker`
/// for a `Color3` — take no `tab_index`, so they can only be reached by
/// recording their focus handle here, at the point in paint order the row is
/// built. That has to happen per row per render, unlike the graphics-quality
/// dropdown's one-off registration in `Shell::quality_control`, because a
/// property row's widget is rebuilt whenever the selection changes.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_editor(
    tab_index: isize,
    stops: &TabOrder,
    editor: RowEditor,
    on_flag: impl Fn(usize, bool) -> Box<dyn Fn(&ClickEvent, &mut Window, &mut App)> + 'static,
    on_scrub: OnScrub,
    on_open: OnOpen,
    window: &Window,
    cx: &mut App,
) -> AnyElement {
    render_row_editor(
        tab_index, stops, editor, &on_flag, on_scrub, on_open, window, cx,
    )
}

/// One flag's click handler, by its index and the value it currently shows.
///
/// A borrowed trait object rather than [`render_editor`]'s own generic: an
/// `EditKind::Optional` draws the editor nested inside it, and a generic
/// function that calls itself with a *different* closure type has no bottom
/// to its monomorphization.
type OnFlag<'a> = &'a dyn Fn(usize, bool) -> Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[allow(clippy::too_many_arguments)]
fn render_row_editor(
    tab_index: isize,
    stops: &TabOrder,
    editor: RowEditor,
    on_flag: OnFlag<'_>,
    on_scrub: OnScrub,
    on_open: OnOpen,
    window: &Window,
    cx: &mut App,
) -> AnyElement {
    match editor {
        // The row *is* the preview: a `ColorSequence`'s ramp or a
        // `NumberSequence`'s curve, drawn at row height and clicked to open
        // the graph that edits it (`crate::sequence_window`). A sequence has
        // more numbers than a row has width and they are the wrong numbers
        // to type, so this replaces the field rather than sitting beside
        // one. Unparseable text draws nothing rather than an empty box —
        // the only way to reach that is a sequence some other tool wrote
        // that Roblox's own constructors would reject too.
        RowEditor::Sequence { color, text } => {
            let stops = crate::properties::edit::sequence_value(color, &text)
                .and_then(|value| crate::sequence_editor::Editor::open(&value))
                .map(|editor| (editor.kind, editor.stops.clone(), editor.ceiling()));
            div()
                // Keyed by the row's own tab stop: a `UIGradient` shows two
                // sequence rows at once, and two elements sharing an id are
                // one element as far as click dispatch is concerned.
                .id(("sequence-preview", tab_index as u64))
                .w_full()
                .h(tokens::input_height())
                .rounded(px(4.))
                .overflow_hidden()
                .bg(tokens::black())
                .cursor_pointer()
                .tab_index(tab_index)
                .hover(|this| tokens::hover_fx(this).border_color(tokens::check_on()))
                .border_1()
                .border_color(tokens::border())
                .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
                .on_click(on_open)
                .children(stops.map(|(kind, stops, ceiling)| {
                    crate::sequence_window::preview(kind, stops, ceiling)
                }))
                .into_any_element()
        }
        RowEditor::Text(input) => text_field(&input, tab_index, cx).into_any_element(),
        // Rail first, number second: the drag is the reason the row looks
        // like this, and the field is what it settles into. The field
        // keeps a fixed width so the rails of a `Lighting` all end on the
        // same edge however long the numbers beside them get.
        RowEditor::Slider(input, rail) => h_flex()
            .w_full()
            .items_center()
            .gap(tokens::label_gap())
            .child(slider::slider(&rail, cx))
            .child(
                div()
                    .flex_none()
                    // Narrower than a labelled field (`field_min_width`):
                    // there is no label in front of this one, and every
                    // value a rail spans is a handful of digits.
                    .w(tokens::scaled_width(52.))
                    .child(text_field(&input, tab_index, cx)),
            )
            .into_any_element(),
        // Captioned lines — a `CFrame`'s Position over its Orientation.
        // Each group is its own run of field rows under its own caption,
        // which is what makes six numbers readable where one flat run of
        // six is a wall.
        RowEditor::Groups(groups, _, inputs) => {
            let mut taken = 0;
            v_flex()
                .w_full()
                .children(groups.iter().map(|group| {
                    let mine = &inputs[taken..taken + group.fields.len()];
                    taken += group.fields.len();
                    v_flex()
                        .w_full()
                        // The caption is a row of its own rather than a
                        // heading over a block: it sits in the same name
                        // column its fields do, one level in, so a
                        // `CFrame` reads as Position and Orientation each
                        // owning the three lines under it.
                        .child(
                            h_flex()
                                .w_full()
                                .min_h(tokens::row_height())
                                .items_center()
                                .py(tokens::label_gap())
                                .border_b_1()
                                .border_color(tokens::border())
                                .child(
                                    div()
                                        .flex_none()
                                        .w(tokens::row_label_width())
                                        .truncate()
                                        .pl(name_indent(1))
                                        .pr(tokens::label_gap())
                                        .text_color(tokens::text_label())
                                        .child(group.caption),
                                ),
                        )
                        .child(number_fields(
                            group.fields,
                            mine,
                            taken - group.fields.len(),
                            tab_index,
                            on_scrub.clone(),
                            2,
                            cx,
                        ))
                }))
                .into_any_element()
        }
        RowEditor::Fields(fields, _, inputs) => {
            number_fields(fields, &inputs, 0, tab_index, on_scrub, 1, cx).into_any_element()
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
                    // An optional's inner editor is a numeric one — a
                    // `CFrame`'s six fields or a `PhysicalProperties`'
                    // five, the only two `Variant`s shaped this way — never
                    // a flag set, so nothing below ever reaches `on_flag`.
                    render_row_editor(
                        tab_index,
                        stops,
                        *inner,
                        &|_, _| Box::new(|_, _, _| {}),
                        on_scrub,
                        on_open,
                        window,
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
            let handle = state.read(cx).focus_handle(cx);
            stops.register(&handle);
            select_field(&handle, window, cx)
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

/// One labelled numeric field per line, each laid out as the property row
/// above it is: the component's name in the same name column, its input in
/// the same value column.
///
/// They used to share one wrapping line. Side by side, three fields in a
/// 150px column left each one 16px wide with 8px of padding on either side
/// — no room for a digit — and wrapping them fixed the width at the cost of
/// a value column whose left edge moved from row to row. Stacking gets both:
/// full-width fields, and one edge every input in the panel starts at.
///
/// `depth` is how far in the names sit — 1 under a property's own expander,
/// 2 under a captioned group inside it.
fn number_fields(
    fields: &'static [Field],
    inputs: &[Entity<InputState>],
    offset: usize,
    tab_index: isize,
    on_scrub: OnScrub,
    depth: usize,
    cx: &App,
) -> Div {
    v_flex()
        .w_full()
        .children(
            fields
                .iter()
                .zip(inputs)
                .enumerate()
                .map(|(index, (field, input))| {
                    let draggable = field.kind.step_per_pixel().is_some();
                    h_flex()
                        .w_full()
                        .min_h(tokens::row_height())
                        .items_center()
                        // Its own seam, for the same reason the rows above
                        // it have one: a column of 31px field boxes with
                        // nothing between them is one grey block, not a
                        // list of values.
                        .py(tokens::label_gap())
                        .border_b_1()
                        .border_color(tokens::border())
                        .rounded(tokens::radius())
                        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                        // The *label* is the drag handle, not the field: that is
                        // what leaves a plain click on the field meaning "put the
                        // caret here", and it is where every other tool with this
                        // gesture puts it. It is the whole name column rather
                        // than the word, which is what takes the handle over
                        // WCAG 2.5.8's target floor.
                        .child(
                            div()
                                .id(SharedString::from(format!("scrub-{}-{index}", field.label)))
                                .flex_none()
                                .w(tokens::row_label_width())
                                .truncate()
                                .pl(name_indent(depth))
                                .pr(tokens::label_gap())
                                .text_color(tokens::text_muted())
                                .when(draggable, |this| {
                                    this.cursor_col_resize()
                                        .hover(|this| {
                                            tokens::hover_fx(this).text_color(tokens::text_full())
                                        })
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            on_scrub(offset + index, field.kind),
                                        )
                                })
                                .child(field.label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .overflow_hidden()
                                .pr(tokens::row_padding())
                                .child(text_field(input, tab_index, cx)),
                        )
                }),
        )
}

/// Where a name sits in the name column: past the row's own padding and the
/// column an expander chevron occupies, then one more step per level of
/// nesting. A row with no chevron still clears the slot, so every property
/// name in the panel starts on the same edge whether or not it has
/// components to open.
pub(super) fn name_indent(depth: usize) -> Pixels {
    tokens::row_padding() + (tokens::chevron_slot() + tokens::label_gap()) * (depth as f32 + 1.)
}

/// One `Input` in the panel's field box — the value column's whole content
/// for a scalar row, a summary, or one component.
pub(super) fn text_field(input: &Entity<InputState>, tab_index: isize, cx: &App) -> Div {
    let handle = input.read(cx).focus_handle(cx);
    field_box()
        .track_focus(&handle)
        .focus(|this| this.border_color(tokens::accent_line()))
        .child(
            Input::new(input)
                .appearance(false)
                .with_size(tokens::field_size())
                .h_full()
                // Without an index a toolkit input keeps the default 0 and
                // sorts ahead of every region in the window — a property field
                // reached before the menu bar.
                .tab_index(tab_index),
        )
}

/// The `InputsStyle` frame's field: [`tokens::field_select`], `RADIUS`, 8px
/// of horizontal padding, 31px tall, and **no border** — the surface change
/// is the whole affordance. 31px also clears WCAG 2.5.8's 24x24 target
/// floor without any help from the spacing exception.
pub(super) fn field_box() -> Div {
    field_surface(tokens::field_select())
}

/// The same box, on the surface a dropdown wears (see
/// [`tokens::field_select`]).
pub(super) fn select_box() -> Div {
    field_surface(tokens::field_select())
}

/// A dropdown's box, ringed while keyboard focus is on `handle` — the
/// toolkit `Select` inside it, or the box itself where the box is the
/// control. The toolkit draws no ring once its own chrome is off, so
/// without this a select took focus invisibly. Inset, like the Viewport
/// dock's quality select: a property's value column clips anything drawn
/// outside it, and a shadow moves nothing, so the column stays aligned.
pub(super) fn select_field(handle: &FocusHandle, window: &Window, cx: &App) -> Div {
    let ringed = handle.contains_focused(window, cx) && window.last_input_was_keyboard();
    select_box().when(ringed, |this| this.shadow(tokens::focus_ring_inset()))
}

fn field_surface(surface: Rgba) -> Div {
    h_flex()
        .w_full()
        .h(tokens::input_height())
        .items_center()
        .overflow_hidden()
        .px(tokens::input_padding())
        .rounded(tokens::radius())
        .bg(surface)
        .border_1()
        .border_color(tokens::border())
}
