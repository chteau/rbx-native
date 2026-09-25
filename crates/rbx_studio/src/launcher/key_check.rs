//! "Paste your key": the masked key field, the check against Roblox, and
//! the permission table — shared by the setup wizard and the Roblox
//! publishing window (boards `Setup-Check-*`, `Setup-Key-*`,
//! `Setup-Network`, `Key-Manage`).
//!
//! Nothing here stores the key: the owner calls [`KeyCheck::key`] once the
//! user continues, and hands it to `key_store::save`.

use std::collections::HashMap;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rbx_cloud::{ApiKey, Client, CloudError, Grant, KeyInfo, KeyReport, ScopeCheck};

use super::ui::{self, Weight};
use crate::tokens;

/// What the check found out about a key.
pub(super) struct Checked {
    pub(super) info: KeyInfo,
    pub(super) report: KeyReport,
    /// The key owner's display name, or `User <id>` when that lookup failed.
    pub(super) owner: String,
    /// Names of the universes a restricted scope names, for the lock pill.
    pub(super) universes: HashMap<u64, String>,
}

pub(super) enum Status {
    Idle,
    Running,
    Done(Box<Checked>),
    /// Roblox refused the key outright (introspect 4xx).
    Invalid(u16),
    Network,
}

pub(super) struct KeyCheck {
    secret: String,
    revealed: bool,
    pub(super) status: Status,
    pub(super) focus: FocusHandle,
    /// Bumped per check, so a slow answer for an older key is dropped.
    serial: u64,
    pub(super) checked_at: Option<std::time::Instant>,
}

