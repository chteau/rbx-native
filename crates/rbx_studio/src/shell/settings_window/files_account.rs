//! The Files & recovery and Account pages. Auto-Recovery and Discord
//! Rich Presence are live; the test-copy name waits on Play, and on
//! Account the multi-account switcher is still on the roadmap.

use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::launcher::{self, KeyTag};
use crate::recovery::{self, INTERVAL_DEFAULT, INTERVAL_MINUTES};
use crate::tokens;

use super::kit::{
    icon, mono, readout, secondary_button, text, ticked_slider, toggle, Row, Section,
};
use super::nav::tilde;
use super::{SettingsWindow, Shell};

/// The Interval row's slider, one stop per minute.
pub(super) struct RecoveryControls {
    interval: Entity<SliderState>,
}

impl RecoveryControls {
    pub(super) fn new(
        shell: &Entity<Shell>,
        cx: &mut Context<SettingsWindow>,
    ) -> (Self, Subscription) {
        let (low, high) = INTERVAL_MINUTES;
        let minutes = shell.read(cx).recovery.minutes();
        let interval = cx.new(|_| {
            SliderState::new()
                .min(low as f32)
                .max(high as f32)
                .step(1.)
                .default_value(minutes as f32)
        });
        let subscription = cx.subscribe(&interval, |this, _, event: &SliderEvent, cx| {
            if let SliderEvent::Change(SliderValue::Single(value)) = event {
                let minutes = value.round() as u32;
                this.shell
                    .update(cx, |shell, cx| shell.set_recovery_minutes(minutes, cx));
            }
        });
        (RecoveryControls { interval }, subscription)
    }
}

impl SettingsWindow {
    pub(super) fn files_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Section> {
        let (enabled, minutes) = {
            let state = &self.shell.read(cx).recovery;
            (state.enabled(), state.minutes())
        };
        // A reset elsewhere moves the setting; the slider follows it.
        let interval = self.recovery.interval.clone();
        if (interval.read(cx).value().end() - minutes as f32).abs() > 1e-4 {
            interval.update(cx, |state, cx| state.set_value(minutes as f32, window, cx));
        }
        let (low, high) = INTERVAL_MINUTES;
        let ticks: Vec<f32> = (low..=high)
            .map(|stop| (stop - low) as f32 / (high - low) as f32)
            .collect();
        let shown = recovery::folder().as_deref().map(tilde).unwrap_or_default();
        vec![
            Section::new(
                "Auto-Recovery",
                vec![
                    Row::new(
                        "Auto-Recovery",
                        toggle(
                            "auto-recovery",
                            enabled,
                            self.set(move |shell, cx| shell.set_auto_recovery(!enabled, cx)),
                        ),
                    )
                    .describe("Save a recovery copy of the open place in the background while it has unsaved changes. Turning it off keeps a copy already written.")
                    .changed(!enabled, |shell, cx| shell.set_auto_recovery(true, cx)),
                    Row::new(
                        "Interval",
                        h_flex()
                            .gap(px(10.))
                            .items_center()
                            .child(ticked_slider(&interval, 180., &ticks, cx))
                            .child(readout(format!("{minutes} min"))),
                    )
                    .describe("How often a recovery copy is written. Ctrl+S deletes it.")
                    .indent()
                    .changed(minutes != INTERVAL_DEFAULT, |shell, cx| {
                        shell.set_recovery_minutes(INTERVAL_DEFAULT, cx)
                    }),
                    Row::new(
                        "Recovery folder",
                        secondary_button("open-auto-saves", "folder", "Open auto-saves")
                            .on_click(|_, _, cx| recovery::open_folder(cx)),
                    )
                    .describe_mono(shown),
                ],
            ),
            Section::new(
                "Play",
                vec![Row::new(
                    "Name for test copies",
                    h_flex()
                        .w(px(200.))
                        .h(px(30.))
                        .px(px(10.))
                        .items_center()
                        .border_1()
                        .border_color(tokens::border2())
                        .rounded(px(6.))
                        .bg(tokens::dock())
                        .child(mono(11.5, 16.).child("{place} (Play)")),
                )
                .describe("What Play calls the copy it runs. {place} is the place\u{2019}s name.")
                .soon()],
            ),
        ]
    }

