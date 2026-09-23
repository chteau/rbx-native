//! The background OS thread that owns the HTTP connection to `argon serve`
//! — long-polls `POST read`, decodes each [`Message`], and reports back
//! over a channel. Mirrors `workspace_view::pump`'s own idiom (a dedicated
//! `std::thread::spawn` loop, typed messages over `std::sync::mpsc`, drained
//! non-blockingly) rather than an async runtime — the shape every other
//! networked/threaded piece of this codebase already uses, and the one
//! `ureq` (blocking by design) is built for.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use rmpv::Value;

use super::wire::{ArgonRef, Changes, Message, Project, Snapshot};

pub(crate) enum ArgonEvent {
    Connected(Project),
    InitialSnapshot(Snapshot),
    Changes(Changes),
    Error(String),
    Disconnected,
    /// `POST /open` was refused — the plugin logs this at Debug
    /// (`Core/init.luau:393-395`).
    OpenFailed(String),
}

/// `POST read` long-polls server-side; this has to outlast whatever the
/// server's own long-poll window is, or every read looks like a timeout.
const READ_TIMEOUT: Duration = Duration::from_secs(35);
/// Every other call is a single fast round trip.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// This client's own protocol identity, checked against `Project.version`
/// the same way `argon-roblox`'s `SemVer.isCompatible` gates its plugin's
/// connection — major.minor must match exactly.
const CLIENT_VERSION: &str = "2.0.0";

/// The read-loop thread's body. `stop` is polled between each `read` (a
/// long-poll can't be cancelled mid-flight with a blocking client, so a
/// disconnect can take up to [`READ_TIMEOUT`] to actually settle — see
/// `argon_client`'s own doc). `writes` is drained non-blockingly in the same
/// gap, each landing as its own short-lived request (see [`write_changes`]).
#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    host: String,
    port: u16,
    https: bool,
    stop: Arc<AtomicBool>,
    events: Sender<ArgonEvent>,
    writes: Receiver<Changes>,
    opens: Receiver<(ArgonRef, u32)>,
) {
    let scheme = if https { "https" } else { "http" };
    let base = format!("{scheme}://{host}:{port}");
    // A 4xx/5xx is turned into `Err` by default, discarding the body —
    // Argon's server puts its actual error ("Already subscribed", a
    // version mismatch, ...) in that body, so status-as-error is switched
    // off and `call_body` below reads it back out itself.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(CALL_TIMEOUT))
        .build()
        .into();
    let read_agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(READ_TIMEOUT))
        .build()
        .into();
    let client_id = generate_client_id();

    let project = match fetch_details(&agent, &base) {
        Ok(project) => project,
        Err(message) => {
            let _ = events.send(ArgonEvent::Error(message));
            return;
        }
    };
    if !version_compatible(&project.version) {
        let _ = events.send(ArgonEvent::Error(format!(
            "Argon server speaks v{}, this client speaks v{CLIENT_VERSION} — need a matching major.minor",
            project.version
        )));
        return;
    }
    if let Err(message) = subscribe(&agent, &base, client_id) {
        let _ = events.send(ArgonEvent::Error(message));
        return;
    }
    match fetch_snapshot(&agent, &base, ArgonRef::ROOT) {
        Ok(snapshot) => {
            if events.send(ArgonEvent::Connected(project)).is_err() {
                return;
            }
            if events.send(ArgonEvent::InitialSnapshot(snapshot)).is_err() {
                return;
            }
        }
        Err(message) => {
            let _ = events.send(ArgonEvent::Error(message));
            return;
        }
    }

    while !stop.load(Ordering::Relaxed) {
        match writes.try_recv() {
            Ok(changes) => write_changes(&agent, &base, client_id, &changes),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => {}
        }
        match opens.try_recv() {
            Ok((id, line)) => {
                if let Err(message) = open_in_editor(&agent, &base, id, line) {
                    let _ = events.send(ArgonEvent::OpenFailed(message));
                }
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => {}
        }
        match read_once(&read_agent, &base, client_id) {
            Ok(Some(Message::SyncChanges(changes))) => {
                if events.send(ArgonEvent::Changes(changes)).is_err() {
                    break;
                }
            }
            Ok(Some(Message::SyncDetails(project))) => {
                if events.send(ArgonEvent::Connected(project)).is_err() {
                    break;
                }
            }
            // Deliberately dropped — running server-sent code is a
            // boundary this client refuses to cross, see the module doc.
            Ok(Some(Message::ExecuteCode)) => {}
            Ok(Some(Message::Disconnect(reason))) => {
                let _ = events.send(ArgonEvent::Error(reason));
                break;
            }
            Ok(None) => {} // the long-poll itself timed out server-side: read again
            Err(message) => {
                let _ = events.send(ArgonEvent::Error(message));
                break;
            }
        }
    }
    let _ = post_client_id(&agent, &format!("{base}/unsubscribe"), client_id);
    let _ = events.send(ArgonEvent::Disconnected);
}

