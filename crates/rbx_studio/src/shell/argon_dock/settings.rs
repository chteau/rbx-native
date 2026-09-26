//! The Argon dock's settings column: the header with the level switch and
//! Restore defaults, then the plugin's fifteen settings as cards under
//! four section headers. Which level a card edits, and what it shows, is
//! `Shell::argon_level` / `argon_shown` in the parent module; the
//! dropdown and stepper controls live in `controls`.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::settings::argon::{Level, Setting, Value};
use crate::tokens;

use super::super::{rows, Shell};
use super::Layout;

/// The sections and their cards, in the order they read.
const SECTIONS: [(&str, &[Setting]); 4] = [
    (
        "CONNECTION",
        &[Setting::AutoConnect, Setting::AutoReconnect, Setting::Https],
    ),
    (
        "SYNC",
        &[
            Setting::InitialSyncPriority,
            Setting::LiveHydrate,
            Setting::KeepUnknowns,
            Setting::OverridePackages,
        ],
    ),
    (
        "TWO-WAY SYNC",
        &[
            Setting::TwoWaySync,
            Setting::SyncbackProperties,
            Setting::OnlyCodeMode,
        ],
    ),
    (
        "WORKFLOW",
        &[
            Setting::DisplayPrompts,
            Setting::ChangesThreshold,
            Setting::DiffLinesLimit,
            Setting::OpenInEditor,
            Setting::LogLevel,
        ],
    ),
];

/// A card's title and one-line description.
fn copy(setting: Setting) -> (&'static str, &'static str) {
    match setting {
        Setting::AutoConnect => ("Auto Connect", "Connect when you open a place."),
        Setting::AutoReconnect => (
            "Auto Reconnect",
            "Reconnect 5 s after the connection drops.",
        ),
        Setting::Https => ("HTTPS", "Connect to the server over HTTPS."),
        Setting::InitialSyncPriority => (
            "Initial Sync Priority",
            "Which side wins when you first connect.",
        ),
        Setting::LiveHydrate => ("Live Hydrate", "Fetch missing instances from the server."),
        Setting::KeepUnknowns => (
            "Keep Unknowns",
            "Keep instances the file system doesn't have.",
        ),
        Setting::OverridePackages => (
            "Override Packages",
            "Let server changes write into packages.",
        ),
        Setting::TwoWaySync => (
            "Two-Way Sync",
            "Send Studio changes back to the file system.",
        ),
        Setting::SyncbackProperties => (
            "Syncback Properties",
            "Sync every property back, not only code.",
        ),
        Setting::OnlyCodeMode => (
            "Only Code Mode",
            "Sync back scripts and their ancestors only.",
        ),
        Setting::DisplayPrompts => ("Display Prompts", "When to ask before applying changes."),
        Setting::ChangesThreshold => ("Changes Threshold", "Changes applied before Argon asks."),
        Setting::DiffLinesLimit => (
            "Diff Lines Limit",
            "Lines the diff view shows before cutting off.",
        ),
        Setting::OpenInEditor => ("Open In Editor", "Open scripts in your OS default editor."),
        Setting::LogLevel => ("Log Level", "How much Argon writes to Output."),
    }
}

