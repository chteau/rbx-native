//! The ntfy wire format: how one user's presence is wrapped for a plain
//! [ntfy](https://ntfy.sh) pub/sub server and verified by peers.
//!
//! ntfy stores and relays opaque messages; it does no auth of its own, so the
//! trust is entirely peer-to-peer HMAC, the same key as the Worker relay
//! ([`crate::auth`]). A sender signs its presence and publishes it to the
//! room's topic; every receiver checks the signature against the key it too
//! derived from the place secret. A receiver signs over the message text
//! **exactly as received**, never a re-serialization, so Rust and Luau
//! senders interoperate without agreeing on JSON byte order.
//!
//! Mirrored by `plugins/team-presence/src/Ntfy.luau`; the golden
//! `plugins/team-presence/fixtures/ntfy_vectors.json` pins both.

use serde::{Deserialize, Serialize};

use crate::auth;
use crate::schema::UserState;

/// A signed request's time may differ from the receiver's clock by this much.
pub const CLOCK_SKEW_MS: u64 = 60_000;

/// The published message body: one user's signed presence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// Sender user id.
    pub u: u64,
    /// Unix milliseconds the update was signed.
    pub t: u64,
    /// `hex(HMAC(key, canonical("POST", sign_path(topic), u, t, d)))`.
    pub s: String,
    /// The [`UserState`] as its exact JSON text, signed verbatim.
    pub d: String,
}

/// The canonical path a topic's messages are signed under.
pub fn sign_path(topic: &str) -> String {
    format!("/{topic}")
}

/// A signed envelope string for `state` published to `topic`, or `None` if
/// the state cannot be encoded.
pub fn encode(
    key: &str,
    topic: &str,
    user_id: u64,
    ts_ms: u64,
    state: &UserState,
) -> Option<String> {
    let d = serde_json::to_string(state).ok()?;
    let canonical = auth::canonical("POST", &sign_path(topic), user_id, ts_ms, d.as_bytes());
    let envelope = Envelope {
        u: user_id,
        t: ts_ms,
        s: auth::sign(key, &canonical),
        d,
    };
    serde_json::to_string(&envelope).ok()
}

/// Largest message body a plain ntfy server accepts.
pub const NTFY_MAX_BODY: usize = 4096;

/// Like [`encode`], but sheds detail until the envelope fits
/// [`NTFY_MAX_BODY`]: oldest edits, then selection, then camera, then script.
/// If even the identity alone is too big, the smallest encoding is returned.
pub fn fit_encode(
    key: &str,
    topic: &str,
    user_id: u64,
    ts_ms: u64,
    state: &UserState,
) -> Option<String> {
    let mut state = state.clone();
    loop {
        let body = encode(key, topic, user_id, ts_ms, &state)?;
        if body.len() <= NTFY_MAX_BODY {
            return Some(body);
        }
        let shed = if !state.edits.is_empty() {
            state.edits.remove(0);
            true
        } else if !state.selection.is_empty() {
            state.selection.pop();
            true
        } else if state.camera.is_some() {
            state.camera = None;
            true
        } else if state.script.is_some() {
            state.script = None;
            true
        } else {
            false
        };
        if !shed {
            // Only identity is left and it still does not fit: send the
            // smallest encoding rather than loop forever.
            return Some(body);
        }
    }
}

/// Verifies and decodes `body`: a well-formed envelope whose signature checks
/// against `key` and `topic`, whose `t` is within [`CLOCK_SKEW_MS`] of
/// `now_ms`, and whose state's `userId` matches `u`. Returns the state, or
/// `None` for anything that fails a check.
pub fn decode(key: &str, topic: &str, now_ms: u64, body: &str) -> Option<UserState> {
    let envelope: Envelope = serde_json::from_str(body).ok()?;
    if now_ms.abs_diff(envelope.t) > CLOCK_SKEW_MS {
        return None;
    }
    let canonical = auth::canonical(
        "POST",
        &sign_path(topic),
        envelope.u,
        envelope.t,
        envelope.d.as_bytes(),
    );
    if auth::sign(key, &canonical) != envelope.s {
        return None;
    }
    let state: UserState = serde_json::from_str(&envelope.d).ok()?;
    if state.user_id != envelope.u {
        return None;
    }
    Some(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Client, SCHEMA_VERSION};

    fn vectors() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../../plugins/team-presence/fixtures/ntfy_vectors.json"
        ))
        .unwrap()
    }

    #[test]
    fn signature_vector() {
        let v = vectors();
        let key = v["key"].as_str().unwrap();
        let topic = v["topic"].as_str().unwrap();
        let u = v["u"].as_u64().unwrap();
        let t = v["t"].as_u64().unwrap();
        let d = v["d"].as_str().unwrap();
        let canonical = auth::canonical("POST", &sign_path(topic), u, t, d.as_bytes());
        assert_eq!(auth::sign(key, &canonical), v["s"].as_str().unwrap());
    }

    fn base_state() -> UserState {
        UserState {
            v: SCHEMA_VERSION,
            user_id: 42,
            name: "Ann".into(),
            color: "#e8590c".into(),
            client: Client::RbxNative,
            camera: None,
            selection: vec![],
            script: None,
            edits: vec![],
            seen: 0,
        }
    }

    #[test]
    fn fit_encode_sheds_large_state() {
        let key = "k".repeat(64);
        let topic = "rbxnp-abc";
        let mut state = base_state();
        state.selection = (0..100)
            .map(|i| format!("{i}{}", "a".repeat(400)))
            .collect();
        state.edits = (0..20)
            .map(|i| crate::schema::Edit {
                path: "Workspace.Part".into(),
                property: "Size".into(),
                value: "1, 2, 3".into(),
                at: i,
            })
            .collect();
        let body = fit_encode(&key, topic, 42, 1_000, &state).unwrap();
        assert!(body.len() <= NTFY_MAX_BODY);
        assert_eq!(decode(&key, topic, 1_000, &body).unwrap().user_id, 42);
    }

    #[test]
    fn fit_encode_keeps_small_state() {
        let key = "k".repeat(64);
        let mut state = base_state();
        state.selection = vec!["Workspace.Part".into()];
        let body = fit_encode(&key, "t", 42, 1_000, &state).unwrap();
        assert_eq!(body, encode(&key, "t", 42, 1_000, &state).unwrap());
        let got = decode(&key, "t", 1_000, &body).unwrap();
        assert_eq!(got.user_id, 42);
        assert_eq!(got.name, "Ann");
    }

    #[test]
    fn encode_then_decode_round_trips() {
        let key = "k".repeat(64);
        let topic = "rbxnp-abc";
        let state = UserState {
            v: SCHEMA_VERSION,
            user_id: 42,
            name: "Ann".into(),
            color: "#e8590c".into(),
            client: Client::RbxNative,
            camera: None,
            selection: vec!["Workspace.Part".into()],
            script: None,
            edits: vec![],
            seen: 0,
        };
        let body = encode(&key, topic, 42, 1_000, &state).unwrap();
        let got = decode(&key, topic, 1_000, &body).unwrap();
        assert_eq!(got.user_id, 42);
        assert_eq!(got.selection, vec!["Workspace.Part".to_string()]);
        // Wrong key, stale time, and a mismatched user id are all rejected.
        assert_eq!(decode(&"x".repeat(64), topic, 1_000, &body), None);
        assert_eq!(decode(&key, topic, 1_000_000_000, &body), None);
    }
}