impl KeyCheck {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        KeyCheck {
            secret: String::new(),
            revealed: false,
            status: Status::Idle,
            focus: cx.focus_handle(),
            serial: 0,
            checked_at: None,
        }
    }

    /// Starts on a key already stored — the publishing window's own.
    pub(super) fn with_key(key: String, cx: &mut Context<Self>) -> Self {
        let mut this = Self::new(cx);
        this.secret = key;
        this.run(cx);
        this
    }

    /// The key once it passed: usable and every required scope granted.
    pub(super) fn key(&self) -> Option<ApiKey> {
        match &self.status {
            Status::Done(checked) if checked.report.ready() => {
                Some(ApiKey::new(self.secret.clone()))
            }
            _ => None,
        }
    }

    /// A stand-in secret for a capture, so the field shows its dots.
    pub(super) fn set_fixture_secret(&mut self) {
        self.secret = "x".repeat(964);
    }

    pub(super) fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        // A key is one long line; a copy from the Dashboard can carry a
        // trailing newline or surrounding spaces.
        let text: String = text.split_whitespace().collect();
        if text.is_empty() {
            return;
        }
        self.secret = text;
        self.run(cx);
    }

    /// Introspects the key off the UI thread, then resolves the owner's name
    /// and the names of any universes a scope is restricted to.
    pub(super) fn run(&mut self, cx: &mut Context<Self>) {
        if self.secret.is_empty() {
            return;
        }
        self.serial += 1;
        let serial = self.serial;
        self.status = Status::Running;
        cx.notify();
        if let Some(status) = fixture_status() {
            self.status = status;
            self.checked_at = Some(std::time::Instant::now());
            return;
        }
        let key = ApiKey::new(self.secret.clone());
        cx.spawn(async move |this, cx| {
            let status = cx.background_spawn(async move { check(key) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.serial == serial {
                    this.status = status;
                    this.checked_at = Some(std::time::Instant::now());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let keystroke = &event.keystroke;
        let command = keystroke.modifiers.control || keystroke.modifiers.platform;
        if command && keystroke.key == "v" {
            self.paste(cx);
            return true;
        }
        if command {
            return false;
        }
        match keystroke.key.as_str() {
            "backspace" => {
                self.secret.pop();
            }
            "enter" => {
                self.run(cx);
                return true;
            }
            _ => match &keystroke.key_char {
                Some(ch) if !ch.chars().any(char::is_whitespace) => self.secret.push_str(ch),
                _ => return false,
            },
        }
        self.status = Status::Idle;
        cx.notify();
        true
    }
}

fn check(key: ApiKey) -> Status {
    let client = Client::new(Some(key));
    let info = match client.introspect() {
        Ok(info) => info,
        Err(CloudError::Http { status, .. }) if (400..500).contains(&status) => {
            return Status::Invalid(status)
        }
        Err(_) => return Status::Network,
    };
    let report = rbx_cloud::check_scopes(&info);
    let owner = client
        .user_display_name(info.authorized_user_id)
        .unwrap_or_else(|_| format!("User {}", info.authorized_user_id));
    let mut universes = HashMap::new();
    for check in &report.checks {
        if let Grant::Universes(ids) = &check.grant {
            for &id in ids {
                if let std::collections::hash_map::Entry::Vacant(slot) = universes.entry(id) {
                    if let Ok(universe) = client.universe(id) {
                        slot.insert(universe.display_name);
                    }
                }
            }
        }
    }
    Status::Done(Box::new(Checked {
        info,
        report,
        owner,
        universes,
    }))
}

/// `Dec 24, 2026` out of `2026-12-24T…`.
pub(super) fn short_date(iso: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = iso.get(..10).unwrap_or("").split('-');
    let (Some(y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next()) else {
        return iso.to_string();
    };
    let month = m
        .parse::<usize>()
        .ok()
        .and_then(|m| MONTHS.get(m.wrapping_sub(1)));
    match (month, d.parse::<u32>()) {
        (Some(month), Ok(day)) => format!("{month} {day}, {y}"),
        _ => iso.to_string(),
    }
}

/// The meta line: `Key “RbxNative” · Cheeteau · expires Dec 24, 2026`.
pub(super) fn meta(checked: &Checked) -> String {
    let expiry = if checked.info.expiration_time_utc.is_empty() {
        "never expires".to_string()
    } else {
        format!("expires {}", short_date(&checked.info.expiration_time_utc))
    };
    format!(
        "Key \u{201c}{}\u{201d} \u{b7} {} \u{b7} {expiry}",
        checked.info.name, checked.owner
    )
}

/// `(granted, total)` for the required and the optional group.
pub(super) fn counts(report: &KeyReport) -> ((usize, usize), (usize, usize)) {
    let count = |required: bool| {
        let rows = report
            .checks
            .iter()
            .filter(|c| c.permission.required == required);
        (
            rows.clone().filter(|c| c.grant.granted()).count(),
            rows.count(),
        )
    };
    (count(true), count(false))
}

// ---------------------------------------------------------------- views

/// The "API key" label, the masked field with its eye, and Paste.
pub(super) fn field(check: &Entity<KeyCheck>, cx: &App) -> impl IntoElement {
    let state = check.read(cx);
    let running = matches!(state.status, Status::Running);
    let shown: SharedString = if state.secret.is_empty() {
        "".into()
    } else if state.revealed {
        state.secret.clone().into()
    } else {
        "\u{2022}"
            .repeat(state.secret.chars().count().min(44))
            .into()
    };
    let empty = state.secret.is_empty();
    let revealed = state.revealed;
    let entity = check.clone();
    v_flex()
        .gap(px(6.))
        .child(
            ui::text(11.5, 16.)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens::text2())
                .child("API key"),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .child(
                    h_flex()
                        .id("key-field")
                        .track_focus(&state.focus)
                        .flex_1()
                        .min_w_0()
                        .h(px(36.))
                        .items_center()
                        .gap(px(8.))
                        .pl(px(12.))
                        .pr(px(6.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(if running {
                            tokens::accent_line()
                        } else {
                            tokens::border2()
                        })
                        .bg(ui::panel2())
                        .cursor_text()
                        .on_click({
                            let focus = state.focus.clone();
                            move |_, window, cx| focus.focus(window, cx)
                        })
                        .on_key_down({
                            let entity = entity.clone();
                            move |event, _, cx| {
                                if entity.update(cx, |this, cx| this.on_key(event, cx)) {
                                    cx.stop_propagation();
                                }
                            }
                        })
                        .child(
                            ui::mono(12., 16.)
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_color(tokens::text())
                                .when(empty, |this| {
                                    this.text_color(tokens::text3())
                                        .font_family(tokens::FONT_FAMILY_UI)
                                        .child("Paste the key you copied from the Dashboard")
                                })
                                .child(shown),
                        )
                        .when(running, |this| {
                            this.child(
                                h_flex()
                                    .gap(px(6.))
                                    .text_color(tokens::text2())
                                    .child(ui::spinner("key-field-spinner", 12.))
                                    .child(ui::text(11.5, 16.).child("Checking\u{2026}")),
                            )
                        })
                        .when(!empty && !running, |this| {
                            this.child(
                                h_flex()
                                    .id("key-reveal")
                                    .size(px(26.))
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(4.))
                                    .text_color(tokens::text2())
                                    .cursor_pointer()
                                    .hover(|this| this.bg(ui::wash()).text_color(tokens::text()))
                                    .child(ui::icon(if revealed { "eye-off" } else { "eye" }, 14.))
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.revealed = !this.revealed;
                                                cx.notify();
                                            })
                                        }
                                    }),
                            )
                        }),
                )
                .child(
                    ui::icon_button(
                        "key-paste",
                        "clipboard-paste",
                        "Paste",
                        Weight::Secondary,
                        false,
                    )
                    .on_click(move |_, _, cx| entity.update(cx, |this, cx| this.paste(cx))),
                ),
        )
}

