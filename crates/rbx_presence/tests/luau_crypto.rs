//! Runs the plugin's `Crypto.luau` in plain Luau (mlua) against the RFC test
//! vectors and the shared golden file `fixtures/crypto_vectors.json`, so the
//! plugin's encryption cannot drift from `rbx_presence::crypt`.

mod common;

use std::time::Instant;

use common::{load_module, new_lua, root};
use mlua::{Function, Lua, Value};
use serde_json::Value as Json;

fn crypto(lua: &Lua, name: &str) -> Function {
    load_module(lua, "Crypto")
        .unwrap()
        .get(name)
        .unwrap_or_else(|e| panic!("Crypto.{name}: {e}"))
}

fn vectors() -> Json {
    let path = root().join("plugins/team-presence/fixtures/crypto_vectors.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("fixture is JSON")
}

fn unhex(text: &str) -> Vec<u8> {
    hex::decode(text).unwrap_or_else(|e| panic!("bad hex {text:?}: {e}"))
}

/// `<name>_hex` decoded, else `<name>` as UTF-8, else the camelCase `<name>Hex`.
fn bytes_field(case: &Json, name: &str) -> Vec<u8> {
    let camel = format!("{name}Hex");
    let snake = format!("{name}_hex");
    if let Some(text) = case[&snake].as_str().or_else(|| case[&camel].as_str()) {
        return unhex(text);
    }
    case[name]
        .as_str()
        .unwrap_or_else(|| panic!("missing {name} in {case}"))
        .as_bytes()
        .to_vec()
}

fn str_any<'a>(case: &'a Json, names: &[&str]) -> &'a str {
    names
        .iter()
        .find_map(|n| case[*n].as_str())
        .unwrap_or_else(|| panic!("missing one of {names:?} in {case}"))
}

fn num_any(case: &Json, names: &[&str]) -> f64 {
    names
        .iter()
        .find_map(|n| case[*n].as_f64())
        .unwrap_or_else(|| panic!("missing one of {names:?} in {case}"))
}

fn cases<'a>(v: &'a Json, names: &[&str]) -> Vec<&'a Json> {
    names
        .iter()
        .filter_map(|n| v[*n].as_array())
        .flatten()
        .collect()
}

fn lstr(lua: &Lua, bytes: &[u8]) -> mlua::LuaString {
    lua.create_string(bytes).unwrap()
}

fn raw(value: Value) -> Option<Vec<u8>> {
    match value {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        Value::Nil => None,
        other => panic!("expected string or nil, got {other:?}"),
    }
}

fn call_bytes(f: &Function, args: impl mlua::IntoLuaMulti) -> Option<Vec<u8>> {
    raw(f.call::<Value>(args).unwrap())
}

#[test]
fn hkdf_rfc5869() {
    let lua = new_lua();
    let hkdf = crypto(&lua, "hkdf");
    let ikm = [0x0bu8; 22];
    let case1 = call_bytes(
        &hkdf,
        (
            lstr(&lua, &ikm),
            lstr(&lua, &unhex("000102030405060708090a0b0c")),
            lstr(&lua, &unhex("f0f1f2f3f4f5f6f7f8f9")),
            42,
        ),
    )
    .unwrap();
    assert_eq!(
        hex::encode(case1),
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
    );
    let case3 = call_bytes(&hkdf, (lstr(&lua, &ikm), "", "", 42)).unwrap();
    assert_eq!(
        hex::encode(case3),
        "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
    );
}

