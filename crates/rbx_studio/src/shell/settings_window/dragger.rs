//! The Dragger & snapping page: Studio's dragger guides, the two drag
//! behaviours, and the Snap popover's increments.

use gpui_kit::component::h_flex;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::*;

use crate::settings::DraggerSettings;
use crate::tokens;
use crate::transform::{self, SnapKind, Transform};

use super::super::Shell;
use super::kit::{toggle, Row, Section};
use super::SettingsWindow;

/// The increment fields: typed into here, and rewritten from the setting
/// whenever it changed elsewhere and the field isn't being typed in.
pub(super) struct Increments {
    translate: Entity<InputState>,
    rotate: Entity<InputState>,
}

/// One dragger switch: which field of [`DraggerSettings`] it flips.
type Switch = fn(&mut DraggerSettings) -> &mut bool;

impl Increments {
    pub(super) fn new(
        shell: &Entity<Shell>,
        window: &mut Window,
        cx: &mut Context<SettingsWindow>,
    ) -> (Self, Vec<Subscription>) {
        let mut subscriptions = Vec::new();
        let mut field = |kind: SnapKind, cx: &mut Context<SettingsWindow>| {
            let value = shell.read(cx).snap_increment(kind);
            let input = cx.new(|cx| InputState::new(window, cx).default_value(format!("{value}")));
            // As typed, like the popover: text that isn't a number yet
            // leaves the increment alone.
            subscriptions.push(
                cx.subscribe(&input, move |this, input, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let text = input.read(cx).value().to_string();
                    if let Some(increment) = transform::parse_increment(&text).filter(|v| *v > 0.) {
                        this.shell.update(cx, |shell, cx| {
                            shell.set_snap_increment(kind, increment, cx)
                        });
                    }
                }),
            );
            input
        };
        let increments = Increments {
            translate: field(SnapKind::Translate, cx),
            rotate: field(SnapKind::Rotate, cx),
        };
        (increments, subscriptions)
    }

    fn sync(&self, [translate, rotate]: [f32; 2], window: &mut Window, cx: &mut App) {
        for (input, increment) in [(&self.translate, translate), (&self.rotate, rotate)] {
            let state = input.read(cx);
            if (window.is_window_active() && state.focus_handle(cx).is_focused(window))
                || transform::parse_increment(&state.value()) == Some(increment)
            {
                continue;
            }
            input.update(cx, |state, cx| {
                state.set_value(format!("{increment}"), window, cx)
            });
        }
    }
}

impl SettingsWindow {
    pub(super) fn dragger_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Section> {
        let (dragger, translate, rotate) = {
            let shell = self.shell.read(cx);
            (
                shell.dragger(),
                shell.snap_increment(SnapKind::Translate),
                shell.snap_increment(SnapKind::Rotate),
            )
        };
        self.increments.sync([translate, rotate], window, cx);
        let focused = [&self.increments.translate, &self.increments.rotate]
            .map(|input| input.read(cx).focus_handle(cx).is_focused(window));
        let defaults = DraggerSettings::default();
        let switch = |id: &'static str, label, description, field: Switch| {
            let on = *field(&mut { dragger });
            let default = *field(&mut { defaults });
            let flip = move |shell: &mut Shell, on: bool, cx: &mut Context<Shell>| {
                let mut settings = shell.dragger();
                *field(&mut settings) = on;
                shell.set_dragger(settings, cx);
            };
            Row::new(
                label,
                toggle(id, on, self.set(move |shell, cx| flip(shell, !on, cx))),
            )
            .describe(description)
            .changed(on != default, move |shell, cx| flip(shell, default, cx))
        };
        let dragging = Section::new(
            "While dragging",
            vec![
                switch(
                    "hover-ruler",
                    "Hover ruler",
                    "Distance to the part under the cursor.",
                    |s| &mut s.show_hover_ruler,
                ),
                switch(
                    "target-snap",
                    "Target snap",
                    "Highlight the face or edge a drag will snap to.",
                    |s| &mut s.show_target_snap,
                ),
                switch(
                    "measurement",
                    "Measurement",
                    "Live size and offset next to the cursor.",
                    |s| &mut s.show_measurement,
                ),
                switch(
                    "dragged-point",
                    "Dragged point",
                    "Mark the point of the part you grabbed.",
                    |s| &mut s.show_dragged_point,
                ),
            ],
        );
        let defaults = Transform::default();
        let snapping = Section::new(
            "Snapping",
            vec![
                switch(
                    "snap-to-parts",
                    "Snap to parts",
                    "Stick to other parts\u{2019} faces and edges.",
                    |s| &mut s.snap_to_parts,
                ),
                switch(
                    "align-dragged",
                    "Align dragged objects",
                    "Rotate the dragged part to match the surface under it.",
                    |s| &mut s.align_dragged_objects,
                ),
                Row::new(
                    "Move increment",
                    number(&self.increments.translate, "studs", focused[0]),
                )
                .describe("Also in the ribbon\u{2019}s Snap popover.")
                .changed(
                    translate != defaults.translate.increment,
                    move |shell, cx| {
                        shell.set_snap_increment(
                            SnapKind::Translate,
                            defaults.translate.increment,
                            cx,
                        )
                    },
                ),
                Row::new(
                    "Rotate increment",
                    number(&self.increments.rotate, "\u{b0}", focused[1]),
                )
                .describe("Snap angle for the Rotate tool.")
                .changed(rotate != defaults.rotate.increment, move |shell, cx| {
                    shell.set_snap_increment(SnapKind::Rotate, defaults.rotate.increment, cx)
                }),
            ],
        );
        vec![dragging, snapping]
    }
}

/// An 80×30 mono field, right-aligned, with its unit after it.
fn number(input: &Entity<InputState>, unit: &'static str, focused: bool) -> impl IntoElement {
    h_flex()
        .gap(px(8.))
        .items_center()
        .child(
            h_flex()
                .w(px(80.))
                .h(px(30.))
                .px(px(10.))
                .items_center()
                .border_1()
                .border_color(if focused {
                    tokens::accent_line()
                } else {
                    tokens::border2()
                })
                .rounded(px(6.))
                .bg(tokens::dock())
                .font_family(tokens::FONT_FAMILY_MONO)
                .child(
                    Input::new(input)
                        .appearance(false)
                        .w_full()
                        .px_0()
                        .text_right()
                        .text_size(px(11.5))
                        .line_height(px(16.))
                        .text_color(tokens::text()),
                ),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(tokens::text2())
                .child(unit),
        )
}
