//! The ntfy wire format: how one user's presence is wrapped for a plain
//! [ntfy](https://ntfy.sh) pub/sub server and verified by peers.
//!
//! ntfy stores and relays opaque messages; it does no auth of its own, so the
//! trust is entirely peer-to-peer HMAC, the same key as the Worker relay
//! ([`crate::auth`]). A sender seals its presence ([`crate::seal`]), signs
//! the sealed JSON and publishes it to the room's topic; every receiver
//! checks the signature, then opens the state with the encryption key it too
//! derived from the place secret. A receiver signs over the message text
//! **exactly as received**, never a re-serialization, so Rust and Luau
//! senders interoperate without agreeing on JSON byte order.
//!
//! Mirrored by `plugins/team-presence/src/Ntfy.luau`; the golden
//! `plugins/team-presence/fixtures/ntfy_vectors.json` pins both.

use serde::{Deserialize, Serialize};

use crate::auth;
use crate::schema::{Sealed, UserState};
use crate::seal;

/// A signed request's time may differ from the receiver's clock by this much.
pub const CLOCK_SKEW_MS: u64 = 60_000;

/// The published message body: one user's signed, sealed presence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// Sender user id.
    pub u: u64,
    /// Unix milliseconds the update was signed.
    pub t: u64,
    /// `hex(HMAC(key, canonical("POST", sign_path(topic), u, t, d)))`.
    pub s: String,
    /// The [`Sealed`] state as its exact JSON text, signed verbatim.
    pub d: String,
}

/// The canonical path a topic's messages are signed under.
pub fn sign_path(topic: &str) -> String {
    format!("/{topic}")
}

