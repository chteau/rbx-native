//! Offline mode for the whole app: one place that knows whether Roblox and
//! the web can be reached, and one gate every HTTP request goes through.
//!
//! The app is offline when the user says so (Settings › Account › Offline
//! mode, or `RBX_NATIVE_OFFLINE=1`) or when it can't reach Roblox (a probe
//! at startup, again whenever a request fails to connect, and on a timer).
//! Offline, every agent made with [`agent`] or [`config`] refuses requests
//! to the outside straight away with [`OFFLINE`] in the error, instead of
//! waiting on a dead network; requests to this machine (the Play probe's
//! channel, Argon, a local relay) always go through. Features check
//! [`status`] to grey themselves out with [`Status::reason`].

use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ureq::http::{Request, Response};
use ureq::middleware::MiddlewareNext;
use ureq::typestate::AgentScope;
use ureq::{Agent, Body, SendBody};

/// The marker in every error a request refused offline carries, so callers
/// can tell "offline" from a real failure.
pub const OFFLINE: &str = "offline";

/// Where the probe knocks: Roblox's API front door. Any HTTP answer at all
/// means it can be reached.
const PROBE_URL: &str = "https://apis.roblox.com/";
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// How often the probe runs while online, and while offline (to notice the
/// network coming back).
const ONLINE_EVERY: Duration = Duration::from_secs(120);
const OFFLINE_EVERY: Duration = Duration::from_secs(15);
/// A failed connection re-probes at most this often.
const MIN_REPROBE: Duration = Duration::from_secs(5);

/// Where the app stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Online,
    /// Roblox can't be reached (no network, or Roblox is down).
    Detected,
    /// The user turned offline mode on.
    Forced,
}

impl Status {
    pub fn offline(self) -> bool {
        self != Status::Online
    }

    /// Why a feature that needs Roblox is unavailable, for its tooltip or
    /// message; empty online.
    pub fn reason(self) -> &'static str {
        match self {
            Status::Online => "",
            Status::Detected => "Offline: Roblox can\u{2019}t be reached",
            Status::Forced => "Offline mode is on (Settings \u{203a} Account)",
        }
    }
}

static FORCED: AtomicBool = AtomicBool::new(false);
static DETECTED: AtomicBool = AtomicBool::new(false);
/// Milliseconds since the epoch of the last probe start, 0 for never.
static LAST_PROBE: AtomicU64 = AtomicU64::new(0);
/// 0 idle, 1 a probe is running.
static PROBING: AtomicU8 = AtomicU8::new(0);
static WATCHING: Mutex<bool> = Mutex::new(false);

/// Where the app stands now. Cheap: two atomic loads.
pub fn status() -> Status {
    if FORCED.load(Ordering::Relaxed) || env_forced() {
        Status::Forced
    } else if DETECTED.load(Ordering::Relaxed) {
        Status::Detected
    } else {
        Status::Online
    }
}

pub fn is_offline() -> bool {
    status().offline()
}

