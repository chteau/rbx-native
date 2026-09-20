//! The panel's chrome: the card, the plot inside it, and the footer that
//! reads and edits whichever stop is selected.
//!
//! Every colour, radius and size here comes from `crate::tokens` — see that
//! module's own rule about inventing one.

use gpui_kit::component::color_picker::ColorPicker;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::sequence_editor::Kind;
use crate::tokens;

use super::paint::{self, Look};
use super::{field_text, sequence_fields, Shell};

/// The card's width, and the plot's height inside it. Wide enough that a
/// keypoint at time 0.05 and one at 0.1 are separable by eye and by pointer
/// — the whole reason this is not a text field.
const CARD_WIDTH: f32 = 560.0;
const CURVE_HEIGHT: f32 = 220.0;
/// A ramp needs far less height than a curve: it carries one dimension.
const RAMP_HEIGHT: f32 = 56.0;
/// Room under the ramp for the stop markers, which hang below it.
const MARKER_ROOM: f32 = 16.0;
const CARD_RADIUS: f32 = 8.0;

impl Shell {
    /// The open editor, laid over the docks. `None` when nothing is open,
    /// which is what keeps this out of the element tree entirely rather
    /// than leaving an invisible layer to hit-test against.
    pub(in crate::shell) fn sequence_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        // The value as the DOM holds it this frame — see the module doc for
        // why the panel keeps no copy of it.
        let editor = self.sequence_editor()?;
        let kind = editor.kind;

        // Seeding the footer's fields here, rather than where a drag or a
        // click changes the selection, is the same discipline
        // `shell::edit::resync_row_widget` follows: a render is the one
        // place that holds both the current value and a `Window`.
        let seeds: Vec<(Entity<gpui_kit::component::input::InputState>, String)> = self
            .sequence
            .as_ref()?
            .fields
            .iter()
            .zip(sequence_fields(kind))
            .map(|(input, (field, _))| (input.clone(), field_text(&editor, *field)))
            .collect();
        for (input, seed) in &seeds {
            super::super::edit::resync_field(input, seed, window, cx);
        }

        let open = self.sequence.as_ref()?;
        let stops = editor.stops.clone();
        let ceiling = editor.ceiling();
        let selected = editor.selected;
        let title = open.title.clone();
        let close_focus = self.tab_order.claim(cx);

        let card = v_flex()
            .id("sequence-editor")
            // The card blocks the mouse so a click on it is not also a
            // click on the dock underneath — which also means the window's
            // own move/up listeners (`Render for Shell`) stop seeing a drag
            // the moment the pointer crosses onto the card. The same two
            // handlers therefore live here as well, so a keypoint drag
            // survives wherever it goes.
            .occlude()
            .on_mouse_move(cx.listener(|shell, event: &MouseMoveEvent, _, cx| {
                shell.drag_sequence(event.position, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|shell, _: &MouseUpEvent, _, _| shell.end_sequence_drag()),
            )
            .w(tokens::scaled_width(CARD_WIDTH))
            .bg(tokens::chrome())
            .rounded(px(CARD_RADIUS))
            .border_1()
            .border_color(tokens::divider())
            .shadow(tokens::elevation())
            .child(super::super::chrome::panel_topbar(
                SharedString::from(title),
                &close_focus,
                cx.listener(|shell, _, _, cx| shell.close_sequence_editor(cx)),
            ))
            .child(
                div().p(tokens::panel_padding()).child(
                    div()
                        .id("sequence-plot")
                        .relative()
                        .w_full()
                        .h(px(plot_height(kind)))
                        .when(kind == Kind::Number, |this| {
                            this.bg(tokens::black()).rounded(px(4.)).overflow_hidden()
                        })
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|shell, event: &MouseDownEvent, _, cx| {
                                shell.begin_sequence_drag(event.position, cx);
                            }),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .w_full()
                                // The markers hang below a ramp, so the
                                // painted area stops short of the box
                                // that catches the pointer.
                                .h(px(paint_height(kind)))
                                .child(paint::plot(
                                    kind,
                                    stops,
                                    ceiling,
                                    Look {
                                        grid: kind == Kind::Number,
                                        handles: Some(selected),
                                    },
                                    Some(open.plot.clone()),
                                )),
                        )
                        // The axis the grid alone cannot state. Studio asks
                        // for this number in a "Max Size" box, which is one
                        // more thing to fill in for something the sequence
                        // already knows.
                        .when(kind == Kind::Number, |this| {
                            this.child(axis_label(ceiling, true))
                                .child(axis_label(0.0, false))
                        }),
                ),
            )
            .child(self.sequence_footer(kind, editor.can_remove_selected(), cx));