/// A signed envelope string for `state` sealed for `room` and published to
/// `topic`, or `None` if it cannot be encoded.
pub fn encode(
    key: &str,
    enc_key: &[u8; 32],
    room: &str,
    topic: &str,
    user_id: u64,
    ts_ms: u64,
    state: &UserState,
) -> Option<String> {
    let d = serde_json::to_string(&seal::seal_state(enc_key, room, state)).ok()?;
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

/// Like [`encode`], but sheds detail until the encrypted envelope fits
/// [`NTFY_MAX_BODY`]: oldest edits, then selection, then camera, then script.
/// If even the identity alone is too big, the smallest encoding is returned.
pub fn fit_encode(
    key: &str,
    enc_key: &[u8; 32],
    room: &str,
    topic: &str,
    user_id: u64,
    ts_ms: u64,
    state: &UserState,
) -> Option<String> {
    let mut state = state.clone();
    state.clamp();
    loop {
        let body = encode(key, enc_key, room, topic, user_id, ts_ms, &state)?;
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

/// Verifies and opens `body`: a well-formed envelope whose signature checks
/// against `key` and `topic`, whose `t` is within [`CLOCK_SKEW_MS`] of
/// `now_ms`, whose sealed `userId` matches `u`, and whose state opens with
/// `enc_key` for `room`. `None` for anything that fails a check.
pub fn decode(
    key: &str,
    enc_key: &[u8; 32],
    room: &str,
    topic: &str,
    now_ms: u64,
    body: &str,
) -> Option<UserState> {
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
    let sealed: Sealed = serde_json::from_str(&envelope.d).ok()?;
    if sealed.user_id != envelope.u {
        return None;
    }
    seal::open_state(enc_key, room, &sealed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypt;
    use crate::schema::{Client, Edit, SCHEMA_VERSION};

    const SECRET: &str = "correct horse battery staple";
    const PLACE: u64 = 104_688_317_714_596;
    const USER: u64 = 1_234_567;
    const TIME: u64 = 1_700_000_000_000;

    fn fixture_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/team-presence/fixtures/ntfy_vectors.json")
    }

    fn fixture_state() -> UserState {
        UserState {
            v: SCHEMA_VERSION,
            user_id: USER,
            name: "Ana".into(),
            color: "#e8590c".into(),
            client: Client::Studio,
            camera: None,
            selection: vec!["Workspace.Part".into()],
            script: None,
            edits: vec![Edit {
                path: "Workspace.Part".into(),
                property: "Size".into(),
                value: "1, 2, 3".into(),
                at: TIME - 500,
            }],
            seen: 0,
        }
    }

    fn regenerate() {
        let key = auth::derive_key(SECRET, PLACE);
        let room = auth::room_id(&key);
        let topic = crate::relay::ntfy_topic(&room);
        let enc_key = crypt::enc_key(SECRET, PLACE);
        let state = fixture_state();
        let body = encode(&key, &enc_key, &room, &topic, USER, TIME, &state).unwrap();
        let envelope: Envelope = serde_json::from_str(&body).unwrap();
        let fixture = serde_json::json!({
            "secret": SECRET,
            "place_id": PLACE,
            "key": key,
            "room": room,
            "topic": topic,
            "u": envelope.u,
            "t": envelope.t,
            "d": envelope.d,
            "s": envelope.s,
            "state": state,
        });
        let mut text = serde_json::to_string_pretty(&fixture).unwrap();
        text.push('\n');
        std::fs::write(fixture_path(), text).unwrap();
    }

    #[test]
    fn golden_vector_verifies_and_opens() {
        if std::env::var("RBX_PRESENCE_REGEN").as_deref() == Ok("1") {
            regenerate();
        }
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fixture_path()).unwrap()).unwrap();
        let secret = v["secret"].as_str().unwrap();
        let place = v["place_id"].as_u64().unwrap();
        let key = v["key"].as_str().unwrap();
        let room = v["room"].as_str().unwrap();
        let topic = v["topic"].as_str().unwrap();
        let u = v["u"].as_u64().unwrap();
        let t = v["t"].as_u64().unwrap();
        let d = v["d"].as_str().unwrap();
        let s = v["s"].as_str().unwrap();
        let canonical = auth::canonical("POST", &sign_path(topic), u, t, d.as_bytes());
        assert_eq!(auth::sign(key, &canonical), s);
        let body = serde_json::to_string(&Envelope {
            u,
            t,
            s: s.to_owned(),
            d: d.to_owned(),
        })
        .unwrap();
        let enc_key = crypt::enc_key(secret, place);
        let expected: UserState = serde_json::from_value(v["state"].clone()).unwrap();
        assert_eq!(decode(key, &enc_key, room, topic, t, &body), Some(expected));
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
        let enc_key = crypt::enc_key("s", 1);
        let topic = "rbxnp-abc";
        let mut state = base_state();
        state.selection = (0..100)
            .map(|i| format!("{i}{}", "a".repeat(400)))
            .collect();
        state.edits = (0..20)
            .map(|i| Edit {
                path: "Workspace.Part".into(),
                property: "Size".into(),
                value: "1, 2, 3".into(),
                at: i,
            })
            .collect();
        let body = fit_encode(&key, &enc_key, "room", topic, 42, 1_000, &state).unwrap();
        assert!(body.len() <= NTFY_MAX_BODY);
        let got = decode(&key, &enc_key, "room", topic, 1_000, &body).unwrap();
        assert_eq!(got.user_id, 42);
        assert!(got.edits.is_empty());
    }

    #[test]
    fn fit_encode_keeps_small_state() {
        let key = "k".repeat(64);
        let enc_key = crypt::enc_key("s", 1);
        let mut state = base_state();
        state.selection = vec!["Workspace.Part".into()];
        let body = fit_encode(&key, &enc_key, "room", "t", 42, 1_000, &state).unwrap();
        assert_eq!(
            decode(&key, &enc_key, "room", "t", 1_000, &body),
            Some(state)
        );
    }

    #[test]
    fn encode_then_decode_round_trips() {
        let key = "k".repeat(64);
        let enc_key = crypt::enc_key("s", 1);
        let topic = "rbxnp-abc";
        let mut state = base_state();
        state.selection = vec!["Workspace.Part".into()];
        let body = encode(&key, &enc_key, "room", topic, 42, 1_000, &state).unwrap();
        assert!(!body.contains("Workspace.Part"));
        assert_eq!(
            decode(&key, &enc_key, "room", topic, 1_000, &body),
            Some(state)
        );
        assert_eq!(
            decode(&"x".repeat(64), &enc_key, "room", topic, 1_000, &body),
            None
        );
        assert_eq!(
            decode(&key, &enc_key, "room", topic, 1_000_000_000, &body),
            None
        );
        assert_eq!(
            decode(&key, &crypt::enc_key("s", 2), "room", topic, 1_000, &body),
            None
        );
        assert_eq!(decode(&key, &enc_key, "other", topic, 1_000, &body), None);
    }

    #[test]
    fn mismatched_user_is_rejected() {
        let key = "k".repeat(64);
        let enc_key = crypt::enc_key("s", 1);
        let body = encode(&key, &enc_key, "room", "t", 43, 1_000, &base_state()).unwrap();
        assert_eq!(decode(&key, &enc_key, "room", "t", 1_000, &body), None);
    }
}
