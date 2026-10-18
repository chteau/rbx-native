//! Payload encryption: the HKDF → ChaCha20-Poly1305 construction the README's
//! "Encryption" section describes. The relay key ([`crate::auth::derive_key`])
//! is offered to the relay in a header, so it cannot be the encryption key;
//! this one is derived from the place secret itself and never leaves a client.
//!
//! Mirrored by `plugins/team-presence/src/Crypto.luau`; both are pinned by
//! `plugins/team-presence/fixtures/crypto_vectors.json`.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::schema::Client;

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;
pub const TAG_LEN: usize = 16;

/// `HKDF-SHA256(ikm = utf8(secret), salt = "", info = "rbx-presence/enc/v1/" + placeId)`, 32 bytes.
pub fn enc_key(secret: &str, place_id: u64) -> [u8; KEY_LEN] {
    let info = format!("rbx-presence/enc/v1/{place_id}");
    let mut okm = [0u8; KEY_LEN];
    Hkdf::<Sha256>::new(None, secret.as_bytes())
        .expand(info.as_bytes(), &mut okm)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    okm
}

/// ChaCha20-Poly1305 (RFC 8439) with the given nonce: `nonce ‖ ciphertext ‖ tag`.
pub fn seal_with_nonce(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let body = cipher
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("ChaCha20-Poly1305 encryption of an in-memory buffer cannot fail");
    let mut out = Vec::with_capacity(NONCE_LEN + body.len());
    out.extend_from_slice(nonce);
    out.extend_from_slice(&body);
    out
}

/// [`seal_with_nonce`] with a fresh random nonce.
pub fn seal(key: &[u8; KEY_LEN], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).expect("the OS random source is available");
    seal_with_nonce(key, &nonce, aad, plaintext)
}

/// Opens `nonce ‖ ciphertext ‖ tag`; `None` if it is too short or fails to authenticate.
pub fn open(key: &[u8; KEY_LEN], aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < NONCE_LEN + TAG_LEN {
        return None;
    }
    let (nonce, body) = sealed.split_at(NONCE_LEN);
    ChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(Nonce::from_slice(nonce), Payload { msg: body, aad })
        .ok()
}

/// Standard base64 with padding.
pub fn b64_encode(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// Standard base64 with padding; `None` on anything else.
pub fn b64_decode(text: &str) -> Option<Vec<u8>> {
    STANDARD.decode(text).ok()
}

/// `"studio"` / `"rbx-native"`, as the schema spells it.
pub fn client_word(client: Client) -> &'static str {
    match client {
        Client::Studio => "studio",
        Client::RbxNative => "rbx-native",
    }
}

/// AAD binding a sealed state to its room and sender:
/// `"rbx-presence/state/v2\n" + room + "\n" + userId + "\n" + client_word`.
pub fn state_aad(room: &str, user_id: u64, client: Client) -> Vec<u8> {
    format!(
        "rbx-presence/state/v2\n{room}\n{user_id}\n{}",
        client_word(client)
    )
    .into_bytes()
}

/// AAD for one journal entry: `"rbx-presence/journal/v2\n" + room + "\n" + userId + "\n" + at`.
pub fn journal_aad(room: &str, user_id: u64, at: u64) -> Vec<u8> {
    format!("rbx-presence/journal/v2\n{room}\n{user_id}\n{at}").into_bytes()
}

/// AAD for one co-editing log entry: `"rbx-presence/ops/v2\n" + room + "\n" + userId + "\n" + client_word`.
pub fn ops_aad(room: &str, user_id: u64, client: Client) -> Vec<u8> {
    format!(
        "rbx-presence/ops/v2\n{room}\n{user_id}\n{}",
        client_word(client)
    )
    .into_bytes()
}

/// The relay's dedupe key for a journal entry, which tells it nothing about
/// the edit: first 16 hex of `HMAC-SHA256(key, path + "\n" + property + "\n" + at)`.
pub fn journal_tag(key: &[u8; KEY_LEN], path: &str, property: &str, at: u64) -> String {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(format!("{path}\n{property}\n{at}").as_bytes());
    hex::encode(mac.finalize().into_bytes())[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hkdf_rfc5869_case3() {
        let ikm = [0x0bu8; 22];
        let mut okm = [0u8; 42];
        Hkdf::<Sha256>::new(None, &ikm)
            .expand(&[], &mut okm)
            .unwrap();
        assert_eq!(
            hex::encode(okm),
            "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
        );
    }

    #[test]
    fn aead_rfc8439_section_2_8_2() {
        let key: [u8; 32] = std::array::from_fn(|i| 0x80 + i as u8);
        let nonce = [
            0x07, 0, 0, 0, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
        ];
        let aad = [
            0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
        ];
        let plain = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let sealed = seal_with_nonce(&key, &nonce, &aad, plain);
        assert_eq!(&sealed[..NONCE_LEN], &nonce);
        assert_eq!(
            hex::encode(&sealed[NONCE_LEN..]),
            "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b61161ae10b594f09e26a7e902ecbd0600691"
        );
        assert_eq!(open(&key, &aad, &sealed).as_deref(), Some(&plain[..]));
    }

    #[test]
    fn round_trip_and_tamper() {
        let key = enc_key("secret", 42);
        let sealed = seal(&key, b"aad", b"hello");
        assert_eq!(open(&key, b"aad", &sealed).as_deref(), Some(&b"hello"[..]));
        assert_eq!(open(&key, b"other", &sealed), None);
        let mut flipped = sealed.clone();
        flipped[NONCE_LEN + 1] ^= 1;
        assert_eq!(open(&key, b"aad", &flipped), None);
        assert_eq!(open(&key, b"aad", &sealed[..10]), None);
        assert_eq!(open(&enc_key("secret", 43), b"aad", &sealed), None);
    }

    #[test]
    fn empty_plaintext_round_trips() {
        let key = enc_key("s", 1);
        let sealed = seal_with_nonce(&key, &[7; NONCE_LEN], b"", b"");
        assert_eq!(sealed.len(), NONCE_LEN + TAG_LEN);
        assert_eq!(open(&key, b"", &sealed), Some(Vec::new()));
    }

    #[test]
    fn base64_round_trip() {
        for len in 0..6usize {
            let bytes: Vec<u8> = (0..len as u8).collect();
            assert_eq!(b64_decode(&b64_encode(&bytes)), Some(bytes));
        }
        assert_eq!(b64_encode(b"ab"), "YWI=");
        assert_eq!(b64_decode("YWI"), None);
        assert_eq!(b64_decode("!!!!"), None);
    }

    #[test]
    fn aads_spell_out_their_parts() {
        assert_eq!(
            state_aad("r", 5, Client::RbxNative),
            b"rbx-presence/state/v2\nr\n5\nrbx-native"
        );
        assert_eq!(
            journal_aad("r", 5, 99),
            b"rbx-presence/journal/v2\nr\n5\n99"
        );
        assert_eq!(
            ops_aad("r", 5, Client::Studio),
            b"rbx-presence/ops/v2\nr\n5\nstudio"
        );
    }

    #[test]
    fn journal_tag_is_16_hex_and_input_sensitive() {
        let key = enc_key("s", 1);
        let tag = journal_tag(&key, "a.b", "Size", 10);
        assert_eq!(tag.len(), 16);
        assert!(tag
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert_ne!(tag, journal_tag(&key, "a.b", "Size", 11));
    }
}