#[test]
fn aead_rfc8439() {
    let lua = new_lua();
    let seal = crypto(&lua, "seal");
    let open = crypto(&lua, "open");
    let key: Vec<u8> = (0x80u8..=0x9f).collect();
    let nonce = unhex("070000004041424344454647");
    let aad = unhex("50515253c0c1c2c3c4c5c6c7");
    let plain: &[u8] = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let sealed = call_bytes(
        &seal,
        (
            lstr(&lua, &key),
            lstr(&lua, &nonce),
            lstr(&lua, &aad),
            lstr(&lua, plain),
        ),
    )
    .unwrap();
    assert_eq!(sealed.len(), 12 + plain.len() + 16);
    assert_eq!(&sealed[..12], &nonce[..]);
    assert_eq!(
        hex::encode(&sealed[12..28]),
        "d31a8d34648e60db7b86afbc53ef7ec2"
    );
    assert_eq!(
        hex::encode(&sealed[sealed.len() - 16..]),
        "1ae10b594f09e26a7e902ecbd0600691"
    );
    let opened = call_bytes(
        &open,
        (lstr(&lua, &key), lstr(&lua, &aad), lstr(&lua, &sealed)),
    );
    assert_eq!(opened.as_deref(), Some(plain));
    let mut tampered = sealed.clone();
    tampered[20] ^= 1;
    assert_eq!(
        call_bytes(
            &open,
            (lstr(&lua, &key), lstr(&lua, &aad), lstr(&lua, &tampered))
        ),
        None
    );
    assert_eq!(
        call_bytes(
            &open,
            (
                lstr(&lua, &key),
                lstr(&lua, &aad),
                lstr(&lua, &sealed[..27])
            )
        ),
        None
    );
}

#[test]
fn fixture_hkdf_and_enc_key() {
    let lua = new_lua();
    let v = vectors();
    let hkdf = crypto(&lua, "hkdf");
    for case in cases(&v, &["hkdf"]) {
        let okm = bytes_field(case, "okm");
        let salt = if case.get("salt").is_some() || case.get("salt_hex").is_some() {
            bytes_field(case, "salt")
        } else {
            Vec::new()
        };
        let got = call_bytes(
            &hkdf,
            (
                lstr(&lua, &bytes_field(case, "ikm")),
                lstr(&lua, &salt),
                lstr(&lua, &bytes_field(case, "info")),
                okm.len(),
            ),
        )
        .unwrap();
        assert_eq!(got, okm, "hkdf {case}");
    }
    let enc_key = crypto(&lua, "encKey");
    for case in cases(&v, &["enc_key", "encKey"]) {
        let got = call_bytes(
            &enc_key,
            (
                str_any(case, &["secret"]),
                num_any(case, &["place_id", "placeId"]),
            ),
        )
        .unwrap();
        assert_eq!(got, bytes_field(case, "key"), "encKey {case}");
    }
}

#[test]
fn fixture_aead() {
    let lua = new_lua();
    let v = vectors();
    let seal_b64 = crypto(&lua, "sealB64");
    let open_b64 = crypto(&lua, "openB64");
    let open = crypto(&lua, "open");
    let b64decode = crypto(&lua, "b64decode");
    for case in cases(&v, &["aead", "seal"]) {
        let key = lstr(&lua, &bytes_field(case, "key"));
        let nonce = bytes_field(case, "nonce");
        let aad = bytes_field(case, "aad");
        let plain = bytes_field(case, "plaintext");
        let expected = str_any(case, &["sealed_b64", "sealedB64"]);
        let got: String = seal_b64
            .call((
                key.clone(),
                lstr(&lua, &nonce),
                lstr(&lua, &aad),
                lstr(&lua, &plain),
            ))
            .unwrap();
        assert_eq!(got, expected, "seal {case}");
        let opened = call_bytes(&open_b64, (key.clone(), lstr(&lua, &aad), expected));
        assert_eq!(opened.as_deref(), Some(&plain[..]), "open {case}");
        let mut sealed = call_bytes(&b64decode, expected).unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0x80;
        assert_eq!(
            call_bytes(&open, (key.clone(), lstr(&lua, &aad), lstr(&lua, &sealed))),
            None,
            "tampered {case}"
        );
        let mut wrong_aad = aad.clone();
        wrong_aad.push(b'x');
        assert_eq!(
            call_bytes(&open_b64, (key, lstr(&lua, &wrong_aad), expected)),
            None,
            "wrong aad {case}"
        );
    }
}

