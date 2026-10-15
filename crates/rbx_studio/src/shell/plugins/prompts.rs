//! What a plugin asks the user: a Color3 (`plugin:PromptColor3`), a
//! BrickColor (`plugin:PromptBrickColor`), whether it may reach an HTTP
//! host, or which item of a `PluginMenu`.

use std::rc::Rc;

use gpui_kit::component::color_picker::{ColorPicker, ColorPickerState};
use gpui_kit::component::v_flex;
use gpui_kit::*;
use rbx_lua::plugin::{Answer, Input, MenuItem, PluginId, PromptKind, UiId};

use crate::launcher::ui::{self, Weight};
use crate::shell::brick_color::BrickTarget;
use crate::shell::edit::{hsla_to_rgb, rgb_to_hsla};
use crate::shell::Shell;
use crate::tokens;

/// Prompts waiting for the user, oldest first; one shows at a time.
#[derive(Default)]
pub(crate) struct Prompts {
    pub(crate) queue: Vec<(UiId, PluginId, PromptKind)>,
    /// The front Color3 prompt's picker, made the first time it shows.
    color: Option<Entity<ColorPickerState>>,
    /// The front BrickColor prompt's choice so far.
    brick: Option<u32>,
    /// Whether `brick` has been seeded from the front prompt.
    brick_seeded: bool,
    /// Where the front menu opens: the cursor when it first showed.
    menu_at: Option<Point<Pixels>>,
}

impl Shell {
    /// Queues a prompt from `Request::Prompt`.
    pub(super) fn plugin_prompt(
        &mut self,
        id: UiId,
        plugin: PluginId,
        kind: PromptKind,
        cx: &mut Context<Self>,
    ) {
        self.plugins.prompts.queue.push((id, plugin, kind));
        cx.notify();
    }

    /// Answers the front prompt and moves on to the next.
    fn answer_prompt(&mut self, answer: Answer, cx: &mut Context<Self>) {
        if self.plugins.prompts.queue.is_empty() {
            return;
        }
        let (id, _, _) = self.plugins.prompts.queue.remove(0);
        let prompts = &mut self.plugins.prompts;
        prompts.color = None;
        prompts.brick = None;
        prompts.brick_seeded = false;
        prompts.menu_at = None;
        self.plugin_input(Input::PromptAnswered(id, answer), cx);
        cx.notify();
    }

