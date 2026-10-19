//! The window's drawing: header, section tabs, live strip, the section
//! body, the footer and the dialog layer. The bodies live in submodules.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::launcher::ui::{self, Weight};
use crate::tokens;

use super::state::{Dialog, Section, UniverseSource};
use super::DataStoresWindow;

mod common;
mod dialogs;
mod entry;
mod other;
mod standard;

#[cfg(test)]
pub(super) use dialogs::{confirm_text, ConfirmText};

impl Render for DataStoresWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.reseed_inputs(window, cx);
        let body = if self.universe.is_none() {
            self.no_universe(cx)
        } else {
            match self.section {
                Section::Standard => self.standard_body(cx),
                Section::Ordered => self.ordered_body(cx),
                Section::Memory => self.memory_body(cx),
            }
        };
        v_flex()
            .id("data-stores-window")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(tokens::dock())
            .font_family(tokens::font_ui())
            .text_size(px(13.))
            .text_color(tokens::text())
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                this.handle_key(e, window, cx);
            }))
            .child(crate::shell::chrome::window_topbar(
                "Data Stores".into(),
                true,
                |window, cx| window.defer(cx, |window, _| window.remove_window()),
            ))
            .when(self.universe.is_some(), |d| {
                d.child(self.header(cx))
                    .child(self.section_tabs(cx))
                    .children(self.live_strip())
            })
            .child(div().flex_1().min_h_0().child(body))
            .child(self.footer())
            .children(self.dialog_layer(cx))
    }
}

