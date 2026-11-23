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
use crate::shell::play_sandbox;
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
    /// "Create one for me" is running.
    sandbox_busy: bool,
    /// The sandbox's name, read once per universe.
    sandbox_name: Option<(u64, SharedString)>,
    sandbox_name_asked: Option<u64>,
    copy_name: Entity<InputState>,
    /// The setting as the field last showed it, to notice a reset elsewhere.
    copy_name_seen: String,
    accounts_error: Option<SharedString>,
    /// The owner and key name last written to the accounts index, so a
    /// check is recorded once.
    labelled: Option<(SharedString, SharedString, SharedString)>,
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
                sandbox_busy: false,
                sandbox_name: None,
                sandbox_name_asked: None,
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
    /// "Create one for me": finds or makes "RbxNative Sandbox".
    fn create_sandbox(&mut self, cx: &mut Context<Self>) {
        if self.recovery.sandbox_busy {
            return;
        }
        self.recovery.sandbox_busy = true;
        self.recovery.sandbox_error = None;
        let session = self.shell.read(cx).roblox_session();
        cx.spawn(async move |this, cx| {
            let made = cx
                .background_spawn(async move {
                    play_sandbox::provision(rbx_cloud::ApiKey::from_env_or_config(), session)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.recovery.sandbox_busy = false;
                match made {
                    Ok(made) => {
                        this.recovery.sandbox_error = made.warning.map(Into::into);
                        this.shell.update(cx, |shell, cx| {
                            shell.set_play_sandbox(Some(made.sandbox), cx)
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

    /// Reads the current sandbox's name when the universe changed.
    fn load_sandbox_name(&mut self, universe_id: u64, cx: &mut Context<Self>) {
        if self.recovery.sandbox_name_asked == Some(universe_id) {
            return;
        }
        self.recovery.sandbox_name_asked = Some(universe_id);
        cx.spawn(async move |this, cx| {
            let name = cx
                .background_spawn(async move {
                    rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config())
                        .universe(universe_id)
                        .map(|universe| universe.display_name)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(name) = name {
                    this.recovery.sandbox_name = Some((universe_id, name.into()));
                    cx.notify();
                }
            });
        })
        .detach();
    }

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
        if let Some(sandbox) = &current {
            self.load_sandbox_name(sandbox.universe_id, cx);
        }
        let busy = self.recovery.sandbox_busy;
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
                .children(current.is_none().then(|| {
                    ui::button(
                        "sandbox-create",
                        if busy { "Creating\u{2026}" } else { "Create one for me" },
                        ui::Weight::Primary,
                        !busy,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.create_sandbox(cx)))
                }))
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
            let name = match &self.recovery.sandbox_name {
                Some((id, name)) if *id == sandbox.universe_id => format!("{name} \u{b7} "),
                _ => String::new(),
            };
            let universe_id = sandbox.universe_id;
            below = below
                .child(mono(11.5, 16.).child(format!(
                    "{name}Place {} \u{b7} Universe {}",
                    sandbox.place_id, sandbox.universe_id
                )))
                .child(
                    h_flex().child(
                        ui::button(
                            "sandbox-dashboard",
                            "Open on Creator Dashboard",
                            ui::Weight::Secondary,
                            true,
                        )
                        .on_click(move |_, _, cx| {
                            cx.open_url(&play_sandbox::dashboard_url(universe_id))
                        }),
                    ),
                );
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
        #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
        let mut rows = vec![row];
        #[cfg(target_os = "macos")]
        rows.extend(self.network_helper_row(cx));
        Section::new("Play", rows)
    }

    /// macOS: the root helper the network simulator installs once, with a
    /// way to take it off again.
    #[cfg(target_os = "macos")]
    fn network_helper_row(&mut self, cx: &mut Context<Self>) -> Option<Row> {
        use rbx_play::netsim::dummynet;

        if !dummynet::helper_installed() {
            return None;
        }
        let button = secondary_button("netsim-helper-remove", "trash", "Remove").on_click(
            cx.listener(|_, _, _, cx| {
                cx.spawn(async move |this, cx| {
                    let removed = cx
                        .background_spawn(async { dummynet::remove_helper() })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if let Err(error) = removed {
                            this.shell.update(cx, |shell, _| {
                                shell.output.push_warning(&format!(
                                    "could not remove the network helper: {error}"
                                ));
                            });
                        }
                        cx.notify();
                    });
                })
                .detach();
            }),
        );
        Some(Row::new("Network helper", button).describe(
            "Installed the first time Play simulated network conditions, so you\u{2019}re not asked for your password every Play. Remove it and Play asks again next time.",
        ))
    }

    fn updates_section(&mut self, cx: &mut Context<Self>) -> Section {
        use crate::updater::{self, Status};

        let blocked = updater::blocked();
        let automatic = match &blocked {
            None => {
                let on = updater::automatic();
                let shell = self.shell.clone();
                let weak = cx.entity().downgrade();
                Row::new(
                    "Automatic updates",
                    toggle("auto-update", on, move |_, _, cx| {
                        if let Err(error) = updater::set_automatic(!on) {
                            shell.update(cx, |shell, _| {
                                shell
                                    .output
                                    .push_warning(&format!("could not save updates.json: {error}"));
                            });
                        }
                        let _ = weak.update(cx, |_, cx| cx.notify());
                    }),
                )
                .describe("Checks GitHub for a new release when RbxNative starts, and installs it for the next launch. Never interrupts what you\u{2019}re doing.")
                .changed(!on, |_, _| {
                    let _ = updater::set_automatic(true);
                })
            }
            Some(blocked) => {
                Row::new("Automatic updates", readout("Off")).describe(blocked.reason())
            }
        };
        let version = updater::current()
            .map(|v| format!("v{v}"))
            .unwrap_or_else(|| format!("v{} (development build)", env!("CARGO_PKG_VERSION")));
        let status = updater::status();
        let description: SharedString = match &status {
            Status::Idle => format!("You have {version}.").into(),
            Status::Checking => "Checking GitHub for a newer release\u{2026}".into(),
            Status::UpToDate(v) => format!("v{v} is the latest release.").into(),
            Status::Installed(v) => {
                format!("v{v} is installed and runs from the next launch.").into()
            }
            Status::Failed(e) => format!("Couldn\u{2019}t check: {e}").into(),
        };
        let control = if blocked.is_none() {
            let label = if status == Status::Checking {
                "Checking\u{2026}"
            } else {
                "Check now"
            };
            secondary_button("update-check", "refresh-cw", label)
                .on_click(cx.listener(|_, _, _, cx| {
                    if updater::begin() {
                        // Its own thread, like the launch check: a download
                        // can take minutes, too long to hold a pool worker.
                        std::thread::spawn(updater::finish);
                        cx.spawn(async move |this, cx| {
                            while updater::running() {
                                cx.background_executor()
                                    .timer(std::time::Duration::from_millis(250))
                                    .await;
                            }
                            let _ = this.update(cx, |_, cx| cx.notify());
                        })
                        .detach();
                    }
                    cx.notify();
                }))
                .into_any_element()
        } else {
            div().into_any_element()
        };
        Section::new(
            "Updates",
            vec![
                automatic,
                Row::new("Check for updates", control).describe(description),
            ],
        )
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
        let strip = self.shell.read(cx).strip_on_publish();
        let updates = self.updates_section(cx);
        vec![
            Section::new(
                "Publishing",
                vec![Row::new(
                    "Strip editor-only data on publish",
                    toggle(
                        "strip-on-publish",
                        strip,
                        self.set(move |shell, cx| shell.set_strip_on_publish(!strip, cx)),
                    ),
                )
                .describe("Save and Publish to Roblox upload a copy without layers and 3D notes. The local file keeps them.")
                .changed(!strip, |shell, cx| shell.set_strip_on_publish(true, cx))],
            ),
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
                .describe("What Play names a sandbox you picked yourself while it runs a test. The RbxNative Sandbox Play made for you always keeps its name. {place} is the place\u{2019}s name; leave it empty to keep the sandbox\u{2019}s own name.")
                .changed(template != copy_name::DEFAULT, |shell, cx| {
                    shell.set_play_copy_name(copy_name::DEFAULT.to_owned(), cx)
                })],
            ),
            updates,
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
            let check = check.read(cx);
            const TAIL: &str = "stored in your system keychain";
            let summary = launcher::key_summary(check, TAIL);
            // The last check's card stands in while this one runs, which
            // only replaces it if Roblox now says something else. Names are
            // only stored once a check found the key ready, so an account
            // with them was ready last time, also one saved before its line
            // was kept (that one says what it can until the check is back).
            let index = key_store::accounts();
            let cached = index
                .active
                .and_then(|id| index.get(id))
                .filter(|account| !account.owner.is_empty() && !account.key_name.is_empty());
            match cached {
                Some(account) if launcher::key_checking(check) => launcher::KeySummary {
                    name: account.key_name.clone().into(),
                    tag: Some(KeyTag::Ready),
                    meta: if account.ready_meta.is_empty() {
                        format!("{} \u{b7} {TAIL}", account.owner).into()
                    } else {
                        account.ready_meta.clone().into()
                    },
                    owner: Some(account.owner.clone().into()),
                },
                _ => summary,
            }
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
                            .flex_wrap()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                text(12.5, 17.)
                                    .min_w_0()
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
            let labels = (owner.clone(), summary.name.clone(), summary.meta.clone());
            if self.recovery.labelled.as_ref() != Some(&labels) {
                key_store::label_active(&labels.0, &labels.1, &labels.2);
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
                        .min_w_0()
                        .truncate()
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
                            .min_w_0()
                            .truncate()
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
                            .child(icon("trash", 14.))
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
        let studio_presence = &self.shell.read(cx).studio_presence;
        let signed_in = studio_presence.session.is_some();
        let presence = studio_presence.enabled;
        let status = match (signed_in, &studio_presence.user_names) {
            (false, _) => String::new(),
            (true, Some((name, display))) if name.is_empty() || name == display => {
                format!("Signed in as {display}")
            }
            (true, Some((name, display))) => format!("Signed in as {display} (@{name})"),
            (true, None) => "Signed in".to_string(),
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
                            "Used for \u{201c}In Studio\u{201d} on your profile and for archiving from Home. Stored in your system keychain.",
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
                        {
                            let offline = rbx_net::forced();
                            let detected = rbx_net::status() == rbx_net::Status::Detected;
                            let control = if rbx_net::env_forced() {
                                readout("On (RBX_NATIVE_OFFLINE)").into_any_element()
                            } else {
                                toggle(
                                    "offline-mode",
                                    offline,
                                    self.set(move |shell, cx| shell.set_offline(!offline, cx)),
                                )
                                .into_any_element()
                            };
                            let local = crate::beta::enabled(crate::shell::LOCAL_RUN_BETA_ID);
                            Row::new("Offline mode", control)
                            .describe(match (detected, local) {
                                (true, true) => "Roblox can\u{2019}t be reached right now, so RbxNative is offline already. Publishing, cloud places, DataStores and Play (real client) wait for the network; Play (local) and cached assets keep working.",
                                (true, false) => "Roblox can\u{2019}t be reached right now, so RbxNative is offline already. Publishing, cloud places, DataStores and Play wait for the network; cached assets keep working.",
                                (false, true) => "Work without the network: nothing reaches Roblox or the web. Publishing, cloud places, DataStores and Play (real client) are off; Play (local) and cached assets keep working.",
                                (false, false) => "Work without the network: nothing reaches Roblox or the web. Publishing, cloud places, DataStores and Play are off; cached assets keep working.",
                            })
                            .changed(offline, |shell, cx| shell.set_offline(false, cx))
                        },
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