    /// The front prompt as an overlay: a dialog with a colour picker (Color3)
    /// or the BrickColor palette (BrickColor) and OK/Cancel; an Allow/Deny
    /// dialog naming the plugin and host (HTTP); a menu at the cursor
    /// (Menu). Answering sends `Input::PromptAnswered` and shows the next.
    pub(super) fn plugin_prompt_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (_, plugin, kind) = self.plugins.prompts.queue.first()?.clone();
        Some(match kind {
            PromptKind::Color3 { initial } => self.color_prompt(initial, window, cx),
            PromptKind::BrickColor { initial } => self.brick_prompt(initial, cx),
            PromptKind::HttpPermission { host } => self.http_prompt(plugin, &host, cx),
            PromptKind::Menu { title, items } => self.menu_prompt(&title, &items, window, cx),
        })
    }

    fn color_prompt(
        &mut self,
        initial: Option<[f32; 3]>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = self
            .plugins
            .prompts
            .color
            .get_or_insert_with(|| {
                let [r, g, b] = initial.unwrap_or([1.0, 1.0, 1.0]);
                let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
                let value = rgb_to_hsla(byte(r), byte(g), byte(b));
                cx.new(|cx| ColorPickerState::new(window, cx).default_value(value))
            })
            .clone();
        let picked = state.clone();
        modal(
            "Pick a Color",
            "A plugin is asking for a colour.",
            ColorPicker::new(&state).into_any_element(),
            vec![
                ui::button("plugin-prompt-cancel", "Cancel", Weight::Secondary, false)
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.answer_prompt(Answer::Color3(None), cx);
                    }))
                    .into_any_element(),
                ui::button("plugin-prompt-ok", "OK", Weight::Primary, false)
                    .on_click(cx.listener(move |shell, _, _, cx| {
                        let value = picked.read(cx).value().map(|color| {
                            let (r, g, b) = hsla_to_rgb(color);
                            [r, g, b].map(|c| f32::from(c) / 255.0)
                        });
                        shell.answer_prompt(Answer::Color3(value), cx);
                    }))
                    .into_any_element(),
            ],
        )
    }

    fn brick_prompt(&mut self, initial: Option<u32>, cx: &mut Context<Self>) -> AnyElement {
        if !self.plugins.prompts.brick_seeded {
            self.plugins.prompts.brick_seeded = true;
            self.plugins.prompts.brick = initial;
            self.reset_brick_cursor(initial);
        }
        let current = self.plugins.prompts.brick;
        let target = BrickTarget::Prompt(Rc::new(
            |shell: &mut Shell, number: u32, cx: &mut Context<Shell>| {
                shell.plugins.prompts.brick = Some(number);
                cx.notify();
            },
        ));
        let palette = self
            .brick_color_palette(target, current, cx)
            .into_any_element();
        modal(
            "Pick a BrickColor",
            "A plugin is asking for a BrickColor.",
            palette,
            vec![
                ui::button("plugin-prompt-cancel", "Cancel", Weight::Secondary, false)
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.answer_prompt(Answer::BrickColor(None), cx);
                    }))
                    .into_any_element(),
                ui::button("plugin-prompt-ok", "OK", Weight::Primary, false)
                    .on_click(cx.listener(|shell, _, _, cx| {
                        let picked = shell.plugins.prompts.brick;
                        shell.answer_prompt(Answer::BrickColor(picked), cx);
                    }))
                    .into_any_element(),
            ],
        )
    }

    fn http_prompt(&mut self, plugin: PluginId, host: &str, cx: &mut Context<Self>) -> AnyElement {
        let name = self
            .plugins
            .host
            .as_ref()
            .and_then(|h| h.plugins().into_iter().find(|p| p.id == plugin))
            .map_or_else(|| "A plugin".to_owned(), |p| p.name);
        div()
            .absolute()
            .inset_0()
            .child(ui::dialog(
                440.,
                ui::dialog_glyph("globe", ui::accent(), ui::wash()),
                format!("Allow {name} to reach {host}?"),
                format!("{name} wants to send HTTP requests to {host}. Your answer is remembered."),
                None,
                vec![
                    ui::button("plugin-prompt-deny", "Deny", Weight::Secondary, false)
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.answer_prompt(Answer::Allow(false), cx);
                        }))
                        .into_any_element(),
                    ui::button("plugin-prompt-allow", "Allow", Weight::Primary, false)
                        .on_click(cx.listener(|shell, _, _, cx| {
                            shell.answer_prompt(Answer::Allow(true), cx);
                        }))
                        .into_any_element(),
                ],
            ))
            .into_any_element()
    }

    fn menu_prompt(
        &mut self,
        title: &str,
        items: &[MenuItem],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let at = *self
            .plugins
            .prompts
            .menu_at
            .get_or_insert_with(|| window.mouse_position());
        let mut rows = Vec::new();
        if !title.is_empty() {
            rows.push(menu_label(title, 0));
        }
        menu_rows(items, 0, &mut rows, cx);
        let surface = v_flex()
            .id("plugin-prompt-menu")
            .min_w(px(160.))
            .py(px(4.))
            .bg(tokens::chrome())
            .border_1()
            .border_color(tokens::border())
            .rounded(tokens::radius())
            .shadow(tokens::elevation())
            .text_size(tokens::text_sm())
            .text_color(tokens::text())
            .on_mouse_down_out(cx.listener(|shell, _, _, cx| {
                shell.answer_prompt(Answer::Menu(None), cx);
            }))
            .children(rows);
        deferred(
            anchored()
                .position(at)
                .snap_to_window_with_margin(px(8.))
                .child(surface),
        )
        .into_any_element()
    }
}

fn modal(
    title: &'static str,
    text: &'static str,
    body: AnyElement,
    footer: Vec<AnyElement>,
) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .child(ui::dialog(
            440.,
            ui::dialog_glyph("palette", ui::accent(), ui::wash()),
            title,
            text,
            Some(div().px(px(20.)).pb(px(12.)).child(body).into_any_element()),
            footer,
        ))
        .into_any_element()
}

fn menu_label(text: &str, depth: usize) -> AnyElement {
    div()
        .px(px(10. + 12. * depth as f32))
        .py(px(3.))
        .text_color(tokens::text2())
        .child(text.to_owned())
        .into_any_element()
}

/// Submenus open inline, indented under their title.
fn menu_rows(
    items: &[MenuItem],
    depth: usize,
    rows: &mut Vec<AnyElement>,
    cx: &mut Context<Shell>,
) {
    for item in items {
        match item {
            MenuItem::Action { id, text, .. } => {
                let id = *id;
                rows.push(
                    div()
                        .id(("plugin-prompt-menu-item", id as usize))
                        .px(px(10. + 12. * depth as f32))
                        .py(px(3.))
                        .cursor_pointer()
                        .hover(|this| this.bg(tokens::hover()))
                        .child(text.clone())
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.answer_prompt(Answer::Menu(Some(id)), cx);
                        }))
                        .into_any_element(),
                );
            }
            MenuItem::Separator => rows.push(
                div()
                    .my(px(4.))
                    .h(px(1.))
                    .bg(tokens::border())
                    .into_any_element(),
            ),
            MenuItem::Submenu { title, items } => {
                rows.push(menu_label(title, depth));
                menu_rows(items, depth + 1, rows, cx);
            }
        }
    }
}
