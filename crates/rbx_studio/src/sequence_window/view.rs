//! The window's contents: its title bar, the plot under it, and the footer
//! that reads and edits whichever stop is selected.
//!
//! Every colour, radius and size here comes from `crate::tokens` — see that
//! module's own rule about inventing one.

use gpui_kit::component::color_picker::ColorPicker;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::sequence_editor::Kind;
use crate::tokens;

use super::paint::{self, Look};
use super::{field_text, sequence_fields, SequenceWindow};

/// A ramp needs far less height than a curve: it carries one dimension.
/// (A curve has no constant here — it fills whatever the window gives it.)
const RAMP_HEIGHT: f32 = 56.0;
/// Room under the ramp for the stop markers, which hang below it.
const MARKER_ROOM: f32 = 16.0;

impl Render for SequenceWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The value as the DOM holds it this frame — see the module doc for
        // why this window keeps no copy of it.
        let Some(editor) = self.editor(cx) else {
            // The row is gone: the selection moved, or a script deleted the
            // instance. A window editing nothing is worse than no window.
            window.remove_window();
            return div().into_any_element();
        };
        let kind = self.kind;

        // Seeding the footer's fields here, rather than where a drag or a
        // click changes the selection, is the same discipline
        // `shell::edit::resync_row_widget` follows: a render is the one
        // place that holds both the current value and a `Window`.
        let seeds: Vec<(Entity<InputState>, String)> = self
            .fields
            .iter()
            .zip(sequence_fields(kind))
            .map(|(input, (field, _))| (input.clone(), field_text(&editor, *field)))
            .collect();
        for (input, seed) in &seeds {
            resync(input, seed, window, cx);
        }

        let ceiling = self.ceiling(&editor);
        let stops = editor.stops.clone();
        let selected = editor.selected;

        v_flex()
            .id("sequence-window")
            .size_full()
            .bg(tokens::chrome())
            .font_family(tokens::FONT_FAMILY_UI)
            .text_color(tokens::text_strong())
            // A drag that leaves the plot — which is most of them, since
            // pushing a keypoint past the top of the axis is how the axis
            // grows — still has to be followed and still has to end.
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                view.drag_to(event.position, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, _, cx| view.end_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, _, cx| view.end_drag(cx)),
            )
            .child(crate::shell::panel_topbar(
                SharedString::from(self.title.clone()),
                self.title_grab.clone(),
                cx.listener(|_, _, window: &mut Window, _| window.remove_window()),
            ))
            .child(
                div().flex_1().p(tokens::panel_padding()).child(
                    div()
                        .id("sequence-plot")
                        .relative()
                        .w_full()
                        // A curve takes whatever height the window has, so a
                        // window manager that grew it past its fixed size
                        // (see `SequenceWindow::open` on X11) gives a bigger
                        // graph rather than dead space. A ramp keeps its
                        // height: it carries one dimension, and a 300px-tall
                        // gradient is not a better gradient.
                        .map(|this| match kind {
                            Kind::Number => this
                                .h_full()
                                .bg(tokens::black())
                                .rounded(px(4.))
                                .overflow_hidden(),
                            Kind::Color => this.h(px(RAMP_HEIGHT + MARKER_ROOM)),
                        })
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, event: &MouseDownEvent, _, cx| {
                                view.begin_drag(event.position, cx);
                            }),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .w_full()
                                // The markers hang below a ramp, so the
                                // painted area stops short of the box that
                                // catches the pointer. A curve paints the
                                // whole box.
                                .map(|this| match kind {
                                    Kind::Number => this.h_full(),
                                    Kind::Color => this.h(px(RAMP_HEIGHT)),
                                })
                                .child(paint::plot(
                                    kind,
                                    stops,
                                    ceiling,
                                    Look {
                                        grid: kind == Kind::Number,
                                        handles: Some(selected),
                                    },
                                    Some(self.plot.clone()),
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
            .child(self.footer(kind, editor.can_remove_selected(), cx))
            .into_any_element()
    }
}

impl SequenceWindow {
    fn footer(&mut self, kind: Kind, removable: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let inputs = self.fields.clone();
        let color = self.color.clone();

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
            // The two buttons wrap as a pair, not one each: a `Reset` that
            // wrapped alone to the next line read as a third field.
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap(tokens::label_gap())
                    .child(footer_button(
                        "sequence-delete",
                        "Delete stop",
                        removable,
                        cx.listener(|view, _, _, cx| view.delete_stop(cx)),
                    ))
                    .child(footer_button(
                        "sequence-reset",
                        "Reset",
                        true,
                        cx.listener(|view, _, _, cx| view.reset(cx)),
                    )),
            )
    }
}

/// Writes `seed` into `input` unless it holds focus (a keystroke in
/// progress must win over an external update) or already shows it — the
/// same rule `shell::edit::resync_field` applies to a property row, for the
/// same reason: `set_value` resets the caret unconditionally, which would
/// be visible jitter on every frame of a drag.
fn resync(input: &Entity<InputState>, seed: &str, window: &mut Window, cx: &mut App) {
    if input.focus_handle(cx).is_focused(window) || input.read(cx).value().as_ref() == seed {
        return;
    }
    input.update(cx, |state, cx| state.set_value(seed.to_owned(), window, cx));
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
        .child(SharedString::from(crate::shell::format_scrubbed(value)))
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
