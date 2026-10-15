//! Strict parsing of a posted state, the Worker's `relay/src/validate.ts`
//! in Rust: anything outside the schema or its limits is refused, unknown
//! keys are dropped, and the sender cannot claim another user's id.

use serde_json::Value;

use super::room::Refusal;
use crate::schema::{
    Client, Edit, UserState, MAX_BODY_BYTES, MAX_EDITS, MAX_SELECTION, MAX_TEXT, SCHEMA_VERSION,
};

const BAD: Refusal = Refusal::new(400, "schema");

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| s.len() <= MAX_TEXT)
        .map(str::to_owned)
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite())
}

fn edit(value: &Value) -> Option<Edit> {
    let at = number(value.get("at")?)?;
    if at < 0.0 {
        return None;
    }
    Some(Edit {
        path: text(value.get("path")?)?,
        property: text(value.get("property")?)?,
        value: text(value.get("value")?)?,
        at: at as u64,
    })
}

/// `body` as `user_id`'s state.
pub fn parse_state(body: &str, user_id: u64) -> Result<UserState, Refusal> {
    if body.len() > MAX_BODY_BYTES {
        return Err(Refusal::new(413, "size"));
    }
    let raw: Value = serde_json::from_str(body).map_err(|_| BAD)?;
    let raw = raw.as_object().ok_or(BAD)?;
    if raw.get("v").and_then(Value::as_u64) != Some(u64::from(SCHEMA_VERSION)) {
        return Err(BAD);
    }
    if raw.get("userId").and_then(Value::as_u64) != Some(user_id) {
        return Err(BAD);
    }
    let name = raw.get("name").and_then(text).ok_or(BAD)?;
    let color = raw
        .get("color")
        .and_then(Value::as_str)
        .filter(|c| {
            c.len() == 7
                && c.starts_with('#')
                && c[1..]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or(BAD)?
        .to_owned();
    let client = match raw.get("client").and_then(Value::as_str) {
        Some("studio") => Client::Studio,
        Some("rbx-native") => Client::RbxNative,
        _ => return Err(BAD),
    };

    let camera = match raw.get("camera") {
        None => None,
        Some(Value::Array(items)) if items.len() == 12 => {
            let mut cam = [0.0; 12];
            for (slot, item) in cam.iter_mut().zip(items) {
                *slot = number(item).ok_or(BAD)?;
            }
            Some(cam)
        }
        Some(_) => return Err(BAD),
    };
    let selection = match raw.get("selection") {
        None => Vec::new(),
        Some(Value::Array(items)) if items.len() <= MAX_SELECTION => items
            .iter()
            .map(text)
            .collect::<Option<Vec<_>>>()
            .ok_or(BAD)?,
        Some(_) => return Err(BAD),
    };
    let script = match raw.get("script") {
        None => None,
        Some(value) => Some(text(value).ok_or(BAD)?),
    };
    let edits = match raw.get("edits") {
        None => Vec::new(),
        Some(Value::Array(items)) if items.len() <= MAX_EDITS => items
            .iter()
            .map(edit)
            .collect::<Option<Vec<_>>>()
            .ok_or(BAD)?,
        Some(_) => return Err(BAD),
    };

    Ok(UserState {
        v: SCHEMA_VERSION,
        user_id,
        name,
        color,
        client,
        camera,
        selection,
        script,
        edits,
        seen: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = r##"{"v":1,"userId":5,"name":"Ana","color":"#e8590c","client":"studio","selection":["Workspace.A"],"edits":[{"path":"Workspace.A","property":"Size","value":"1, 2, 3","at":10}],"extra":true}"##;

    #[test]
    fn a_good_state_parses_and_unknown_keys_drop() {
        let state = parse_state(OK, 5).unwrap();
        assert_eq!(state.name, "Ana");
        assert_eq!(state.edits[0].at, 10);
        assert_eq!(state.seen, 0);
    }

    #[test]
    fn another_users_id_is_refused() {
        assert_eq!(parse_state(OK, 6), Err(BAD));
    }

    #[test]
    fn limits_and_shapes_are_enforced() {
        let bad = |body: String| parse_state(&body, 5).unwrap_err();
        assert_eq!(bad(OK.replace(r#""v":1"#, r#""v":2"#)), BAD);
        assert_eq!(bad(OK.replace("#e8590c", "#E8590C")), BAD);
        assert_eq!(bad(OK.replace("studio", "roblox")), BAD);
        assert_eq!(bad(OK.replace("Ana", &"x".repeat(MAX_TEXT + 1))), BAD);
        assert_eq!(
            bad(OK.replace(r#""extra":true"#, r#""camera":[1,2,3]"#)),
            BAD
        );
        assert_eq!(
            bad(format!("{}{}", OK, " ".repeat(MAX_BODY_BYTES))),
            Refusal::new(413, "size")
        );
        let many = format!(r#"[{}]"#, vec![r#""a""#; MAX_SELECTION + 1].join(","));
        assert_eq!(bad(OK.replace(r#"["Workspace.A"]"#, &many)), BAD);
    }
}
