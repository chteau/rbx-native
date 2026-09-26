//! The Files & recovery and Account pages. Everything on
//! them but the key card is on the roadmap: autosave, several accounts,
//! and Discord presence are each a planned item of their own.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::launcher::{self, KeyTag};
use crate::tokens;

use super::kit::{
    icon, mono, readout, secondary_button, still_slider, still_toggle, text, Row, Section,
};
use super::nav::tilde;
use super::SettingsWindow;

/// Auto-Recovery's interval stops, in minutes.
const INTERVAL_MINUTES: (u32, u32) = (1, 10);
/// The interval a first run would get: Studio's own default is five
/// minutes or so; four sits on a stop and reads as "a few".
const INTERVAL_DEFAULT: u32 = 4;

impl SettingsWindow {
    pub(super) fn files_page(&self) -> Vec<Section> {
        let (low, high) = INTERVAL_MINUTES;
        let fraction = (INTERVAL_DEFAULT - low) as f32 / (high - low) as f32;
        let folder = crate::settings::default_config_dir()
            .map(|dir| tilde(&dir.join("recovery")))
            .unwrap_or_default();
        vec![
            Section::new(
                "Auto-Recovery",
                vec![
                    Row::new("Auto-Recovery", still_toggle(true))
                        .describe("Save a recovery copy of every open place in the background.")
                        .soon_faded(),
                    Row::new(
                        "Interval",
                        h_flex()
                            .gap(px(10.))
                            .items_center()
                            .child(still_slider(fraction, 180., (high - low + 1) as usize))
                            .child(
                                div()
                                    .opacity(0.4)
                                    .child(readout(format!("{INTERVAL_DEFAULT} min"))),
                            ),
                    )
                    .describe("How often a recovery copy is written.")
                    .indent()
                    .soon_faded(),
                    Row::new(
                        "Recovery folder",
                        secondary_button("open-auto-saves", "folder", "Open auto-saves"),
                    )
                    .describe_mono(folder)
                    .soon(),
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
                    .hover(|this| this.bg(rgba(0x202123FF)))
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
                            .bg(rgba(0x2A2D4AFF))
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
            Section::new(
                "Discord",
                vec![
                    Row::new("Rich Presence", still_toggle(false))
                        .describe(
                            "Show \u{201c}Editing in RbxNative\u{201d} on your Discord profile.",
                        )
                        .soon_faded(),
                    Row::new("Hide place and script names", still_toggle(true))
                        .describe("Show only that you\u{2019}re in RbxNative.")
                        .indent()
                        .soon_faded(),
                ],
            ),
        ]
    }
}