impl DataStoresWindow {
    fn handle_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if e.keystroke.modifiers.modified() {
            return;
        }
        match e.keystroke.key.as_str() {
            "escape" => {
                if self.dialog.is_some() {
                    self.dialog = None;
                } else if self.pane.editing.is_some() {
                    self.cancel_edit(cx);
                }
                cx.notify();
            }
            "enter" => {
                let allowed = match &self.dialog {
                    Some(Dialog::Confirm { live_ack, .. }) => {
                        *live_ack || self.universe.as_ref().is_none_or(|u| !u.live)
                    }
                    _ => false,
                };
                if allowed {
                    self.confirm(cx);
                } else if self.dialog.is_none() && self.root_focused(window, cx) {
                    self.activate_selection(cx);
                }
            }
            key @ ("up" | "down" | "pageup" | "pagedown")
                if self.dialog.is_none() && self.root_focused(window, cx) =>
            {
                let delta = match key {
                    "up" => -1,
                    "down" => 1,
                    "pageup" => -10,
                    _ => 10,
                };
                self.move_selection(delta, cx);
            }
            _ => {}
        }
    }

    /// Arrow keys belong to a text field while one has focus.
    fn root_focused(&self, window: &Window, cx: &App) -> bool {
        window.focused(cx).as_ref() == Some(&self.focus)
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(universe) = self.universe.clone() else {
            return div().into_any_element();
        };
        let title = universe
            .name
            .clone()
            .unwrap_or_else(|| format!("Universe {}", universe.id));
        let source = match universe.source {
            UniverseSource::Linked => "Linked to this place",
            UniverseSource::Sandbox => "Your Play sandbox",
            UniverseSource::Picked => "Picked experience",
            UniverseSource::Mock => "Mock universe",
        };
        let sandbox_other = self
            .shell
            .read(cx)
            .play
            .sandbox
            .is_some_and(|s| s.universe_id != universe.id);
        let (tag, glyph) = if universe.live {
            (
                ui::tag("LIVE", tokens::warning(), tokens::error_soft()),
                "shield-alert",
            )
        } else {
            (
                ui::tag("SANDBOX", ui::green(), ui::green_soft()),
                "shield-check",
            )
        };
        h_flex()
            .h(px(56.))
            .flex_none()
            .px(px(16.))
            .gap(px(12.))
            .items_center()
            .border_b_1()
            .border_color(tokens::border())
            .child(
                div()
                    .text_color(tokens::text2())
                    .child(ui::icon("database", 20.)),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::BOLD)
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .font_family(tokens::font_mono())
                            .text_size(px(11.5))
                            .text_color(tokens::text3())
                            .child(format!("Universe {} \u{b7} {source}", universe.id)),
                    ),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .when(universe.source == UniverseSource::Mock, |d| {
                        d.child(ui::tag("MOCK", ui::accent(), ui::wash()))
                    })
                    .child(tag)
                    .child(
                        div()
                            .text_color(tokens::text2())
                            .child(ui::icon(glyph, 16.)),
                    ),
            )
            .child(
                ui::button(
                    "ds-change-experience",
                    "Change experience\u{2026}",
                    Weight::Secondary,
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| this.pick_experience(cx))),
            )
            .when(sandbox_other, |d| {
                d.child(
                    ui::button("ds-use-sandbox", "Use sandbox", Weight::Ghost, true)
                        .on_click(cx.listener(|this, _, _, cx| this.use_sandbox(cx))),
                )
            })
            .child(
                ui::icon_button("ds-refresh", "refresh-cw", "Refresh", Weight::Ghost, true)
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            )
            .into_any_element()
    }

    fn section_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        const TABS: [(&str, Section); 3] = [
            ("Data Stores", Section::Standard),
            ("Ordered Data Stores", Section::Ordered),
            ("Memory Store", Section::Memory),
        ];
        h_flex()
            .h(px(36.))
            .flex_none()
            .px(px(12.))
            .gap(px(4.))
            .border_b_1()
            .border_color(tokens::border())
            .children(TABS.into_iter().enumerate().map(|(i, (label, section))| {
                let selected = self.section == section;
                h_flex()
                    .id(SharedString::from(format!("ds-section-{i}")))
                    .h_full()
                    .px(px(12.))
                    .items_center()
                    .cursor_pointer()
                    .text_color(if selected {
                        tokens::check_on()
                    } else {
                        tokens::text2()
                    })
                    .when(selected, |d| {
                        d.border_b_2().border_color(tokens::check_on())
                    })
                    .when(!selected, |d| d.hover(|h| h.text_color(tokens::text())))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_section(section, cx)))
            }))
    }

    fn live_strip(&self) -> Option<Div> {
        self.universe.as_ref().filter(|u| u.live).map(|_| {
            h_flex()
                .h(px(30.))
                .flex_none()
                .px(px(16.))
                .items_center()
                .bg(tokens::error_soft())
                .text_color(tokens::warning())
                .text_size(px(12.))
                .child(
                    "Live experience: reads are safe, but every write changes real player data and asks you to confirm first.",
                )
        })
    }

    fn footer(&self) -> impl IntoElement {
        let status = self.status.as_ref();
        h_flex()
            .h(px(28.))
            .flex_none()
            .px(px(12.))
            .gap(px(12.))
            .items_center()
            .border_t_1()
            .border_color(tokens::border())
            .text_size(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(match status {
                        Some(s) if s.ok => ui::green(),
                        _ => tokens::text_error(),
                    })
                    .children(status.map(|s| s.text.clone())),
            )
            .when(self.rate_limited, |d| {
                d.child(
                    div()
                        .text_color(tokens::warning())
                        .child("Roblox is rate limiting this experience \u{2014} retrying\u{2026}"),
                )
            })
            .when(!self.rate_limited && self.in_flight > 0, |d| {
                d.child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .text_color(tokens::text2())
                        .child(ui::spinner("ds-footer-spin", 12.))
                        .child("Working\u{2026}"),
                )
            })
    }

    fn no_universe(&self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(440.))
                    .p(px(24.))
                    .gap(px(10.))
                    .rounded(tokens::radius())
                    .border_1()
                    .border_color(tokens::border())
                    .bg(ui::panel())
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::BOLD)
                            .child("No experience to browse"),
                    )
                    .child(
                        div().text_color(tokens::text2()).child(
                            "This place isn\u{2019}t linked to Roblox and no Play sandbox is set (Settings \u{203a} Files & Account). Choose one of your experiences to browse its data.",
                        ),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(
                                ui::button("ds-choose-experience", "Choose an experience\u{2026}", Weight::Primary, false)
                                    .on_click(cx.listener(|this, _, _, cx| this.pick_experience(cx))),
                            )
                            .child(
                                ui::button("ds-manage-key-empty", "Manage key", Weight::Secondary, false)
                                    .on_click(cx.listener(|this, _, _, cx| this.manage_key(cx))),
                            ),
                    ),
            )
            .into_any_element()
    }
}