        Some(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .pb(tokens::section_gap())
                .child(card)
                .into_any_element(),
        )
    }

    fn sequence_footer(
        &mut self,
        kind: Kind,
        removable: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let inputs: Vec<Entity<gpui_kit::component::input::InputState>> = self
            .sequence
            .as_ref()
            .map(|open| open.fields.clone())
            .unwrap_or_default();
        let color = self.sequence.as_ref().and_then(|open| open.color.clone());

        h_flex()
            .w_full()
            .flex_wrap()
            .items_center()
            .gap(tokens::group_gap())
            .px(tokens::panel_padding())
            .pb(tokens::panel_padding())
            .children(
                sequence_fields(kind)
                    .iter()
                    .zip(inputs)
                    .map(|((_, label), input)| {
                        h_flex()
                            .flex_none()
                            .items_center()
                            .gap(tokens::label_gap())
                            .child(
                                div()
                                    .text_size(tokens::text_sm())
                                    .line_height(tokens::line_sm())
                                    .text_color(tokens::text_label())
                                    .child(*label),
                            )
                            .child(
                                div()
                                    .w(tokens::field_min_width())
                                    .h(tokens::input_height())
                                    .px(tokens::input_padding())
                                    .bg(tokens::field_select())
                                    .rounded(px(4.))
                                    .flex()
                                    .items_center()
                                    .child(
                                        Input::new(&input)
                                            .appearance(false)
                                            .with_size(tokens::field_size())
                                            .h_full(),
                                    ),
                            )
                    }),
            )
            .children(color.map(|state| {
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap(tokens::label_gap())
                    .child(
                        div()
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .text_color(tokens::text_label())
                            .child("Colour"),
                    )
                    .child(ColorPicker::new(&state).with_size(tokens::field_size()))
            }))
            .child(div().flex_1())
            .child(footer_button(
                "sequence-delete",
                "Delete stop",
                removable,
                cx.listener(|shell, _, _, cx| shell.delete_sequence_stop(cx)),
            ))
            .child(footer_button(
                "sequence-reset",
                "Reset",
                true,
                cx.listener(|shell, _, _, cx| shell.reset_sequence(cx)),
            ))
    }
}

/// The plot's own box, which for a ramp is taller than what gets painted so
/// the markers under it are still inside the area a click reaches.
fn plot_height(kind: Kind) -> f32 {
    match kind {
        Kind::Number => CURVE_HEIGHT,
        Kind::Color => RAMP_HEIGHT + MARKER_ROOM,
    }
}

fn paint_height(kind: Kind) -> f32 {
    match kind {
        Kind::Number => CURVE_HEIGHT,
        Kind::Color => RAMP_HEIGHT,
    }
}

/// One end of the value axis, tucked inside the plot rather than in a
/// gutter beside it: a two-character number costs less room than a column
/// would, and the plot is the thing worth the width.
fn axis_label(value: f32, top: bool) -> impl IntoElement {
    div()
        .absolute()
        .left(tokens::label_gap())
        .map(|this| {
            if top {
                this.top(tokens::label_gap())
            } else {
                this.bottom(tokens::label_gap())
            }
        })
        .text_size(tokens::text_xs())
        .line_height(tokens::line_xs())
        .text_color(tokens::text_disabled())
        .child(SharedString::from(super::super::scrub::format(
            value,
            crate::properties::FieldKind::Decimal,
        )))
}

fn footer_button(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_none()
        .h(tokens::input_height())
        .px(tokens::panel_padding())
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(tokens::tile())
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .map(|this| {
            if enabled {
                this.text_color(tokens::text_strong())
                    .cursor_pointer()
                    .hover(|this| this.bg(tokens::hover()))
                    .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::chrome())))
                    .on_click(on_click)
            } else {
                this.text_color(tokens::text_disabled())
            }
        })
        .child(label)
}
