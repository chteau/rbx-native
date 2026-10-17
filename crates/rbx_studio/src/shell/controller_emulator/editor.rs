//! The mapping editor: click a control's label, press the key to bind to
//! it. Edits go to a draft that only Save commits.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use rbx_play::gamepad::{Device, PadState};

use crate::launcher::ui;
use crate::tokens;

use super::drawing::Geometry;
use super::mapping::{self, Bindings, Control};
use super::ControllerEmulator;

const DRAWING_HEIGHT: f32 = 300.;
const ROW_WIDTH: f32 = 290.;

pub(super) struct Editing {
    draft: Bindings,
    recording: Option<Control>,
    warning: Option<String>,
    confirm_reset: bool,
    hovered: Option<Control>,
}

impl Editing {
    pub(super) fn new(draft: Bindings) -> Self {
        Editing {
            draft,
            recording: None,
            warning: None,
            confirm_reset: false,
            hovered: None,
        }
    }

    /// Whether the key was consumed: only while a control waits for one.
    pub(super) fn record_key(&mut self, key: &str, device: Device) -> bool {
        let Some(control) = self.recording else {
            return false;
        };
        if key == "escape" {
            self.recording = None;
            return true;
        }
        let displaced = mapping::bind(&mut self.draft, control, key);
        self.warning = displaced.map(|other| {
            format!(
                "{} was bound to {}; that control is now unbound.",
                mapping::key_label(key),
                other.label(device)
            )
        });
        self.recording = None;
        true
    }
}

fn row(
    index: usize,
    device: Device,
    editing: &Editing,
    control: Control,
    cx: &mut Context<ControllerEmulator>,
) -> impl IntoElement {
    let recording = editing.recording == Some(control);
    let key = editing.draft.get(&control);
    let (text, color) = match key {
        _ if recording => ("Press a key…".to_owned(), tokens::text()),
        Some(key) => (mapping::key_label(key), tokens::text()),
        None => ("Unbound".to_owned(), tokens::text3()),
    };
    let chip = div()
        .px_2()
        .py_0p5()
        .rounded(px(4.))
        .border_1()
        .border_color(if recording {
            tokens::check_on()
        } else {
            tokens::border()
        })
        .font_family("monospace")
        .text_size(px(11.5))
        .text_color(color)
        .child(text);
    h_flex()
        .id(("ctrl-mapping-row", index))
        .w(px(ROW_WIDTH))
        .px_2()
        .py_1()
        .gap_2()
        .items_center()
        .justify_between()
        .rounded(px(4.))
        .cursor_pointer()
        .hover(|style| style.bg(tokens::hover()))
        .child(div().child(control.label(device)))
        .child(chip)
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if let Some(editing) = this.editing.as_mut() {
                editing.hovered = hovered.then_some(control);
            }
            cx.notify();
        }))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if let Some(editing) = this.editing.as_mut() {
                editing.recording = Some(control);
            }
            cx.notify();
        }))
}

fn bottom_bar(
    device: Device,
    editing: &Editing,
    cx: &mut Context<ControllerEmulator>,
) -> impl IntoElement {
    let bar = h_flex()
        .flex_none()
        .px_4()
        .py_2()
        .gap_2()
        .items_center()
        .border_t_1()
        .border_color(tokens::border());
    if editing.confirm_reset {
        return bar
            .child(
                div()
                    .flex_1()
                    .child(format!("Restore the default keys for {}?", device.label())),
            )
            .child(
                ui::button("ctrl-restore", "Restore", ui::Weight::Primary, true).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| {
                        if let Some(editing) = this.editing.as_mut() {
                            editing.draft = mapping::defaults();
                            editing.confirm_reset = false;
                            editing.warning = None;
                            editing.recording = None;
                        }
                        cx.notify();
                    }),
                ),
            )
            .child(
                ui::button("ctrl-keep", "Keep", ui::Weight::Secondary, true).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        if let Some(editing) = this.editing.as_mut() {
                            editing.confirm_reset = false;
                        }
                        cx.notify();
                    },
                )),
            );
    }
    let message = div()
        .flex_1()
        .text_size(px(12.))
        .text_color(tokens::text_error());
    bar.child(match &editing.warning {
        Some(warning) => message.child(warning.clone()),
        None => message,
    })
    .child(
        ui::button(
            "ctrl-reset-defaults",
            "Restore Defaults",
            ui::Weight::Secondary,
            true,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            if let Some(editing) = this.editing.as_mut() {
                editing.confirm_reset = true;
            }
            cx.notify();
        })),
    )
    .child(
        ui::button("ctrl-cancel", "Cancel", ui::Weight::Secondary, true).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.editing = None;
                cx.notify();
            },
        )),
    )
    .child(
        ui::button("ctrl-save", "Save", ui::Weight::Primary, true).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                if let Some(editing) = this.editing.as_ref() {
                    let draft = editing.draft.clone();
                    this.save_bindings(draft, cx);
                }
            },
        )),
    )
}

pub(super) fn editor(
    device: Device,
    editing: &Editing,
    geometry: &Geometry,
    cx: &mut Context<ControllerEmulator>,
) -> impl IntoElement {
    let drawing = super::drawing::controller(
        device,
        &PadState::default(),
        Some(&editing.draft),
        editing.hovered,
        geometry,
        cx,
    );
    let rows: Vec<_> = Control::all()
        .into_iter()
        .enumerate()
        .map(|(index, control)| row(index, device, editing, control, cx))
        .collect();
    v_flex()
        .flex_1()
        .min_h_0()
        .child(div().flex_none().h(px(DRAWING_HEIGHT)).child(drawing))
        .child(
            div()
                .id("ctrl-mapping-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_2()
                .child(h_flex().flex_wrap().children(rows)),
        )
        .child(bottom_bar(device, editing, cx))
}

#[cfg(test)]
mod tests {
    use super::{mapping, Control, Device, Editing};
    use rbx_play::gamepad::Button;

    const SOUTH: Control = Control::Button(Button::South);
    const EAST: Control = Control::Button(Button::East);

    fn recording(control: Control) -> Editing {
        let mut editing = Editing::new(mapping::defaults());
        editing.recording = Some(control);
        editing
    }

    #[test]
    fn keys_are_ignored_when_not_recording() {
        let mut editing = Editing::new(mapping::defaults());
        assert!(!editing.record_key("v", Device::Xbox));
        assert_eq!(editing.draft, mapping::defaults());
    }

    #[test]
    fn escape_cancels_recording() {
        let mut editing = recording(SOUTH);
        assert!(editing.record_key("escape", Device::Xbox));
        assert_eq!(editing.recording, None);
        assert_eq!(editing.draft, mapping::defaults());
    }

    #[test]
    fn a_taken_key_moves_and_the_warning_names_the_old_control() {
        let mut editing = recording(EAST);
        assert!(editing.record_key("space", Device::Xbox));
        assert_eq!(editing.draft.get(&EAST).map(String::as_str), Some("space"));
        assert!(!editing.draft.contains_key(&SOUTH));
        let warning = editing.warning.unwrap();
        assert!(warning.contains("Space") && warning.contains("A"));
        assert_eq!(editing.recording, None);
    }

    #[test]
    fn a_free_key_clears_the_warning() {
        let mut editing = recording(EAST);
        editing.warning = Some("old".into());
        assert!(editing.record_key("v", Device::Xbox));
        assert_eq!(editing.warning, None);
        assert_eq!(editing.draft.get(&EAST).map(String::as_str), Some("v"));
    }
}
