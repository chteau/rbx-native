//! Strict parsing of a posted sealed state, the Worker's
//! `relay/src/validate.ts` in Rust: anything outside the schema or its
//! limits is refused, unknown keys are dropped, and the sender cannot claim
//! another user's id. The relay never opens `ct`; it only checks its shape.

use serde_json::Value;

use super::room::Refusal;
use crate::schema::{
    Client, LoggedOp, OpsFill, Sealed, SealedEdit, SealedOps, MAX_BODY_BYTES, MAX_EDITS,
    OPS_BODY_BYTES, SCHEMA_VERSION,
};

const BAD: Refusal = Refusal::new(400, "schema");

fn base64_text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| {
            s.len() <= MAX_BODY_BYTES
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
        })
        .map(str::to_owned)
}

fn sealed_edit(value: &Value) -> Option<SealedEdit> {
    let at = value
        .get("at")?
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)?;
    let tag = value
        .get("tag")?
        .as_str()
        .filter(|t| {
            t.len() == 16
                && t.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })?
        .to_owned();
    Some(SealedEdit {
        at: at as u64,
        tag,
        ct: base64_text(value.get("ct")?)?,
    })
}

/// `body` as `user_id`'s sealed state.
pub fn parse_sealed(body: &str, user_id: u64) -> Result<Sealed, Refusal> {
    if body.len() > MAX_BODY_BYTES {
        return Err(Refusal::new(413, "size"));
    }
    let raw: Value = serde_json::from_str(body).map_err(|_| BAD)?;
    let raw = raw.as_object().ok_or(BAD)?;
    if raw.get("v").and_then(Value::as_u64) != Some(u64::from(SCHEMA_VERSION)) {
        return Err(Refusal::new(400, "version"));
    }
    if raw.get("userId").and_then(Value::as_u64) != Some(user_id) {
        return Err(Refusal::new(403, "user"));
    }
    let client = match raw.get("client").and_then(Value::as_str) {
        Some("studio") => Client::Studio,
        Some("rbx-native") => Client::RbxNative,
        _ => return Err(BAD),
    };
    let ct = raw
        .get("ct")
        .and_then(base64_text)
        .filter(|c| !c.is_empty())
        .ok_or(BAD)?;
    let j = match raw.get("j") {
        None => Vec::new(),
        Some(Value::Array(items)) if items.len() <= MAX_EDITS => items
            .iter()
            .map(sealed_edit)
            .collect::<Option<Vec<_>>>()
            .ok_or(BAD)?,
        Some(_) => return Err(BAD),
    };
    Ok(Sealed {
        v: SCHEMA_VERSION,
        user_id,
        client,
        ct,
        j,
        seen: 0,
    })
}

fn version(raw: &serde_json::Map<String, Value>) -> Result<(), Refusal> {
    if raw.get("v").and_then(Value::as_u64) != Some(u64::from(SCHEMA_VERSION)) {
        return Err(Refusal::new(400, "version"));
    }
    Ok(())
}

fn client(value: Option<&Value>) -> Option<Client> {
    match value.and_then(Value::as_str) {
        Some("studio") => Some(Client::Studio),
        Some("rbx-native") => Some(Client::RbxNative),
        _ => None,
    }
}

fn object(body: &str) -> Result<serde_json::Map<String, Value>, Refusal> {
    if body.len() > OPS_BODY_BYTES {
        return Err(Refusal::new(413, "size"));
    }
    match serde_json::from_str(body) {
        Ok(Value::Object(raw)) => Ok(raw),
        _ => Err(BAD),
    }
}

fn ct(value: &Value) -> Option<String> {
    base64_text(value).filter(|c| !c.is_empty())
}

/// `body` as `user_id`'s chunks for the op log: `cts`, or a lone `ct`.
pub fn parse_sealed_ops(body: &str, user_id: u64) -> Result<SealedOps, Refusal> {
    let raw = object(body)?;
    version(&raw)?;
    if raw.get("userId").and_then(Value::as_u64) != Some(user_id) {
        return Err(Refusal::new(403, "user"));
    }
    let client = client(raw.get("client")).ok_or(BAD)?;
    let cts = match (raw.get("cts"), raw.get("ct")) {
        (Some(Value::Array(cts)), None) if !cts.is_empty() => {
            cts.iter().map(ct).collect::<Option<Vec<_>>>()
        }
        (None, Some(one)) => ct(one).map(|c| vec![c]),
        _ => None,
    }
    .ok_or(BAD)?;
    Ok(SealedOps {
        v: SCHEMA_VERSION,
        user_id,
        client,
        cts,
    })
}

