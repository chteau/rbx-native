//! "In Studio" presence through Roblox's user-heartbeats API, the call real
//! Studio makes every 60 s (`docs/research/play-sandbox/studio-re.md`).
//! Authenticated with the user's `.ROBLOSECURITY` session cookie, not an
//! Open Cloud key, with the usual web CSRF dance: a `403` carrying an
//! `x-csrf-token` header is retried once with that token.
//!
//! Verified live (2026-10-14): `{"studioLocationInfo":{}}` shows "Studio";
//! with an owned place's ids it shows "Studio - <experience name>"; a place
//! the user cannot edit answers `403 {"message":"Unauthorized."}`. Presence
//! outlives the last pulse by more than 4 minutes.

use std::sync::mpsc;
use std::time::Duration;

pub const PULSE_URL: &str = "https://apis.roblox.com/user-heartbeats-api/pulse";
/// Studio's own interval (`FIntUserHeartbeatsPulseIntervalSeconds`).
pub const PULSE_INTERVAL: Duration = Duration::from_secs(60);

/// The `.ROBLOSECURITY` value. `Debug` never prints it.
#[derive(Clone)]
pub struct SessionCookie(String);

impl SessionCookie {
    /// Accepts the bare value or `.ROBLOSECURITY=<value>`; trims whitespace.
    pub fn new(raw: &str) -> Self {
        let raw = raw.trim();
        SessionCookie(
            raw.strip_prefix(".ROBLOSECURITY=")
                .unwrap_or(raw)
                .to_owned(),
        )
    }

    /// The raw value, for storing it in the keyring. Never log it.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    pub(crate) fn value(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SessionCookie {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionCookie(..)")
    }
}

/// Where the user is, as far as presence is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StudioLocation {
    /// The open place, when it is linked to Roblox; both or neither.
    pub place: Option<(u64, u64)>, // (place_id, universe_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PulseError {
    /// Cookie missing, expired or revoked (403 "Unauthorized." with no place).
    Unauthorized,
    /// The cookie is fine but the user cannot edit this place; the caller
    /// should retry without it.
    PlaceNotEditable,
    /// Anything else: transport error or unexpected status, with a short
    /// description that never contains the cookie.
    Other(String),
}

/// Response the transport hands back: status, `x-csrf-token` header, body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub csrf: Option<String>,
    pub body: String,
}

/// The JSON body for one pulse:
/// `{"clientSideTimestampEpochMs": now_ms, "sessionInfo": {"sessionId": session_id},
///   "locationInfo": {"studioLocationInfo": {}}}`, with `"placeId"` and
/// `"universeId"` (numbers) inside `studioLocationInfo` when `place` is set.
pub fn pulse_body(session_id: &str, location: StudioLocation, now_ms: u64) -> serde_json::Value {
    let mut studio = serde_json::Map::new();
    if let Some((place_id, universe_id)) = location.place {
        studio.insert("placeId".to_owned(), place_id.into());
        studio.insert("universeId".to_owned(), universe_id.into());
    }
    serde_json::json!({
        "clientSideTimestampEpochMs": now_ms,
        "sessionInfo": { "sessionId": session_id },
        "locationInfo": { "studioLocationInfo": studio },
    })
}

