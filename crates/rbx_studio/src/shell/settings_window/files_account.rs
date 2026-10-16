//! The Files & recovery and Account pages: Auto-Recovery, the name Play
//! gives test copies, the stored Roblox accounts with their switcher,
//! and Discord Rich Presence.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::key_store;
use crate::launcher::{self, ui, KeyTag};
use crate::recovery::{self, INTERVAL_DEFAULT, INTERVAL_MINUTES};
use crate::shell::play::copy_name;
use crate::tokens;

use super::kit::{
    icon, mono, readout, secondary_button, text, ticked_slider, toggle, Row, Section,
};
use super::nav::tilde;
use super::{SettingsWindow, Shell};

/// The Interval row's slider, one stop per minute.
pub(super) struct RecoveryControls {
    interval: Entity<SliderState>,
    sandbox: Entity<InputState>,
    sandbox_error: Option<SharedString>,
    copy_name: Entity<InputState>,
    /// The setting as the field last showed it, to notice a reset elsewhere.
    copy_name_seen: String,
    accounts_error: Option<SharedString>,
    /// The owner and key name last written to the accounts index, so a
    /// check is recorded once.
    labelled: Option<(SharedString, SharedString)>,
}

impl RecoveryControls {
    pub(super) fn new(
        shell: &Entity<Shell>,
        window: &mut Window,
        cx: &mut Context<SettingsWindow>,
    ) -> (Self, Subscription) {
        let sandbox = cx.new(|cx| InputState::new(window, cx).placeholder("Place link or id"));
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
        let copy_name_seen = shell.read(cx).play.copy_name.clone();
        let copy_name = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(copy_name_seen.clone(), window, cx);
            state
        });
        let commit = cx.subscribe(&copy_name, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                let text = input.read(cx).value().to_string();
                this.shell
                    .update(cx, |shell, cx| shell.set_play_copy_name(text, cx));
            }
        });
        (
            RecoveryControls {
                interval,
                sandbox,
                sandbox_error: None,
                copy_name,
                copy_name_seen,
                accounts_error: None,
                labelled: None,
            },
            Subscription::join(subscription, commit),
        )
    }
}

/// Opens Roblox's publishing page; a replaced or removed key is checked again.
fn open_publishing(window: WeakEntity<SettingsWindow>, cx: &mut App) {
    launcher::open_publishing(
        move |cx| {
            let _ = window.update(cx, |this, cx| {
                this.key = None;
                cx.notify();
            });
        },
        cx,
    );
}

