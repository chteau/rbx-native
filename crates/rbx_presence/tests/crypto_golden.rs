//! Golden vectors shared with `plugins/team-presence/src/Crypto.luau` and the
//! relay's TypeScript. `RBX_PRESENCE_REGEN=1` rebuilds the fixture from the
//! Rust implementation (after checking it against the RFC outputs); a plain
//! run asserts that the checked-in file matches every entry.

use hkdf::Hkdf;
use rbx_presence::crypt;
use rbx_presence::schema::Client;
use serde_json::{json, Value};
use sha2::Sha256;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/team-presence/fixtures/crypto_vectors.json"
);

const RFC8439_PLAINTEXT: &str = "Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
const RFC8439_CIPHERTEXT: &str = "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b6116";
const RFC8439_TAG: &str = "1ae10b594f09e26a7e902ecbd0600691";

fn hkdf_entry(ikm: &[u8], info: &str, len: usize) -> Value {
    let mut okm = vec![0u8; len];
    Hkdf::<Sha256>::new(None, ikm)
        .expand(info.as_bytes(), &mut okm)
        .unwrap();
    json!({"ikm_hex": hex::encode(ikm), "info": info, "okm_hex": hex::encode(okm)})
}

fn aead_entry(key: &[u8; 32], nonce: &[u8; 12], aad: &str, plaintext: &str) -> Value {
    let sealed = crypt::seal_with_nonce(key, nonce, aad.as_bytes(), plaintext.as_bytes());
    json!({
        "key_hex": hex::encode(key),
        "nonce_hex": hex::encode(nonce),
        "aad": aad,
        "aad_hex": hex::encode(aad),
        "plaintext": plaintext,
        "sealed_b64": crypt::b64_encode(&sealed),
    })
}

fn build() -> Value {
    let case3 = hkdf_entry(&[0x0b; 22], "", 42);
    assert_eq!(
        case3["okm_hex"],
        "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
    );
    let ours_ikm: Vec<u8> = (1..=16).collect();
    let hkdf = vec![
        case3,
        hkdf_entry(b"secret", "rbx-presence/enc/v1/42", 32),
        hkdf_entry(&ours_ikm, "rbx-presence/enc/v1/1", 32),
    ];
    assert_eq!(
        hkdf[1]["okm_hex"],
        hex::encode(crypt::enc_key("secret", 42)).as_str()
    );

    let enc_key: Vec<Value> = [("secret", 42u64), ("", 1), ("pässwörd ✓", 104688317714596)]
        .iter()
        .map(|(secret, place)| {
            json!({
                "secret": secret,
                "place_id": place,
                "key_hex": hex::encode(crypt::enc_key(secret, *place)),
            })
        })
        .collect();

    let rfc_key: [u8; 32] = std::array::from_fn(|i| 0x80 + i as u8);
    let rfc_nonce = [
        0x07, 0, 0, 0, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
    ];
    let rfc_aad = [
        0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
    ];
    let rfc_sealed =
        crypt::seal_with_nonce(&rfc_key, &rfc_nonce, &rfc_aad, RFC8439_PLAINTEXT.as_bytes());
    assert_eq!(
        hex::encode(&rfc_sealed[12..]),
        format!("{RFC8439_CIPHERTEXT}{RFC8439_TAG}")
    );
    let rfc_entry = json!({
        "key_hex": hex::encode(rfc_key),
        "nonce_hex": hex::encode(rfc_nonce),
        "aad_hex": hex::encode(rfc_aad),
        "plaintext": RFC8439_PLAINTEXT,
        "sealed_b64": crypt::b64_encode(&rfc_sealed),
    });

    let key = crypt::enc_key("secret", 42);
    let long = "abcdefghij".repeat(100);
    let aead = vec![
        rfc_entry,
        aead_entry(&key, &[7; 12], "", ""),
        aead_entry(&key, &[9; 12], "aad", &long),
        aead_entry(&key, &[1; 12], "ä\n✓", "héllo wörld ✓ 日本語 🎲"),
    ];

    let room = "0123456789abcdef0123456789abcdef";
    let state_key = crypt::enc_key("secret", 42);
    let state_aad = crypt::state_aad(room, 7, Client::RbxNative);
    let state_plain = r##"{"v":2,"userId":7,"name":"Ada","color":"#e8590c","client":"rbx-native","selection":["Workspace.Part"],"edits":[],"seen":0}"##;
    let state_nonce = [3u8; 12];
    let state_sealed =
        crypt::seal_with_nonce(&state_key, &state_nonce, &state_aad, state_plain.as_bytes());
    let state = json!({
        "secret": "secret",
        "place_id": 42,
        "room": room,
        "nonce_hex": hex::encode(state_nonce),
        "aad": String::from_utf8(state_aad).unwrap(),
        "plaintext": state_plain,
        "sealed_b64": crypt::b64_encode(&state_sealed),
    });

    let journal_tag: Vec<Value> = [
        ("secret", 42u64, "Workspace.Part", "Size", 1000u64),
        ("s", 1, "A.B.C", "CFrame", 0),
        ("secret", 42, "Workspace.Ünï", "Name ✓", 1_700_000_000_123),
    ]
    .iter()
    .map(|(secret, place, path, property, at)| {
        let key = crypt::enc_key(secret, *place);
        json!({
            "key_hex": hex::encode(key),
            "path": path,
            "property": property,
            "at": at,
            "tag": crypt::journal_tag(&key, path, property, *at),
        })
    })
    .collect();

    let base64: Vec<Value> = (0..6usize)
        .map(|len| {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 250) as u8).collect();
            json!({"hex": hex::encode(&bytes), "b64": crypt::b64_encode(&bytes)})
        })
        .collect();

    json!({
        "hkdf": hkdf,
        "enc_key": enc_key,
        "aead": aead,
        "state": state,
        "journal_tag": journal_tag,
        "base64": base64,
    })
}

#[test]
fn crypto_vectors_match_fixture() {
    let built = build();
    if std::env::var("RBX_PRESENCE_REGEN").as_deref() == Ok("1") {
        let mut text = serde_json::to_string_pretty(&built).unwrap();
        text.push('\n');
        std::fs::write(FIXTURE, text).unwrap();
        return;
    }
    let text = std::fs::read_to_string(FIXTURE).unwrap();
    let on_disk: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(on_disk, built, "regenerate with RBX_PRESENCE_REGEN=1");
}

#[test]
fn fixture_aead_entries_open() {
    if std::env::var("RBX_PRESENCE_REGEN").as_deref() == Ok("1") {
        return;
    }
    let text = std::fs::read_to_string(FIXTURE).unwrap();
    let fixture: Value = serde_json::from_str(&text).unwrap();
    for entry in fixture["aead"].as_array().unwrap() {
        let key: [u8; 32] = hex::decode(entry["key_hex"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let sealed = crypt::b64_decode(entry["sealed_b64"].as_str().unwrap()).unwrap();
        let aad = hex::decode(entry["aad_hex"].as_str().unwrap()).unwrap();
        let opened = crypt::open(&key, &aad, &sealed);
        assert_eq!(
            opened.as_deref(),
            Some(entry["plaintext"].as_str().unwrap().as_bytes())
        );
    }
}