impl Shell {
    /// The column: header row, then the sections — in their own scroll
    /// area with a 12px gutter when `own_scroll`, at natural height
    /// otherwise (the body scrolls instead).
    pub(super) fn argon_settings_column(
        &mut self,
        layout: Layout,
        own_scroll: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let header = self.settings_header(layout, cx);
        let sections = v_flex().gap(px(16.)).children(
            SECTIONS
                .iter()
                .map(|(title, settings)| self.section(title, settings, layout, cx)),
        );
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .gap(px(12.))
            .child(header)
            .child(if own_scroll {
                // The kit's `vertical_scrollbar` follows the theme's mode
                // (shown while scrolling); a column clipped mid-card needs
                // its thumb at rest, so this one is always shown: 4px wide,
                // 2px in from the column's right edge, flush with the top.
                // The overlay is a sibling of the scroll area rather than a
                // child, so the scroll offset never moves it.
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("argon-settings-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.argon_ui.settings_scroll)
                            .child(sections.pr(px(12.))),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left_0()
                            .right(px(2.))
                            .child(
                                gpui_kit::base::Scrollbar::new(&self.argon_ui.settings_scroll)
                                    .id("argon-settings-scrollbar")
                                    .axis(Axis::Vertical)
                                    .mode(gpui_kit::base::ScrollbarMode::Always)
                                    .styles(|styles| {
                                        styles.thumb(|thumb| {
                                            thumb
                                                .bg(tokens::border2())
                                                .width(px(4.))
                                                .inset(px(0.))
                                                .radius(px(2.))
                                        })
                                    })
                                    .viewport_from_layout(),
                            ),
                    )
                    .into_any_element()
            } else {
                div().flex_none().child(sections).into_any_element()
            })
    }