/// `argon-roblox`'s own `Client:generateId`: a random number, not a real
/// auth token — the server uses it only to route one client's `/read`
/// queue and to reject a duplicate subscription.
fn generate_client_id() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0x9E37_79B9);
    nanos.wrapping_mul(2_654_435_761) & 0x3FFF_FFFF
}

fn major_minor(version: &str) -> Option<(&str, &str)> {
    let mut parts = version.split('.');
    Some((parts.next()?, parts.next()?))
}

/// Exact major.minor match, the same gate `argon-roblox`'s `SemVer.
/// isCompatible` applies (patch is free).
fn version_compatible(server: &str) -> bool {
    major_minor(server) == major_minor(CLIENT_VERSION)
}

fn call_body(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<Vec<u8>, String> {
    let mut response = result.map_err(|err| err.to_string())?;
    let status = response.status();
    let bytes = response
        .body_mut()
        .read_to_vec()
        .map_err(|err| err.to_string())?;
    if status.is_success() {
        return Ok(bytes);
    }
    let body = String::from_utf8_lossy(&bytes);
    Err(format!("http {status}: {body}"))
}

fn decode_body(bytes: &[u8]) -> Result<Value, String> {
    rmpv::decode::read_value(&mut &*bytes).map_err(|err| err.to_string())
}

fn encode_body(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    // Infallible for the shapes this client ever builds (no reader/writer
    // error possible against an in-memory `Vec`).
    rmpv::encode::write_value(&mut bytes, value).expect("encoding an owned Value cannot fail");
    bytes
}

fn post_msgpack(agent: &ureq::Agent, url: &str, body: &Value) -> Result<Value, String> {
    let bytes = encode_body(body);
    let raw = call_body(
        agent
            .post(url)
            .content_type("application/msgpack")
            .send(&bytes),
    )?;
    decode_body(&raw)
}

fn post_client_id(agent: &ureq::Agent, url: &str, client_id: u32) -> Result<(), String> {
    let body = Value::Map(vec![(Value::from("clientId"), Value::from(client_id))]);
    post_msgpack(agent, url, &body)?;
    Ok(())
}

/// `POST subscribe` takes a `name` alongside `clientId` — unlike
/// `unsubscribe`/`read`, which take `clientId` alone (see
/// [`post_client_id`]). `argon-roblox` sends its own plugin's name here;
/// this client identifies itself the same way.
fn subscribe(agent: &ureq::Agent, base: &str, client_id: u32) -> Result<(), String> {
    let body = Value::Map(vec![
        (Value::from("clientId"), Value::from(client_id)),
        (Value::from("name"), Value::from("rbx-native")),
    ]);
    post_msgpack(agent, &format!("{base}/subscribe"), &body)?;
    Ok(())
}

/// `POST /open`: the server opens the file it has for `instance` in the
/// OS default editor (`argon@3dbed6d:src/server/open.rs:12-25`; the
/// plugin sends the same two keys, `Client/init.luau:135-140`).
fn open_in_editor(agent: &ureq::Agent, base: &str, id: ArgonRef, line: u32) -> Result<(), String> {
    let body = Value::Map(vec![
        (Value::from("instance"), id.encode()),
        (Value::from("line"), Value::from(line)),
    ]);
    post_msgpack(agent, &format!("{base}/open"), &body)?;
    Ok(())
}

fn fetch_details(agent: &ureq::Agent, base: &str) -> Result<Project, String> {
    let raw = call_body(agent.get(format!("{base}/details")).call())?;
    let value = decode_body(&raw)?;
    Project::decode(&value).ok_or_else(|| "malformed /details response".to_owned())
}

fn fetch_snapshot(agent: &ureq::Agent, base: &str, root: ArgonRef) -> Result<Snapshot, String> {
    let body = Value::Map(vec![(Value::from("instance"), root.encode())]);
    let value = post_msgpack(agent, &format!("{base}/snapshot"), &body)?;
    Snapshot::decode(&value).ok_or_else(|| "malformed /snapshot response".to_owned())
}

fn read_once(agent: &ureq::Agent, base: &str, client_id: u32) -> Result<Option<Message>, String> {
    let body = Value::Map(vec![(Value::from("clientId"), Value::from(client_id))]);
    match post_msgpack(agent, &format!("{base}/read"), &body) {
        Ok(value) => Ok(Message::decode(&value)),
        // The long-poll's own timeout firing (no change to report yet) is
        // not a connection failure — read again.
        Err(message) if message.to_lowercase().contains("timeout") => Ok(None),
        Err(message) => Err(message),
    }
}

fn write_changes(agent: &ureq::Agent, base: &str, client_id: u32, changes: &Changes) {
    let body = Value::Map(vec![
        (Value::from("clientId"), Value::from(client_id)),
        (Value::from("changes"), changes.encode()),
    ]);
    // Best-effort: a dropped write is surfaced as nothing landing on the
    // dock's "Ns ago" readout rather than a hard error, matching how the
    // reference plugin treats its own two-way sync as opportunistic.
    let _ = post_msgpack(agent, &format!("{base}/write"), &body);
}

#[cfg(test)]
mod tests;