/// `body` as a fill (`POST …/ops/fill`): a named log's entries, at
/// consecutive seqs from 1 up. Any key holder may hand over anyone's
/// entries; each stays bound to its sender by its AAD.
pub fn parse_fill(body: &str) -> Result<OpsFill, Refusal> {
    let raw = object(body)?;
    version(&raw)?;
    let log = raw
        .get("log")
        .and_then(Value::as_str)
        .filter(|l| {
            (1..=64).contains(&l.len())
                && l.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or(BAD)?
        .to_owned();
    let entries = raw
        .get("entries")
        .and_then(Value::as_array)
        .filter(|e| !e.is_empty())
        .ok_or(BAD)?
        .iter()
        .map(|e| {
            Some(LoggedOp {
                seq: e.get("seq")?.as_u64().filter(|s| *s >= 1)?,
                user_id: e.get("userId")?.as_u64()?,
                client: client(e.get("client"))?,
                ct: ct(e.get("ct")?)?,
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(BAD)?;
    if entries.windows(2).any(|w| w[1].seq != w[0].seq + 1) {
        return Err(BAD);
    }
    Ok(OpsFill {
        v: SCHEMA_VERSION,
        log,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OP: &str = r#"{"v":2,"userId":5,"client":"rbx-native","ct":"AAAA","x":1}"#;
    const OPS: &str = r#"{"v":2,"userId":5,"client":"studio","cts":["AAAA","BBBB"]}"#;

    #[test]
    fn good_ops_parse_one_or_many() {
        let one = parse_sealed_ops(OP, 5).unwrap();
        assert_eq!(
            (one.cts, one.client),
            (vec!["AAAA".into()], Client::RbxNative)
        );
        let many = parse_sealed_ops(OPS, 5).unwrap();
        assert_eq!(many.cts, ["AAAA", "BBBB"]);
    }

    #[test]
    fn bad_ops_are_refused() {
        let bad = |body: &str| parse_sealed_ops(body, 5).unwrap_err();
        assert_eq!(parse_sealed_ops(OP, 6), Err(Refusal::new(403, "user")));
        assert_eq!(
            bad(&OP.replace(r#""v":2"#, r#""v":1"#)),
            Refusal::new(400, "version")
        );
        assert_eq!(bad(&OP.replace("rbx-native", "x")), BAD);
        assert_eq!(bad(&OP.replace("AAAA", "")), BAD);
        assert_eq!(bad(&OP.replace("AAAA", "A!")), BAD);
        assert_eq!(bad(&OPS.replace("BBBB", "B!")), BAD);
        assert_eq!(bad(&OPS.replace(r#"["AAAA","BBBB"]"#, "[]")), BAD);
        assert_eq!(bad("[]"), BAD);
        assert_eq!(
            bad(&format!("{OP}{}", " ".repeat(OPS_BODY_BYTES))),
            Refusal::new(413, "size")
        );
    }

    const FILL: &str = r#"{"v":2,"log":"ab12","entries":[{"seq":4,"userId":5,"client":"studio","ct":"AAAA"},{"seq":5,"userId":6,"client":"rbx-native","ct":"BBBB"}]}"#;

    #[test]
    fn a_fill_parses_and_keeps_each_sender() {
        let fill = parse_fill(FILL).unwrap();
        assert_eq!(fill.log, "ab12");
        assert_eq!(fill.entries[1].seq, 5);
        assert_eq!(fill.entries[1].user_id, 6);
        assert_eq!(fill.entries[1].client, Client::RbxNative);
    }

    #[test]
    fn a_bad_fill_is_refused() {
        assert_eq!(
            parse_fill(&FILL.replace(r#""seq":5"#, r#""seq":6"#)),
            Err(BAD)
        );
        assert_eq!(parse_fill(&FILL.replace("ab12", "AB12")), Err(BAD));
        assert_eq!(
            parse_fill(&FILL.replace(r#""seq":4"#, r#""seq":0"#)),
            Err(BAD)
        );
        assert_eq!(parse_fill(&FILL.replace("BBBB", "B B")), Err(BAD));
        assert_eq!(
            parse_fill(&FILL.replace(r#""v":2"#, r#""v":1"#)),
            Err(Refusal::new(400, "version"))
        );
        assert_eq!(parse_fill(r#"{"v":2,"log":"ab","entries":[]}"#), Err(BAD));
    }

    const OK: &str = r#"{"v":2,"userId":5,"client":"studio","ct":"AAAA","j":[{"at":10,"tag":"0123456789abcdef","ct":"BBBB"}],"seen":9,"extra":true}"#;

    #[test]
    fn a_good_body_parses_and_unknown_keys_drop() {
        let sealed = parse_sealed(OK, 5).unwrap();
        assert_eq!(sealed.ct, "AAAA");
        assert_eq!(sealed.client, Client::Studio);
        assert_eq!(sealed.j[0].at, 10);
        assert_eq!(sealed.j[0].tag, "0123456789abcdef");
        assert_eq!(sealed.seen, 0);
    }

    #[test]
    fn another_users_id_is_refused() {
        assert_eq!(parse_sealed(OK, 6), Err(Refusal::new(403, "user")));
    }

    #[test]
    fn other_versions_are_refused_as_version() {
        assert_eq!(
            parse_sealed(&OK.replace(r#""v":2"#, r#""v":1"#), 5),
            Err(Refusal::new(400, "version"))
        );
    }

    #[test]
    fn limits_and_shapes_are_enforced() {
        let bad = |body: String| parse_sealed(&body, 5).unwrap_err();
        assert_eq!(bad(OK.replace("studio", "roblox")), BAD);
        assert_eq!(bad(OK.replace(r#""ct":"AAAA""#, r#""ct":"""#)), BAD);
        assert_eq!(bad(OK.replace(r#""ct":"AAAA""#, r#""ct":"AA!A""#)), BAD);
        assert_eq!(bad(OK.replace("0123456789abcdef", "0123456789ABCDEF")), BAD);
        assert_eq!(bad(OK.replace("0123456789abcdef", "0123")), BAD);
        assert_eq!(bad(OK.replace(r#""at":10"#, r#""at":-1"#)), BAD);
        assert_eq!(bad(OK.replace(r#""ct":"BBBB""#, r#""ct":"B B""#)), BAD);
        assert_eq!(
            bad(format!("{}{}", OK, " ".repeat(MAX_BODY_BYTES))),
            Refusal::new(413, "size")
        );
        let entry = r#"{"at":1,"tag":"0123456789abcdef","ct":"AAAA"}"#;
        let many = vec![entry; MAX_EDITS + 1].join(",");
        let body = format!(r#"{{"v":2,"userId":5,"client":"studio","ct":"AAAA","j":[{many}]}}"#);
        assert_eq!(bad(body), BAD);
    }
}
