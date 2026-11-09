//! File › Save as Template…: the open place written as a named template (see
//! `crate::place_templates`) with a viewport thumbnail, for Home's New row.

use std::time::Duration;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::v_flex;
use gpui_kit::*;

use super::Shell;
use crate::command_bar::Feedback;
use crate::launcher::ui::{self, Weight};
use crate::{place_templates, tokens};

const SOURCE: &str = "Save as Template";

/// How long to wait for the viewport's frame before saving without a
/// thumbnail; a hidden or parked viewport never answers, and a busy one
/// takes a few seconds to render the extra frame.
const FRAME_WAIT: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(super) struct SaveTemplate {
    requested: bool,
    field: Option<Entity<InputState>>,
    error: Option<String>,
    busy: bool,
}

impl Shell {
    /// Asks for the prompt from a menu row, which has no `Window` to open it
    /// with; the next render does (see `realize_save_template`).
    pub(crate) fn request_save_template(&mut self, cx: &mut Context<Self>) {
        self.save_template.requested = true;
        cx.notify();
    }

    pub(super) fn realize_save_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.save_template.requested) {
            return;
        }
        let stem = self
            .path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let state = cx.new(|cx| InputState::new(window, cx).default_value(stem));
        cx.subscribe_in(&state, window, |shell, _, event: &InputEvent, _, cx| {
            match event {
                InputEvent::PressEnter { .. } => shell.confirm_save_template(cx),
                // A complaint about the old name is stale once it changes.
                InputEvent::Change if shell.save_template.error.take().is_some() => cx.notify(),
                _ => {}
            }
        })
        .detach();
        state.update(cx, |state, cx| {
            state.focus(window, cx);
            // The place's name is only a suggestion: typing replaces it.
            state.select_all(window, cx);
        });
        self.save_template.field = Some(state);
        self.save_template.error = None;
        self.save_template.busy = false;
        cx.notify();
    }

    /// Escape's half of the prompt; returns whether it was open. A save in
    /// flight keeps it open, so its outcome has somewhere to show.
    pub(super) fn close_save_template(&mut self) -> bool {
        if self.save_template.busy {
            return false;
        }
        self.save_template.field.take().is_some()
    }

    fn confirm_save_template(&mut self, cx: &mut Context<Self>) {
        if self.save_template.busy {
            return;
        }
        let Some(field) = self.save_template.field.as_ref() else {
            return;
        };
        let typed = field.read(cx).value().to_string();
        let Some(dir) = place_templates::dir() else {
            self.save_template.error = Some("No config directory to save into".to_owned());
            cx.notify();
            return;
        };
        let name = match place_templates::check_name(&dir, &typed) {
            Ok(name) => name,
            Err(message) => {
                self.save_template.error = Some(message);
                cx.notify();
                return;
            }
        };
        self.save_template.error = None;
        self.save_template.busy = true;
        // Edits still in an open script's buffer belong in the template.
        self.flush_script_edits(cx);
        let dom = self.dom.clone();
        let frame = self.viewport.read(cx).capture(None);
        cx.spawn(async move |shell, cx| {
            let saved = cx
                .background_spawn(async move {
                    let thumbnail = frame
                        .ok_or_else(|| "the viewport is closed".to_owned())
                        .and_then(|frame| {
                            frame
                                .recv_timeout(FRAME_WAIT)
                                .map_err(|_| "the viewport didn't answer in time".to_owned())
                        })
                        .and_then(|frame| frame)
                        .and_then(|(w, h, rgba)| place_templates::thumbnail_png(w, h, rgba));
                    let saved = place_templates::save(&dir, &name, &dom, thumbnail.as_deref().ok());
                    (saved.map(|_| name), thumbnail.err())
                })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                shell.finish_save_template(saved.0, saved.1, cx)
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_save_template(
        &mut self,
        saved: Result<String, String>,
        no_thumbnail: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.save_template.busy = false;
        match saved {
            Ok(name) => {
                self.save_template.field = None;
                let feedback =
                    Feedback::Output(format!("Saved template {name}; it's on Home's New row"));
                self.output.push(SOURCE, feedback.clone());
                self.command_bar.set_feedback(feedback);
                if let Some(why) = no_thumbnail {
                    self.output.push(
                        SOURCE,
                        Feedback::Warning(format!("Saved without a thumbnail: {why}")),
                    );
                }
            }
            Err(message) => self.save_template.error = Some(message),
        }
        cx.notify();
    }

    pub(super) fn save_template_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.save_template.field.as_ref()?;
        let busy = self.save_template.busy;
        let border = if self.save_template.error.is_some() {
            ui::red_line()
        } else {
            tokens::border()
        };
        let field = ui::field_frame(Some(360.), ui::panel2(), border, "layout-template").child(
            div().flex_1().min_w_0().h_full().child(
                Input::new(state)
                    .appearance(false)
                    .h_full()
                    .px(px(0.))
                    .text_size(px(12.))
                    .text_color(tokens::text()),
            ),
        );
        let body = v_flex()
            .mt(px(14.))
            .ml(px(78.))
            .gap(px(6.))
            .child(field)
            .children(self.save_template.error.as_ref().map(|error| {
                ui::text(11., 16.)
                    .text_color(ui::red())
                    .child(error.clone())
            }))
            .into_any_element();
        let save = if busy {
            ui::disabled_button("save-template-confirm", "Saving\u{2026}").into_any_element()
        } else {
            ui::button("save-template-confirm", "Save", Weight::Primary, false)
                .on_click(cx.listener(|shell, _, _, cx| shell.confirm_save_template(cx)))
                .into_any_element()
        };
        Some(
            ui::dialog(
                480.,
                ui::dialog_glyph("layout-template", ui::accent(), tokens::accent_soft()),
                "Save as Template",
                "Saves this place as it is now, so Home's New row can start new places from it. Your template stays on this computer.",
                Some(body),
                vec![
                    ui::button("save-template-cancel", "Cancel", Weight::Ghost, false)
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.close_save_template();
                            cx.notify();
                        }))
                        .into_any_element(),
                    save,
                ],
            )
            .into_any_element(),
        )
    }
}