/// What sits under the field: the running header and skeleton, the result
/// header and table, or one of the whole-key cards. `table_height` is the
/// table's fixed height (`None` fills what is left).
pub(super) fn result(
    check: &Entity<KeyCheck>,
    table_height: Option<f32>,
    cx: &App,
) -> Option<AnyElement> {
    let state = check.read(cx);
    let rerun = {
        let entity = check.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            entity.update(cx, |this, cx| this.run(cx))
        }
    };
    let dashboard = |id: &'static str, small: bool| {
        ui::external_button(id, "Open Creator Dashboard", Weight::Secondary, small)
            .on_click(|_, _, cx| cx.open_url(rbx_cloud::DASHBOARD_API_KEYS_URL))
    };
    Some(match &state.status {
        Status::Idle => return None,
        Status::Running => v_flex()
            .gap(px(12.))
            .child(header(
                div()
                    .size(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(tokens::text2())
                    .child(ui::spinner("key-check-spinner", 16.))
                    .into_any_element(),
                "Checking your key with Roblox\u{2026}",
                "This takes a moment.".into(),
                None,
            ))
            .child(skeleton(table_height))
            .into_any_element(),
        Status::Invalid(status) => card(
            ui::status_dot("x", ui::red(), Some(ui::red_soft()), 22.),
            "Roblox didn\u{2019}t accept this key",
            format!(
                "{status} {}",
                if *status == 401 { "Unauthorized" } else { "Forbidden" }
            ),
            v_flex()
                .gap(px(10.))
                .child(ui::text(12., 18.).text_color(tokens::text2()).child("The usual causes:"))
                .child(
                    v_flex()
                        .gap(px(4.))
                        .pl(px(4.))
                        .children(
                            [
                                "Part of the key is missing. It\u{2019}s one long line; copy it again with Copy Key To Clipboard.",
                                "The key was deleted or regenerated on the Creator Dashboard.",
                                "The key only allows certain IP addresses, and this network isn\u{2019}t one of them.",
                            ]
                            .map(|line| {
                                h_flex()
                                    .items_start()
                                    .gap(px(8.))
                                    .text_color(tokens::text2())
                                    .child(ui::text(12., 18.).child("\u{2022}"))
                                    .child(ui::text(12., 18.).flex_1().child(line))
                            }),
                        ),
                )
                .into_any_element(),
            vec![
                ui::button("key-retry", "Try again", Weight::Secondary, false)
                    .on_click(rerun.clone())
                    .into_any_element(),
                dashboard("key-dashboard", false).into_any_element(),
            ],
        ),
        Status::Network => card(
            ui::status_dot("wifi-off", tokens::text2(), Some(rgba(0xFFFFFF0F)), 22.),
            "Couldn\u{2019}t reach Roblox",
            "network error".into(),
            ui::text(12., 18.)
                .text_color(tokens::text2())
                .child("Check your internet connection and try again. Your key is fine as far as we know; nothing was saved.")
                .into_any_element(),
            vec![ui::button("key-retry", "Try again", Weight::Secondary, false)
                .on_click(rerun.clone())
                .into_any_element()],
        ),
        Status::Done(checked) if !checked.report.usable => {
            let (title, meta, body) = if checked.info.expired {
                (
                    "This key has expired",
                    format!("expired {}", short_date(&checked.info.expiration_time_utc)),
                    "Roblox keeps it, but it no longer works. Set a new expiration on the Creator Dashboard, or paste another key.",
                )
            } else {
                (
                    "This key is disabled",
                    "disabled".to_string(),
                    "Enable it on the Creator Dashboard, then try again.",
                )
            };
            card(
                ui::status_dot("x", ui::red(), Some(ui::red_soft()), 22.),
                title,
                meta,
                ui::text(12., 18.).text_color(tokens::text2()).child(body).into_any_element(),
                vec![
                    dashboard("key-dashboard", false).into_any_element(),
                    ui::button("key-retry", "Try again", Weight::Secondary, false)
                        .on_click(rerun.clone())
                        .into_any_element(),
                ],
            )
        }
        Status::Done(checked) => {
            let missing: Vec<&ScopeCheck> = checked
                .report
                .checks
                .iter()
                .filter(|c| c.permission.required && !c.grant.granted())
                .collect();
            let head = if missing.is_empty() {
                header(
                    ui::status_dot("check", ui::green(), Some(ui::green_soft()), 22.)
                        .into_any_element(),
                    "This key works. You can open and publish your places.",
                    meta(checked),
                    Some(vec![ui::button("key-again", "Check again", Weight::Secondary, true)
                        .on_click(rerun.clone())
                        .into_any_element()]),
                )
            } else {
                let how = missing
                    .iter()
                    .map(|c| {
                        let (system, op) = c.permission.scope.rsplit_once(':').unwrap_or((c.permission.scope, ""));
                        format!("{system} \u{2192} {}", capitalize(op))
                    })
                    .collect::<Vec<_>>()
                    .join(" and ");
                header(
                    ui::status_dot("x", ui::red(), Some(ui::red_soft()), 22.).into_any_element(),
                    if missing.len() == 1 {
                        "1 required permission is missing".to_string()
                    } else {
                        format!("{} required permissions are missing", missing.len())
                    },
                    format!("Edit the key on the Dashboard and add {how}."),
                    Some(vec![
                        ui::external_button("key-open-dashboard", "Open Dashboard", Weight::Secondary, true)
                            .on_click(|_, _, cx| cx.open_url(rbx_cloud::DASHBOARD_API_KEYS_URL))
                            .into_any_element(),
                        ui::icon_button("key-again", "refresh-cw", "Check again", Weight::Secondary, true)
                            .on_click(rerun.clone())
                            .into_any_element(),
                    ]),
                )
            };
            v_flex()
                .gap(px(12.))
                .when(table_height.is_none(), |this| this.flex_1().min_h_0())
                .child(head)
                .child(table(checked, table_height))
                .into_any_element()
        }
    })
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn header(
    glyph: AnyElement,
    title: impl Into<SharedString>,
    meta: String,
    actions: Option<Vec<AnyElement>>,
) -> Div {
    h_flex()
        .items_center()
        .gap(px(10.))
        .child(glyph)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(1.))
                .child(
                    ui::text(12.5, 17.)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens::text())
                        .child(title.into()),
                )
                .child(
                    ui::text(11.5, 16.)
                        .truncate()
                        .text_color(tokens::text2())
                        .child(meta),
                ),
        )
        .when_some(actions, |this, actions| {
            this.child(h_flex().gap(px(8.)).children(actions))
        })
}

