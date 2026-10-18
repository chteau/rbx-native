//! Strict parsing of a posted sealed state, the Worker's
//! `relay/src/validate.ts` in Rust: anything outside the schema or its
//! limits is refused, unknown keys are dropped, and the sender cannot claim
//! another user's id. The relay never opens `ct`; it only checks its shape.

use serde_json::Value;

use super::room::Refusal;
use crate::schema::{
    Client, Sealed, SealedEdit, SealedOp, MAX_BODY_BYTES, MAX_EDITS, SCHEMA_VERSION,
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

/// `body` as `user_id`'s op log entry: [`parse_sealed`] without `j`.
pub fn parse_sealed_op(body: &str, user_id: u64) -> Result<SealedOp, Refusal> {
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
    Ok(SealedOp {
        v: SCHEMA_VERSION,
        user_id,
        client,
        ct,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OP: &str = r#"{"v":2,"userId":5,"client":"rbx-native","ct":"AAAA","x":1}"#;

    #[test]
    fn a_good_op_parses() {
        let op = parse_sealed_op(OP, 5).unwrap();
        assert_eq!(op.ct, "AAAA");
        assert_eq!(op.client, Client::RbxNative);
    }

    #[test]
    fn a_bad_op_is_refused() {
        assert_eq!(parse_sealed_op(OP, 6), Err(Refusal::new(403, "user")));
        assert_eq!(
            parse_sealed_op(&OP.replace(r#""v":2"#, r#""v":1"#), 5),
            Err(Refusal::new(400, "version"))
        );
        assert_eq!(parse_sealed_op(&OP.replace("rbx-native", "x"), 5), Err(BAD));
        assert_eq!(parse_sealed_op(&OP.replace("AAAA", ""), 5), Err(BAD));
        assert_eq!(parse_sealed_op(&OP.replace("AAAA", "A!"), 5), Err(BAD));
        assert_eq!(parse_sealed_op("[]", 5), Err(BAD));
        assert_eq!(
            parse_sealed_op(&format!("{OP}{}", " ".repeat(MAX_BODY_BYTES)), 5),
            Err(Refusal::new(413, "size"))
        );
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
