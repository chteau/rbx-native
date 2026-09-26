//! "Paste your key": the masked key field, the check against Roblox, and
//! the permission table — shared by the setup wizard and the Roblox
//! publishing window.
//!
//! Nothing here stores the key: the owner calls [`KeyCheck::key`] once the
//! user continues, and hands it to `key_store::save`.

use std::collections::HashMap;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::*;
use rbx_cloud::{ApiKey, Client, CloudError, Grant, KeyInfo, KeyReport};

use super::ui::{self};

mod table;
mod view;

pub(super) use table::table;
pub(super) use view::{field, result};

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

pub(crate) struct KeyCheck {
    secret: String,
    revealed: bool,
    pub(super) status: Status,
    pub(super) focus: FocusHandle,
    /// Bumped per check, so a slow answer for an older key is dropped.
    serial: u64,
    pub(super) checked_at: Option<std::time::Instant>,
    /// The permission table's scroll position, so its thumb can show.
    pub(super) scroll: ScrollHandle,
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
            scroll: ScrollHandle::new(),
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

/// The stored key's secret, or a capture fixture's stand-in, or nothing.
pub(super) fn stored_secret() -> String {
    ApiKey::from_env_or_config()
        .map(|key| key.expose_secret().to_string())
        .or_else(|| {
            std::env::var(FIXTURE_VARIABLE)
                .ok()
                .map(|_| "x".repeat(964))
        })
        .unwrap_or_default()
}

/// The tag a key card wears beside its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tag {
    Ready,
    NeedsAttention,
    Refused,
}

/// What a key card says about a check: the key's name, its tag, and one
/// line of detail ending in `tail` once the check is done.
pub(crate) struct Summary {
    pub(crate) name: SharedString,
    pub(crate) tag: Option<Tag>,
    pub(crate) meta: SharedString,
    /// The key owner's name, once the check has one.
    pub(crate) owner: Option<SharedString>,
}

pub(crate) fn summary(check: &KeyCheck, tail: &str) -> Summary {
    let summary = |name: &str, tag, meta: String| Summary {
        name: name.to_owned().into(),
        tag,
        meta: meta.into(),
        owner: None,
    };
    match &check.status {
        Status::Done(checked) => {
            let expiry = if checked.info.expiration_time_utc.is_empty() {
                "never expires".to_string()
            } else {
                format!("expires {}", short_date(&checked.info.expiration_time_utc))
            };
            let tag = if checked.report.ready() {
                Tag::Ready
            } else {
                Tag::NeedsAttention
            };
            Summary {
                owner: Some(checked.owner.clone().into()),
                ..summary(
                    &checked.info.name,
                    Some(tag),
                    format!("{} \u{b7} {expiry} \u{b7} {tail}", checked.owner),
                )
            }
        }
        Status::Running => summary("Your key", None, "Checking with Roblox\u{2026}".into()),
        Status::Idle => summary("No key stored", None, "Replace key to add one.".into()),
        Status::Invalid(status) => summary(
            "Your key",
            Some(Tag::Refused),
            format!("Roblox answered {status}. Replace it with a working key."),
        ),
        Status::Network => summary(
            "Your key",
            None,
            "Couldn\u{2019}t reach Roblox. Check your connection and try again.".into(),
        ),
    }
}