/// A whole-key failure: glyph, title, mono status, body, actions.
fn card(
    glyph: Div,
    title: &'static str,
    status: String,
    body: AnyElement,
    actions: Vec<AnyElement>,
) -> AnyElement {
    v_flex()
        .gap(px(10.))
        .p(px(16.))
        .rounded(px(8.))
        .border_1()
        .border_color(tokens::border())
        .bg(ui::panel2())
        .child(
            h_flex()
                .items_center()
                .gap(px(10.))
                .child(glyph)
                .child(
                    ui::text(12.5, 17.)
                        .flex_1()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens::text())
                        .child(title),
                )
                .child(ui::mono(11., 15.).text_color(tokens::text3()).child(status)),
        )
        .child(body)
        .child(h_flex().gap(px(8.)).mt(px(4.)).children(actions))
        .into_any_element()
}

fn frame(height: Option<f32>) -> Stateful<Div> {
    v_flex()
        .id("permission-table")
        .map(|this| match height {
            Some(h) => this.h(px(h)),
            None => this.flex_1().min_h_0(),
        })
        .rounded(px(8.))
        .border_1()
        .border_color(tokens::border())
        .bg(ui::panel2())
        .overflow_y_scroll()
}

fn skeleton(height: Option<f32>) -> impl IntoElement {
    const WIDTHS: [(f32, f32); 11] = [
        (180., 150.),
        (210., 130.),
        (140., 170.),
        (200., 120.),
        (160., 190.),
        (190., 140.),
        (150., 160.),
        (220., 110.),
        (170., 150.),
        (130., 180.),
        (200., 140.),
    ];
    frame(height)
        .overflow_hidden()
        .children(WIDTHS.map(|(a, b)| {
            h_flex()
                .h(px(30.))
                .flex_none()
                .items_center()
                .gap(px(12.))
                .px(px(14.))
                .border_b_1()
                .border_color(tokens::border())
                .child(div().size(px(16.)).rounded_full().bg(ui::wash()))
                .child(div().w(px(a)).h(px(9.)).rounded(px(3.)).bg(ui::wash()))
                .child(div().flex_1())
                .child(
                    div()
                        .w(px(b))
                        .h(px(9.))
                        .rounded(px(3.))
                        .bg(ui::wash_faint()),
                )
        }))
}