/// Whether `RBX_NATIVE_OFFLINE` holds the app offline, whatever the
/// setting says.
pub fn env_forced() -> bool {
    std::env::var("RBX_NATIVE_OFFLINE").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Turns offline mode on or off (the Settings toggle). Turning it off
/// checks the network again at once, off the caller's thread.
pub fn set_forced(on: bool) {
    let was = FORCED.swap(on, Ordering::Relaxed);
    if was && !on {
        reprobe(true);
    }
}

pub fn forced() -> bool {
    FORCED.load(Ordering::Relaxed)
}

/// Whether the network answered when last asked; for tests and the probe.
pub fn set_detected(offline: bool) {
    DETECTED.store(offline, Ordering::Relaxed);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Knocks on Roblox once and records the answer; blocking (up to the probe
/// timeout), so never on a UI thread. Skipped while offline mode is on.
pub fn probe() -> Status {
    if FORCED.load(Ordering::Relaxed) || env_forced() {
        return status();
    }
    LAST_PROBE.store(now_ms(), Ordering::Relaxed);
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(PROBE_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let reached = agent.head(PROBE_URL).call().is_ok();
    set_detected(!reached);
    status()
}

/// A probe on a thread of its own, unless one is running or (unless
/// `now`) one ran a moment ago.
fn reprobe(now: bool) {
    let last = LAST_PROBE.load(Ordering::Relaxed);
    if !now && last != 0 && now_ms().saturating_sub(last) < MIN_REPROBE.as_millis() as u64 {
        return;
    }
    if PROBING
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("rbx_net probe".into())
        .spawn(|| {
            probe();
            PROBING.store(0, Ordering::Release);
        });
}

/// Starts the app's watch over the network (once): a probe now, then on a
/// timer, faster while offline.
pub fn watch() {
    let Ok(mut started) = WATCHING.lock() else {
        return;
    };
    if *started {
        return;
    }
    *started = true;
    let _ = std::thread::Builder::new()
        .name("rbx_net watch".into())
        .spawn(|| {
            let mut last = None::<Instant>;
            loop {
                let every = if status() == Status::Detected {
                    OFFLINE_EVERY
                } else {
                    ONLINE_EVERY
                };
                if last.is_none_or(|t| t.elapsed() >= every) {
                    probe();
                    last = Some(Instant::now());
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
}

/// Whether `host` is this machine, which offline mode never cuts off.
fn is_local(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host == "localhost"
        || host.ends_with(".localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified())
}

/// Whether a connection to `host` is refused right now: the check for
/// clients that don't go through ureq (the relay's WebSocket).
pub fn blocks(host: &str) -> bool {
    !is_local(host) && is_offline()
}

/// The error a request refused offline fails with.
pub fn offline_error() -> ureq::Error {
    ureq::Error::Io(io::Error::new(
        io::ErrorKind::NotConnected,
        format!("{OFFLINE}: {}", status().reason()),
    ))
}

/// Whether `error` is a request [`gate`] refused, or a failure to connect
/// at all (what offline looks like from the inside).
pub fn is_offline_error(error: &ureq::Error) -> bool {
    match error {
        ureq::Error::Io(e) => {
            e.kind() == io::ErrorKind::NotConnected && e.to_string().starts_with(OFFLINE)
        }
        _ => false,
    }
}

/// Whether `message` (an error turned to text somewhere up the stack) came
/// from a request refused offline.
pub fn is_offline_message(message: &str) -> bool {
    message.contains(&format!("{OFFLINE}: "))
}

fn connection_failed(error: &ureq::Error) -> bool {
    matches!(
        error,
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed | ureq::Error::Io(_)
    ) || matches!(error, ureq::Error::Timeout(t) if matches!(t, ureq::Timeout::Connect | ureq::Timeout::Resolve))
}

/// The middleware: refuses outside requests while offline, and re-probes
/// when one fails to connect.
pub fn gate(
    request: Request<SendBody>,
    next: MiddlewareNext,
) -> Result<Response<Body>, ureq::Error> {
    let local = request.uri().host().is_some_and(is_local);
    if !local && is_offline() {
        return Err(offline_error());
    }
    let result = next.handle(request);
    if !local {
        match &result {
            Err(error) if connection_failed(error) => reprobe(false),
            Ok(_) if DETECTED.load(Ordering::Relaxed) => set_detected(false),
            _ => {}
        }
    }
    result
}

/// What every request names itself as: the app and its release version,
/// which this crate shares with the editor through the workspace version.
pub const USER_AGENT: &str = concat!("rbx-native/", env!("CARGO_PKG_VERSION"));

/// An agent config with the gate and [`USER_AGENT`] in: what every app agent
/// is built from.
pub fn config() -> ureq::config::ConfigBuilder<AgentScope> {
    Agent::config_builder()
        .middleware(gate)
        .user_agent(USER_AGENT)
}

/// A default agent with the gate in, for one-off requests.
pub fn agent() -> Agent {
    config().build().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_is_never_offline() {
        assert!(is_local("127.0.0.1"));
        assert!(is_local("localhost"));
        assert!(is_local("[::1]"));
        assert!(!is_local("apis.roblox.com"));
        assert!(!is_local("10.0.0.2"));
    }

    #[test]
    fn offline_refuses_at_once_and_says_why() {
        FORCED.store(true, Ordering::Relaxed);
        let started = Instant::now();
        let err = agent()
            .get("https://apis.roblox.com/")
            .call()
            .expect_err("refused");
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(is_offline_error(&err), "{err}");
        assert!(is_offline_message(&err.to_string()), "{err}");
        assert_eq!(status(), Status::Forced);
        assert!(status().reason().contains("Settings"));
        FORCED.store(false, Ordering::Relaxed);
    }
}