    /// 26px: "Settings", the Global / Game / Place switch, Restore defaults.
    fn settings_header(&mut self, layout: Layout, cx: &mut Context<Self>) -> Div {
        let keys = self.argon_level_keys();
        let current = self.argon_level();
        let segments = [Level::Global, Level::Game, Level::Place].map(|level| {
            let enabled = match level {
                Level::Global => true,
                Level::Game => keys.game.is_some(),
                Level::Place => keys.place.is_some(),
            };
            let selected = level == current;
            h_flex()
                .id(SharedString::from(format!("argon-level-{}", level.label())))
                .h(px(22.))
                .px(px(10.))
                .items_center()
                .rounded(tokens::radius_segment())
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .map(|this| {
                    if selected {
                        this.bg(tokens::accent_soft())
                            .text_color(tokens::check_on())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                    } else if enabled {
                        this.tab_index(self.tab_order.next())
                            .cursor_pointer()
                            .text_color(tokens::text2())
                            .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                            .focus_visible(|this| {
                                this.shadow(tokens::focus_ring(tokens::field_select()))
                            })
                            .on_click(cx.listener(move |shell, _, window, cx| {
                                shell.set_argon_level(level, window, cx)
                            }))
                    } else {
                        this.text_color(tokens::text3())
                    }
                })
                .child(level.label())
        });
        let restore = if layout.compact_restore {
            h_flex()
                .id("argon-restore-defaults")
                .tab_index(self.tab_order.next())
                .flex_none()
                .size(px(26.))
                .items_center()
                .justify_center()
                .rounded(tokens::radius())
                .cursor_pointer()
                .text_color(tokens::text2())
                .hover(|this| {
                    tokens::hover_fx(this)
                        .bg(tokens::hover())
                        .text_color(tokens::text())
                })
                .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
                .tooltip(|window, cx| super::super::tooltip::text("Restore defaults", window, cx))
                .on_click(
                    cx.listener(|shell, _, window, cx| shell.argon_restore_defaults(window, cx)),
                )
                .child(Icon::new(IconName::RotateCcw).size(px(12.)))
        } else {
            h_flex()
                .id("argon-restore-defaults")
                .tab_index(self.tab_order.next())
                .flex_none()
                .h(px(26.))
                .px(px(8.))
                .items_center()
                .gap(px(6.))
                .rounded(tokens::radius())
                .cursor_pointer()
                .text_size(tokens::text_ghost())
                .line_height(tokens::line_ghost())
                .text_color(tokens::text2())
                .hover(|this| {
                    tokens::hover_fx(this)
                        .bg(tokens::hover())
                        .text_color(tokens::text())
                })
                .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
                .on_click(
                    cx.listener(|shell, _, window, cx| shell.argon_restore_defaults(window, cx)),
                )
                .child(Icon::new(IconName::RotateCcw).size(px(12.)))
                .child("Restore defaults")
        };
        h_flex()
            .h(px(26.))
            .flex_none()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .text_size(tokens::text_md())
                    .line_height(tokens::line_md())
                    .font_weight(tokens::WEIGHT_BOLD)
                    .text_color(tokens::text())
                    .child("Settings"),
            )
            .child(
                h_flex()
                    .p(px(2.))
                    .gap(px(2.))
                    .rounded(tokens::radius())
                    .bg(tokens::field_select())
                    .children(segments),
            )
            .child(div().flex_1())
            .child(restore)
    }

    /// A 14px uppercase header, 6px, then the cards in rows of
    /// `layout.columns`.
    fn section(
        &mut self,
        title: &'static str,
        settings: &[Setting],
        layout: Layout,
        cx: &mut Context<Self>,
    ) -> Div {
        let cards: Vec<AnyElement> = settings
            .iter()
            .map(|&setting| self.card(setting, layout, cx).into_any_element())
            .collect();
        let mut rows = v_flex().gap(px(10.));
        let mut cards = cards.into_iter().peekable();
        while cards.peek().is_some() {
            let mut row = h_flex().items_stretch().gap(px(12.));
            for _ in 0..layout.columns {
                match cards.next() {
                    Some(card) => row = row.child(div().flex_1().min_w_0().flex().child(card)),
                    None => row = row.child(div().flex_1().min_w_0()),
                }
            }
            rows = rows.child(row);
        }
        v_flex()
            .gap(px(6.))
            .child(
                div()
                    .h(px(14.))
                    .text_size(tokens::text_xxs())
                    .line_height(tokens::line_xxs())
                    .font_weight(tokens::WEIGHT_BOLD)
                    .text_color(tokens::text3())
                    .child(title),
            )
            .child(rows)
    }

    /// One card: title, one-line description, and the control on the right.
    fn card(&mut self, setting: Setting, layout: Layout, cx: &mut Context<Self>) -> Div {
        let (title, description) = copy(setting);
        let shown = self.argon_shown(setting);
        let control: AnyElement = match shown {
            Value::Bool(on) => rows::checkbox(
                SharedString::from(format!("argon-{}", setting.key())),
                on,
                cx.listener(move |shell, _, _, cx| {
                    shell.argon_set(setting, Value::Bool(!on), cx);
                }),
            )
            .into_any_element(),
            Value::Choice(current) => self.dropdown(setting, current, cx).into_any_element(),
            Value::Number(_) => self.stepper(setting, cx).into_any_element(),
        };
        h_flex()
            .w_full()
            .items_center()
            .gap(px(16.))
            .px(px(12.))
            .py(px(10.))
            .rounded(tokens::radius_tile())
            .bg(tokens::field_select())
            .border_1()
            .border_color(tokens::border())
            .hover(|this| tokens::hover_fx(this).border_color(tokens::border2()))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(tokens::text_md())
                            .line_height(tokens::line_md())
                            .font_weight(tokens::WEIGHT_SEMIBOLD)
                            .text_color(tokens::text())
                            .child(title)
                            .when(setting == Setting::TwoWaySync, |this| {
                                this.child(
                                    div()
                                        .px(px(5.))
                                        .rounded(tokens::radius_badge())
                                        .border_1()
                                        .border_color(tokens::border2())
                                        .text_size(tokens::text_xxs())
                                        .line_height(tokens::line_xxs())
                                        .font_weight(tokens::WEIGHT_SEMIBOLD)
                                        .text_color(tokens::text2())
                                        .child("WIP"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(tokens::text_sm())
                            .line_height(tokens::line_sm())
                            .text_color(tokens::text2())
                            .overflow_hidden()
                            .map(|this| {
                                if layout.clamp_descriptions {
                                    this.line_clamp(2)
                                } else {
                                    this.truncate()
                                }
                            })
                            .child(description),
                    ),
            )
            .child(div().flex_none().child(control))
    }
}