/// The permission table: REQUIRED then OPTIONAL, one row per scope.
pub(super) fn table(checked: &Checked, height: Option<f32>) -> impl IntoElement {
    let ((req_on, req_all), (opt_on, opt_all)) = counts(&checked.report);
    let group = |label: &'static str, count: String| {
        h_flex()
            .h(px(28.))
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(14.))
            .bg(ui::panel())
            .border_b_1()
            .border_color(tokens::border())
            .child(
                ui::text(10., 14.)
                    .font_weight(FontWeight::BOLD)
                    .text_color(tokens::text3())
                    .child(label),
            )
            .child(ui::mono(10.5, 14.).text_color(tokens::text3()).child(count))
    };
    let rows = |required: bool| {
        checked
            .report
            .checks
            .iter()
            .filter(move |c| c.permission.required == required)
            .enumerate()
            .map(|(index, c)| row(index, c, &checked.universes))
            .collect::<Vec<_>>()
    };
    frame(height)
        .child(group("REQUIRED", format!("{req_on} of {req_all}")))
        .children(rows(true))
        .child(group(
            "OPTIONAL",
            format!("{opt_on} of {opt_all} on \u{b7} the rest stay off"),
        ))
        .children(rows(false))
}

fn row(index: usize, check: &ScopeCheck, names: &HashMap<u64, String>) -> AnyElement {
    let required = check.permission.required;
    let (dot, label_color, grant): (Div, Rgba, AnyElement) = match &check.grant {
        Grant::Missing if required => (
            ui::status_dot("x", ui::red(), Some(ui::red_soft()), 16.),
            tokens::text(),
            ui::text(11.5, 16.)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(ui::red())
                .child("Missing")
                .into_any_element(),
        ),
        Grant::Missing => (
            ui::status_dot("minus", tokens::text3(), None, 16.),
            tokens::text2(),
            ui::text(11.5, 16.)
                .text_color(tokens::text3())
                .child("Off")
                .into_any_element(),
        ),
        Grant::Everywhere => (
            ui::status_dot("check", ui::green(), Some(ui::green_soft()), 16.),
            tokens::text(),
            ui::text(11.5, 16.)
                .text_color(tokens::text2())
                .child("Everywhere")
                .into_any_element(),
        ),
        Grant::Universes(ids) => {
            let label = match ids.as_slice() {
                [one] => names
                    .get(one)
                    .cloned()
                    .unwrap_or_else(|| format!("Universe {one}")),
                many => format!("{} experiences", many.len()),
            };
            let tip: SharedString = ids
                .iter()
                .map(|id| {
                    names
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| format!("Universe {id}"))
                })
                .collect::<Vec<_>>()
                .join("\n")
                .into();
            (
                ui::status_dot("check", ui::green(), Some(ui::green_soft()), 16.),
                tokens::text(),
                div()
                    .id(SharedString::from(format!(
                        "grant-{}",
                        check.permission.scope
                    )))
                    .max_w_full()
                    .tooltip(move |window, cx| crate::shell::tooltip::text(tip.clone(), window, cx))
                    .child(ui::pill(label, Some("lock"), false))
                    .into_any_element(),
            )
        }
    };
    let _ = index;
    h_flex()
        .h(px(30.))
        .flex_none()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .border_b_1()
        .border_color(tokens::border())
        .child(dot)
        .child(
            ui::text(12., 16.)
                .w(px(230.))
                .flex_none()
                .truncate()
                .text_color(label_color)
                .child(check.permission.feature),
        )
        .child(
            ui::mono(11., 15.)
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(tokens::text3())
                .child(check.permission.scope),
        )
        .child(h_flex().w(px(150.)).flex_none().justify_end().child(grant))
        .into_any_element()
}

/// `RBX_STUDIO_LAUNCHER_KEY=ready|missing|invalid|expired|disabled|network`:
/// the check's answer for a capture, instead of asking Roblox.
fn fixture_status() -> Option<Status> {
    let which = std::env::var(FIXTURE_VARIABLE).ok()?;
    Some(super::fixtures::key_status(&which))
}

pub(super) const FIXTURE_VARIABLE: &str = "RBX_STUDIO_LAUNCHER_KEY";

impl Render for KeyCheck {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