#[test]
fn fixture_journal_tag_and_base64() {
    let lua = new_lua();
    let v = vectors();
    let journal_tag = crypto(&lua, "journalTag");
    for case in cases(&v, &["journal_tag", "journalTag"]) {
        let got: String = journal_tag
            .call((
                lstr(&lua, &bytes_field(case, "key")),
                str_any(case, &["path"]),
                str_any(case, &["property"]),
                num_any(case, &["at"]),
            ))
            .unwrap();
        assert_eq!(got, str_any(case, &["tag"]), "journalTag {case}");
    }
    let enc = crypto(&lua, "b64encode");
    let dec = crypto(&lua, "b64decode");
    for case in cases(&v, &["base64", "b64"]) {
        let bytes = unhex(str_any(case, &["hex"]));
        let text = str_any(case, &["b64", "base64", "text"]);
        let got: String = enc.call(lstr(&lua, &bytes)).unwrap();
        assert_eq!(got, text, "b64encode {case}");
        assert_eq!(call_bytes(&dec, text), Some(bytes), "b64decode {case}");
    }
    for len in 0..8u8 {
        let bytes: Vec<u8> = (0..len).map(|i| i.wrapping_mul(97)).collect();
        let text: String = enc.call(lstr(&lua, &bytes)).unwrap();
        assert_eq!(call_bytes(&dec, text), Some(bytes));
    }
    assert_eq!(enc.call::<String>("ab").unwrap(), "YWI=");
    assert_eq!(call_bytes(&dec, "YWI"), None);
    assert_eq!(call_bytes(&dec, "!!!!"), None);
}

#[test]
fn fixture_state() {
    let lua = new_lua();
    let v = vectors();
    let case = &v["state"];
    let key: mlua::LuaString = crypto(&lua, "encKey")
        .call((str_any(case, &["secret"]), num_any(case, &["place_id"])))
        .unwrap();
    let aad: String = crypto(&lua, "stateAad")
        .call((str_any(case, &["room"]), 7, "rbx-native"))
        .unwrap();
    assert_eq!(aad, str_any(case, &["aad"]));
    let plain = str_any(case, &["plaintext"]);
    let expected = str_any(case, &["sealed_b64"]);
    let got: String = crypto(&lua, "sealB64")
        .call((
            key.clone(),
            lstr(&lua, &bytes_field(case, "nonce")),
            aad.as_str(),
            plain,
        ))
        .unwrap();
    assert_eq!(got, expected);
    let opened = call_bytes(&crypto(&lua, "openB64"), (key, aad.as_str(), expected));
    assert_eq!(opened.as_deref(), Some(plain.as_bytes()));
}

#[test]
fn aads_spell_out_their_parts() {
    let lua = new_lua();
    let state: String = crypto(&lua, "stateAad")
        .call(("r", 5, "rbx-native"))
        .unwrap();
    assert_eq!(state, "rbx-presence/state/v2\nr\n5\nrbx-native");
    let journal: String = crypto(&lua, "journalAad").call(("r", 5, 99)).unwrap();
    assert_eq!(journal, "rbx-presence/journal/v2\nr\n5\n99");
    let ops: String = crypto(&lua, "opsAad").call(("r", 5, "studio")).unwrap();
    assert_eq!(ops, "rbx-presence/ops/v2\nr\n5\nstudio");
}

#[test]
fn sixteen_kib_round_trip_is_fast() {
    let lua = new_lua();
    let key: Vec<u8> = crypto(&lua, "encKey")
        .call::<mlua::LuaString>(("secret", 42))
        .unwrap()
        .as_bytes()
        .to_vec();
    let plain: Vec<u8> = (0..16 * 1024u32).map(|i| (i * 31 % 251) as u8).collect();
    let started = Instant::now();
    let sealed = call_bytes(
        &crypto(&lua, "seal"),
        (
            lstr(&lua, &key),
            lstr(&lua, &[9u8; 12]),
            "aad",
            lstr(&lua, &plain),
        ),
    )
    .unwrap();
    let opened = call_bytes(
        &crypto(&lua, "open"),
        (lstr(&lua, &key), "aad", lstr(&lua, &sealed)),
    );
    let elapsed = started.elapsed();
    assert_eq!(opened, Some(plain));
    assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
}