    pub(super) fn account_page(&mut self, cx: &mut Context<Self>) -> Vec<Section> {
        // Checked when the page is first shown, not when the window opens:
        // the check is a round trip to Roblox.
        // A search reads this page's rows too, and shouldn't start one.
        let summary = if self.key.is_none() && !self.query(cx).is_empty() {
            launcher::KeySummary {
                name: "Your key".into(),
                tag: None,
                meta: SharedString::default(),
                owner: None,
            }
        } else {
            let check = self.key.get_or_insert_with(|| {
                let check = launcher::check_stored_key(cx);
                cx.observe(&check, |_, _, cx| cx.notify()).detach();
                check
            });
            launcher::key_summary(check.read(cx), "stored in your system keychain")
        };
        let name: SharedString = match &summary.owner {
            Some(_) => format!("Open Cloud key \u{201c}{}\u{201d}", summary.name).into(),
            None => summary.name.clone(),
        };
        let tag = summary.tag.map(|tag| {
            let (label, color) = match tag {
                KeyTag::Ready => ("READY", tokens::diff_add()),
                KeyTag::NeedsAttention => ("NEEDS ATTENTION", tokens::text_error()),
                KeyTag::Refused => ("REFUSED", tokens::text_error()),
            };
            h_flex()
                .h(px(18.))
                .px(px(6.))
                .items_center()
                .rounded(px(4.))
                .bg(Rgba { a: 0.12, ..color })
                .text_color(color)
                .text_size(px(10.5))
                .line_height(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(label)
        });
        let window = cx.entity().downgrade();
        let key_card = h_flex()
            .gap(px(12.))
            .py(px(14.))
            .px(px(16.))
            .items_center()
            .child(
                h_flex()
                    .flex_none()
                    .size(px(34.))
                    .rounded(px(8.))
                    .items_center()
                    .justify_center()
                    .bg(tokens::accent_soft())
                    .text_color(tokens::check_on())
                    .child(icon("key-round", 18.)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                text(12.5, 17.)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(name),
                            )
                            .children(tag),
                    )
                    .child(
                        text(11.5, 16.)
                            .truncate()
                            .text_color(tokens::text2())
                            .child(summary.meta.clone()),
                    ),
            )
            .child(
                h_flex()
                    .id("open-publishing")
                    .flex_none()
                    .h(px(28.))
                    .px(px(10.))
                    .gap(px(6.))
                    .items_center()
                    .border_1()
                    .border_color(tokens::border2())
                    .rounded(px(5.))
                    .bg(tokens::field_select())
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .hover(|this| this.bg(tokens::secondary_hover()))
                    .child("Open Roblox publishing")
                    .child(icon("external-link", 12.))
                    .on_click(move |_, _, cx| {
                        let window = window.clone();
                        // A replaced or removed key is checked again.
                        launcher::open_publishing(
                            move |cx| {
                                let _ = window.update(cx, |this, cx| {
                                    this.key = None;
                                    cx.notify();
                                });
                            },
                            cx,
                        );
                    }),
            );
        let owner = summary.owner.clone().unwrap_or_else(|| "You".into());
        let initial: SharedString = owner
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .collect::<String>()
            .into();
        let accounts = v_flex()
            .gap(px(6.))
            .child(
                h_flex()
                    .h(px(40.))
                    .px(px(12.))
                    .gap(px(10.))
                    .items_center()
                    .border_1()
                    .border_color(tokens::accent_line())
                    .rounded(px(6.))
                    .bg(tokens::accent_soft())
                    .child(
                        h_flex()
                            .size(px(22.))
                            .rounded_full()
                            .items_center()
                            .justify_center()
                            .bg(tokens::avatar_on_accent())
                            .text_color(tokens::check_on())
                            .text_size(px(11.))
                            .font_weight(FontWeight::BOLD)
                            .child(initial),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(owner),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.5))
                            .text_color(tokens::text2())
                            .child(format!("Key \u{201c}{}\u{201d}", summary.name)),
                    )
                    .child(
                        text(10.5, 14.)
                            .px(px(6.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens::check_on())
                            .child("Active"),
                    ),
            )
            .child(
                h_flex()
                    .h(px(40.))
                    .px(px(12.))
                    .gap(px(10.))
                    .items_center()
                    .border_1()
                    .border_dashed()
                    .border_color(tokens::border2())
                    .rounded(px(6.))
                    .text_size(px(12.))
                    .text_color(tokens::text2())
                    .child(icon("plus", 13.))
                    .child("Add another account\u{2019}s key"),
            );
        vec![
            Section {
                head: Some(key_card.into_any_element()),
                ..Section::new(
                    "Roblox",
                    vec![Row::new("Accounts", div())
                        .describe("One key per Roblox account; switch without pasting keys again.")
                        .soon()
                        .below(accounts)],
                )
            },
            {
                let enabled = self.shell.read(cx).discord.is_some();
                let hide = self.shell.read(cx).discord_hide_names;
                Section::new(
                    "Discord",
                    vec![
                        Row::new(
                            "Rich Presence",
                            toggle(
                                "discord-presence",
                                enabled,
                                self.set(move |shell, cx| shell.set_discord_presence(!enabled, cx)),
                            ),
                        )
                        .describe(
                            "Show \u{201c}Editing in RbxNative\u{201d} on your Discord profile.",
                        )
                        .changed(enabled, |shell, cx| shell.set_discord_presence(false, cx)),
                        Row::new(
                            "Hide place and script names",
                            toggle(
                                "discord-hide-names",
                                hide,
                                self.set(move |shell, cx| shell.set_discord_hide_names(!hide, cx)),
                            ),
                        )
                        .describe("Show only that you\u{2019}re in RbxNative.")
                        .indent()
                        .changed(!hide, |shell, cx| shell.set_discord_hide_names(true, cx)),
                    ],
                )
            },
        ]
    }
}