/// Sends `body` through `post(body, csrf_token)` with the CSRF retry, and
/// classifies the result: 2xx → Ok; 403 with a new csrf header → retry once
/// with it (and remember it in `csrf`); 401/403 → `Unauthorized` when
/// `has_place` is false, `PlaceNotEditable` when true; else `Other`.
pub fn send_pulse(
    body: &serde_json::Value,
    has_place: bool,
    csrf: &mut String,
    post: &mut dyn FnMut(&serde_json::Value, &str) -> Result<Reply, String>,
) -> Result<(), PulseError> {
    for attempt in 0..2 {
        let sent = csrf.clone();
        let reply = post(body, &sent).map_err(PulseError::Other)?;
        if (200..300).contains(&reply.status) {
            return Ok(());
        }
        if reply.status == 403 && attempt == 0 {
            if let Some(new) = reply.csrf.as_ref().filter(|t| **t != sent) {
                *csrf = new.clone();
                continue;
            }
        }
        return Err(match reply.status {
            401 | 403 if has_place => PulseError::PlaceNotEditable,
            401 | 403 => PulseError::Unauthorized,
            status => PulseError::Other(format!("unexpected status {status}")),
        });
    }
    Err(PulseError::Other("csrf retry exhausted".to_owned()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn session_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("OS random number generator");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

type Transport = dyn FnMut(&serde_json::Value, &str) -> Result<Reply, String> + Send;

/// One pulse with the place-fallback retry; `false` means the cookie was refused.
fn pulse(
    session: &str,
    location: StudioLocation,
    fallback: &mut bool,
    csrf: &mut String,
    post: &mut Transport,
    events: &mpsc::Sender<PulserEvent>,
) -> bool {
    loop {
        let place = if *fallback { None } else { location.place };
        let body = pulse_body(session, StudioLocation { place }, now_ms());
        match send_pulse(&body, place.is_some(), csrf, post) {
            Ok(()) => {
                let _ = events.send(PulserEvent::Pulsed);
                return true;
            }
            Err(PulseError::PlaceNotEditable) => *fallback = true,
            Err(PulseError::Unauthorized) => {
                let _ = events.send(PulserEvent::SignedOut);
                return false;
            }
            Err(PulseError::Other(_)) => return true,
        }
    }
}

/// Events the background pulser reports to the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PulserEvent {
    /// A pulse was accepted.
    Pulsed,
    /// The cookie was refused; the pulser has stopped.
    SignedOut,
}

enum PulserMessage {
    Location(StudioLocation),
    Stop,
}

/// Background thread pulsing every `PULSE_INTERVAL`, and right away when the
/// location changes. On `PlaceNotEditable` it falls back to no place for the
/// rest of that location. On `Unauthorized` it sends `SignedOut` and exits.
/// Dropping it stops the thread. A random UUIDv4-shaped session id is made
/// once per pulser.
pub struct Pulser {
    sender: mpsc::Sender<PulserMessage>,
}

impl Pulser {
    /// Starts the thread with a real HTTP transport (ureq, `Cookie:
    /// .ROBLOSECURITY=…`, `Content-Type: application/json`).
    pub fn start(
        cookie: SessionCookie,
        location: StudioLocation,
    ) -> (Self, mpsc::Receiver<PulserEvent>) {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(concat!("rbx-native/", env!("CARGO_PKG_VERSION")))
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .into();
        let cookie_header = format!(".ROBLOSECURITY={}", cookie.value());
        let post = move |body: &serde_json::Value, token: &str| -> Result<Reply, String> {
            let mut req = agent.post(PULSE_URL).header("Cookie", &cookie_header);
            if !token.is_empty() {
                req = req.header("x-csrf-token", token);
            }
            let mut resp = req.send_json(body).map_err(|e| e.to_string())?;
            let status = resp.status().as_u16();
            let csrf = resp
                .headers()
                .get("x-csrf-token")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let body = resp.body_mut().read_to_string().unwrap_or_default();
            Ok(Reply { status, csrf, body })
        };
        Self::start_with(location, PULSE_INTERVAL, Box::new(post))
    }

    /// Same, with an injected transport and interval (for tests).
    pub fn start_with(
        location: StudioLocation,
        interval: Duration,
        mut post: Box<Transport>,
    ) -> (Self, mpsc::Receiver<PulserEvent>) {
        let (sender, messages) = mpsc::channel();
        let (events_tx, events_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let session = session_id();
            let mut location = location;
            let mut fallback = false;
            let mut csrf = String::new();
            if !pulse(
                &session,
                location,
                &mut fallback,
                &mut csrf,
                &mut *post,
                &events_tx,
            ) {
                return;
            }
            loop {
                match messages.recv_timeout(interval) {
                    Ok(PulserMessage::Location(l)) => {
                        location = l;
                        fallback = false;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Ok(PulserMessage::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
                if !pulse(
                    &session,
                    location,
                    &mut fallback,
                    &mut csrf,
                    &mut *post,
                    &events_tx,
                ) {
                    return;
                }
            }
        });
        (Pulser { sender }, events_rx)
    }

    pub fn set_location(&self, location: StudioLocation) {
        let _ = self.sender.send(PulserMessage::Location(location));
    }
}

impl Drop for Pulser {
    fn drop(&mut self) {
        let _ = self.sender.send(PulserMessage::Stop);
    }
}