impl SettingsWindow {
    fn use_sandbox(&mut self, cx: &mut Context<Self>) {
        let text = self.recovery.sandbox.read(cx).value().trim().to_string();
        let place_id = rbx_cloud::place_id_from_link(&text).or_else(|| text.parse::<u64>().ok());
        let Some(place_id) = place_id else {
            self.recovery.sandbox_error = Some("Not a place link or id.".into());
            cx.notify();
            return;
        };
        self.recovery.sandbox_error = None;
        cx.spawn(async move |this, cx| {
            let resolved = cx
                .background_spawn(async move {
                    rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config())
                        .universe_of_place(place_id)
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match resolved {
                    Ok(None) => {
                        this.recovery.sandbox_error =
                            Some("Roblox has no universe for that place.".into())
                    }
                    Ok(Some(universe_id)) => {
                        this.shell.update(cx, |shell, cx| {
                            shell.set_play_sandbox(
                                Some(crate::settings::PlaySandbox {
                                    place_id,
                                    universe_id,
                                }),
                                cx,
                            )
                        });
                    }
                    Err(error) => this.recovery.sandbox_error = Some(error.into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl SettingsWindow {
    fn play_sandbox_section(&mut self, cx: &mut Context<Self>) -> Section {
        let current = self.shell.read(cx).play.sandbox;
        let input = self.recovery.sandbox.clone();
        let mut row = Row::new(
            "Sandbox place",
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(div().w(px(180.)).child(Input::new(&input)))
                .child(
                    ui::button("sandbox-use", "Use", ui::Weight::Secondary, true)
                        .on_click(cx.listener(|this, _, _, cx| this.use_sandbox(cx))),
                )
                .children(current.as_ref().map(|_| {
                    ui::button("sandbox-clear", "Clear", ui::Weight::Secondary, true).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.shell
                                .update(cx, |shell, cx| shell.set_play_sandbox(None, cx));
                        }),
                    )
                })),
        )
        .describe("A private experience you own. Play publishes your place over it; keep it private and use it for nothing else.");
        let mut below = v_flex().gap(px(4.));
        let mut any = false;
        if let Some(sandbox) = &current {
            any = true;
            below = below.child(mono(11.5, 16.).child(format!(
                "Place {} \u{b7} Universe {}",
                sandbox.place_id, sandbox.universe_id
            )));
        }
        if let Some(error) = &self.recovery.sandbox_error {
            any = true;
            below = below.child(
                text(11.5, 16.)
                    .text_color(tokens::text_error())
                    .child(error.clone()),
            );
        }
        if any {
            row = row.below(below);
        }
        Section::new("Play", vec![row])
    }

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
        let template = self.shell.read(cx).play.copy_name.clone();
        if template != self.recovery.copy_name_seen {
            self.recovery.copy_name_seen = template.clone();
            let value = template.clone();
            self.recovery
                .copy_name
                .update(cx, |state, cx| state.set_value(value, window, cx));
        }
        let copy_input = self.recovery.copy_name.clone();
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
                    div().w(px(200.)).child(Input::new(&copy_input)),
                )
                .describe("What Play names your sandbox while it runs a test. {place} is the place\u{2019}s name; leave it empty to keep the sandbox\u{2019}s own name.")
                .changed(template != copy_name::DEFAULT, |shell, cx| {
                    shell.set_play_copy_name(copy_name::DEFAULT.to_owned(), cx)
                })],
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
                    .on_click({
                        let window = window.clone();
                        move |_, _, cx| open_publishing(window.clone(), cx)
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
        if let (Some(owner), Some(KeyTag::Ready)) = (&summary.owner, summary.tag) {
            let labels = (owner.clone(), summary.name.clone());
            if self.recovery.labelled.as_ref() != Some(&labels) {
                key_store::label_active(&labels.0, &labels.1);
                self.recovery.labelled = Some(labels);
            }
        }
        let index = key_store::accounts();
        let others: Vec<_> = index
            .accounts
            .iter()
            .filter(|account| Some(account.id) != index.active)
            .cloned()
            .collect();
        let mut accounts = v_flex().gap(px(6.));
        accounts = accounts.child(
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
        );
        for account in others {
            let id = account.id;
            let owner: SharedString = if account.owner.is_empty() {
                "Unnamed key".into()
            } else {
                account.owner.clone().into()
            };
            let initial: SharedString = owner
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .collect::<String>()
                .into();
            accounts = accounts.child(
                h_flex()
                    .h(px(40.))
                    .px(px(12.))
                    .gap(px(10.))
                    .items_center()
                    .border_1()
                    .border_color(tokens::border())
                    .rounded(px(6.))
                    .child(
                        h_flex()
                            .size(px(22.))
                            .rounded_full()
                            .items_center()
                            .justify_center()
                            .bg(tokens::accent_soft())
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
                            .child(if account.key_name.is_empty() {
                                String::new()
                            } else {
                                format!("Key \u{201c}{}\u{201d}", account.key_name)
                            }),
                    )
                    .child(
                        secondary_button(("account-switch", id as usize), "repeat", "Switch")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.recovery.accounts_error = None;
                                cx.spawn(async move |this, cx| {
                                    let result = key_store::switch(id, cx).await;
                                    let _ = this.update(cx, |this, cx| {
                                        match result {
                                            Ok(()) => this.key = None,
                                            Err(error) => {
                                                this.recovery.accounts_error =
                                                    Some(error.to_string().into())
                                            }
                                        }
                                        cx.notify();
                                    });
                                })
                                .detach();
                            })),
                    )
                    .child(
                        div()
                            .id(("account-remove", id as usize))
                            .flex_none()
                            .cursor_pointer()
                            .text_color(tokens::text2())
                            .hover(|this| this.text_color(tokens::text_error()))
                            .child(icon("trash-2", 14.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.recovery.accounts_error = None;
                                cx.spawn(async move |this, cx| {
                                    let result = key_store::remove(id, cx).await;
                                    let _ = this.update(cx, |this, cx| {
                                        if let Err(error) = result {
                                            this.recovery.accounts_error =
                                                Some(error.to_string().into());
                                        }
                                        this.key = None;
                                        cx.notify();
                                    });
                                })
                                .detach();
                            })),
                    ),
            );
        }
        accounts = accounts.child(
            h_flex()
                .id("account-add")
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
                .cursor_pointer()
                .hover(|this| this.bg(tokens::secondary_hover()))
                .child(icon("plus", 13.))
                .child("Add another account\u{2019}s key")
                .on_click({
                    let window = window.clone();
                    move |_, _, cx| open_publishing(window.clone(), cx)
                }),
        );
        if let Some(error) = &self.recovery.accounts_error {
            accounts = accounts.child(
                text(11.5, 16.)
                    .text_color(tokens::text_error())
                    .child(error.clone()),
            );
        }
        let signed_in = self.shell.read(cx).studio_presence.session.is_some();
        let user_id = self.shell.read(cx).studio_presence.user_id;
        let presence = self.shell.read(cx).studio_presence.enabled;
        let status = if signed_in {
            match user_id {
                Some(id) => format!("Signed in as user {id}"),
                None => "Signed in".to_string(),
            }
        } else {
            String::new()
        };
        let sign_in_control = if signed_in {
            h_flex()
                .gap(px(10.))
                .items_center()
                .child(text(12., 16.).text_color(tokens::text2()).child(status))
                .child(
                    ui::button("roblox-sign-out", "Sign out", ui::Weight::Secondary, true)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.shell.update(cx, |shell, cx| shell.roblox_sign_out(cx));
                        })),
                )
        } else {
            h_flex().child(
                ui::button(
                    "roblox-sign-in",
                    "Sign in\u{2026}",
                    ui::Weight::Primary,
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.shell
                        .update(cx, |shell, cx| shell.open_roblox_login(cx));
                })),
            )
        };
        let play = self.play_sandbox_section(cx);
        vec![
            Section {
                head: Some(key_card.into_any_element()),
                ..Section::new(
                    "Roblox",
                    vec![
                        Row::new("Accounts", div())
                            .describe(
                                "One key per Roblox account; switch without pasting keys again.",
                            )
                            .below(accounts),
                        Row::new("Roblox sign-in", sign_in_control).describe(
                            "Used only for \u{201c}In Studio\u{201d} on your profile. Stored in your system keychain.",
                        ),
                        Row::new(
                            "Show \u{201c}In Studio\u{201d} on Roblox",
                            toggle(
                                "studio-presence",
                                presence,
                                self.set(move |shell, cx| shell.set_studio_presence(!presence, cx)),
                            ),
                        )
                        .describe(
                            "Your friends see you in Studio, with the place when it\u{2019}s linked.",
                        )
                        .changed(presence, |shell, cx| shell.set_studio_presence(false, cx)),
                    ],
                )
            },
            play,
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
