//! Who may use a room: whoever knows the place's secret (see
//! [`crate::secret`]). Everything is derived from it with HMAC-SHA-256, the
//! same way in Rust, Luau (`Auth.luau`) and TypeScript (`relay/src/auth.ts`);
//! `plugins/team-presence/fixtures/auth_vectors.json` pins all three.
//!
//! - key  = hex(HMAC(secret, "rbx-presence/key/v1/" + place id))
//! - room = first 32 hex digits of hex(SHA-256("rbx-presence/room/v1/" + key))
//! - canonical request = METHOD \n path(+query) \n user id \n unix ms \n hex(SHA-256(body))
//! - signature = hex(HMAC(key, canonical request))
//!
//! HMAC keys are the UTF-8 bytes of the strings as written (the secret and
//! the key are both lower-case hex text), never hex-decoded.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

/// Request header carrying the sender's Roblox user id.
pub const USER_HEADER: &str = "X-Presence-User";
/// Request header carrying the unix-millisecond time the request was signed.
pub const TIME_HEADER: &str = "X-Presence-Time";
/// Request header carrying [`sign`]'s output.
pub const SIG_HEADER: &str = "X-Presence-Sig";
/// Request header carrying the key itself, so the relay can learn it for a
/// room it has not seen (it checks the key against the room id first).
pub const KEY_HEADER: &str = "X-Presence-Key";

/// The room key for `secret` in place `place_id`.
pub fn derive_key(secret: &str, place_id: u64) -> String {
    hmac_hex(secret, &format!("rbx-presence/key/v1/{place_id}"))
}

/// The room id the relay files `key`'s room under.
pub fn room_id(key: &str) -> String {
    let digest = Sha256::digest(format!("rbx-presence/room/v1/{key}").as_bytes());
    hex::encode(digest)[..32].to_string()
}

/// The canonical text of one request.
pub fn canonical(method: &str, path: &str, user_id: u64, ts_ms: u64, body: &[u8]) -> String {
    let body_hash = hex::encode(Sha256::digest(body));
    format!("{method}\n{path}\n{user_id}\n{ts_ms}\n{body_hash}")
}

/// hex(HMAC-SHA-256(key, canonical)).
pub fn sign(key: &str, canonical: &str) -> String {
    hmac_hex(key, canonical)
}

fn hmac_hex(key: &str, message: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// The four header pairs for one request: user, time, signature, and the
/// key when `with_key`.
pub fn headers(
    key: &str,
    method: &str,
    path: &str,
    user_id: u64,
    ts_ms: u64,
    body: &[u8],
    with_key: bool,
) -> Vec<(&'static str, String)> {
    let canonical = canonical(method, path, user_id, ts_ms, body);
    let mut out = vec![
        (USER_HEADER, user_id.to_string()),
        (TIME_HEADER, ts_ms.to_string()),
        (SIG_HEADER, sign(key, &canonical)),
    ];
    if with_key {
        out.push((KEY_HEADER, key.to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vectors() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../../plugins/team-presence/fixtures/auth_vectors.json"
        ))
        .unwrap()
    }

    fn s<'a>(v: &'a serde_json::Value, k: &str) -> &'a str {
        v[k].as_str().unwrap()
    }

    #[test]
    fn sha256_vectors() {
        for case in vectors()["sha256"].as_array().unwrap() {
            let got = hex::encode(Sha256::digest(s(case, "input").as_bytes()));
            assert_eq!(got, s(case, "sha256"));
        }
    }

    #[test]
    fn hmac_vectors() {
        for case in vectors()["hmac"].as_array().unwrap() {
            assert_eq!(
                hmac_hex(s(case, "key"), s(case, "message")),
                s(case, "hmac")
            );
        }
    }

    #[test]
    fn request_vectors() {
        for case in vectors()["requests"].as_array().unwrap() {
            let place = case["placeId"].as_u64().unwrap();
            let user = case["userId"].as_u64().unwrap();
            let ts = case["ts"].as_u64().unwrap();
            let key = derive_key(s(case, "secret"), place);
            assert_eq!(key, s(case, "key"));
            assert_eq!(room_id(&key), s(case, "room"));
            let canon = canonical(
                s(case, "method"),
                s(case, "path"),
                user,
                ts,
                s(case, "body").as_bytes(),
            );
            assert_eq!(canon, s(case, "canonical"));
            assert_eq!(sign(&key, &canon), s(case, "sig"));
        }
    }
}
