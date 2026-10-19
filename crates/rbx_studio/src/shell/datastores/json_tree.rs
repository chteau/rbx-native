//! Flattening a JSON value into rows for the value tree, plus the small
//! parsers and formatters the editors share.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

const PREVIEW_CHARS: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Object,
    Array,
    String,
    Number,
    Bool,
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TreeRow {
    pub(super) path: String,
    pub(super) depth: usize,
    pub(super) label: String,
    pub(super) preview: String,
    pub(super) kind: Kind,
    pub(super) expandable: bool,
    pub(super) collapsed: bool,
}

/// Roblox stores NaN and infinities as `{"m":null,"t":"numeric","v":"inf"}`
/// because JSON has no spelling for them.
fn non_finite(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    if object.get("t")?.as_str()? != "numeric" {
        return None;
    }
    let v = object.get("v")?.as_str()?;
    matches!(v, "inf" | "-inf" | "nan").then_some(v)
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        let mut out: String = text.chars().take(max).collect();
        out.push('…');
        out
    } else {
        text.to_string()
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

fn escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn describe(value: &Value) -> (Kind, String, bool) {
    if let Some(v) = non_finite(value) {
        return (Kind::Number, v.to_string(), false);
    }
    match value {
        Value::Object(map) => (
            Kind::Object,
            format!("{{{}}}", plural(map.len(), "key", "keys")),
            !map.is_empty(),
        ),
        Value::Array(items) => (
            Kind::Array,
            format!("[{}]", plural(items.len(), "item", "items")),
            !items.is_empty(),
        ),
        Value::String(s) => (
            Kind::String,
            serde_json::to_string(&cut(s, PREVIEW_CHARS)).unwrap_or_default(),
            false,
        ),
        Value::Number(n) => (Kind::Number, n.to_string(), false),
        Value::Bool(b) => (Kind::Bool, b.to_string(), false),
        Value::Null => (Kind::Null, "null".to_string(), false),
    }
}

fn push_row(
    out: &mut Vec<TreeRow>,
    value: &Value,
    path: String,
    label: String,
    depth: usize,
    collapsed: &BTreeSet<String>,
) {
    let (kind, preview, expandable) = describe(value);
    let folded = expandable && collapsed.contains(&path);
    out.push(TreeRow {
        path: path.clone(),
        depth,
        label,
        preview,
        kind,
        expandable,
        collapsed: folded,
    });
    if !expandable || folded {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let child_path = format!("{path}/{}", escape(key));
                push_row(out, child, child_path, key.clone(), depth + 1, collapsed);
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                push_row(
                    out,
                    child,
                    format!("{path}/{i}"),
                    format!("[{i}]"),
                    depth + 1,
                    collapsed,
                );
            }
        }
        _ => {}
    }
}

pub(super) fn rows(value: &Value, collapsed: &BTreeSet<String>) -> Vec<TreeRow> {
    let mut out = Vec::new();
    if non_finite(value).is_some() {
        push_row(
            &mut out,
            value,
            String::new(),
            "value".to_string(),
            0,
            collapsed,
        );
        return out;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                push_row(
                    &mut out,
                    child,
                    format!("/{}", escape(key)),
                    key.clone(),
                    0,
                    collapsed,
                );
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                push_row(
                    &mut out,
                    child,
                    format!("/{i}"),
                    format!("[{i}]"),
                    0,
                    collapsed,
                );
            }
        }
        _ => push_row(
            &mut out,
            value,
            String::new(),
            "value".to_string(),
            0,
            collapsed,
        ),
    }
    out
}

pub(super) fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

pub(super) fn size_label(bytes: usize) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64 / 1024.0;
    let mut unit = "KB";
    if size >= 1024.0 {
        size /= 1024.0;
        unit = "MB";
    }
    format!("{size:.1} {unit}")
}

pub(super) fn parse_json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|err| {
        let message = err.to_string();
        let bare = message
            .rfind(" at line ")
            .map_or(message.as_str(), |at| &message[..at]);
        format!("Line {}, column {}: {bare}", err.line(), err.column())
    })
}

pub(super) fn parse_users(text: &str) -> Result<Vec<u64>, String> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| match part.parse::<u64>() {
            Ok(id) if id > 0 => Ok(id),
            _ => Err(format!("“{part}” isn’t a UserId (a whole number).")),
        })
        .collect()
}

pub(super) fn parse_attributes(text: &str) -> Result<Map<String, Value>, String> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match parse_json(text)? {
        Value::Object(map) => Ok(map),
        _ => Err("Attributes must be a JSON object, like {\"schema\": 3}.".to_string()),
    }
}

pub(super) fn short(value: &Value, max: usize) -> String {
    cut(&value.to_string(), max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn none() -> BTreeSet<String> {
        BTreeSet::new()
    }

    #[test]
    fn flattens_object_with_paths_and_depths() {
        let r = rows(&json!({"a": {"b": 1}, "c": [true]}), &none());
        let paths: Vec<_> = r.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, ["/a", "/a/b", "/c", "/c/0"]);
        assert_eq!(r[1].depth, 1);
        assert_eq!(r[3].label, "[0]");
    }

    #[test]
    fn scalar_root_is_one_row() {
        let r = rows(&json!(5), &none());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].label, "value");
        assert_eq!(r[0].preview, "5");
    }

    #[test]
    fn container_previews_use_singular_forms() {
        let r = rows(&json!({"a": {"x": 1}, "b": [1, 2], "c": [1]}), &none());
        assert_eq!(r[0].preview, "{1 key}");
        let b = r.iter().find(|r| r.path == "/b").unwrap();
        assert_eq!(b.preview, "[2 items]");
        let c = r.iter().find(|r| r.path == "/c").unwrap();
        assert_eq!(c.preview, "[1 item]");
    }

    #[test]
    fn pointer_escapes_tilde_and_slash() {
        let r = rows(&json!({"a/b": 1, "c~d": 2}), &none());
        assert_eq!(r[0].path, "/a~1b");
        assert_eq!(r[1].path, "/c~0d");
    }

    #[test]
    fn collapsed_path_hides_children() {
        let folded: BTreeSet<String> = ["/a".to_string()].into();
        let r = rows(&json!({"a": {"b": 1}}), &folded);
        assert_eq!(r.len(), 1);
        assert!(r[0].collapsed && r[0].expandable);
    }

    #[test]
    fn long_strings_are_cut() {
        let r = rows(&json!({"s": "x".repeat(300)}), &none());
        assert_eq!(r[0].kind, Kind::String);
        assert_eq!(r[0].preview.chars().count(), PREVIEW_CHARS + 3);
        assert!(r[0].preview.ends_with("…\""));
    }

    #[test]
    fn non_finite_tag_is_a_number() {
        let r = rows(
            &json!({"x": {"m": null, "t": "numeric", "v": "-inf"}}),
            &none(),
        );
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, Kind::Number);
        assert_eq!(r[0].preview, "-inf");
        assert!(!r[0].expandable);
    }

    #[test]
    fn size_labels() {
        assert_eq!(size_label(512), "512 B");
        assert_eq!(size_label(1434), "1.4 KB");
        assert_eq!(size_label(2_202_009), "2.1 MB");
    }

    #[test]
    fn parse_json_reports_position_once() {
        let err = parse_json("{\n  \"a\": }").unwrap_err();
        assert!(err.starts_with("Line 2, column "), "{err}");
        assert!(!err.contains(" at line "), "{err}");
    }

    #[test]
    fn parse_json_accepts_valid() {
        assert_eq!(parse_json("[1]").unwrap(), json!([1]));
    }

    #[test]
    fn users_split_on_commas_and_spaces() {
        assert_eq!(parse_users("1, 2 3,4").unwrap(), vec![1, 2, 3, 4]);
        assert!(parse_users("  ").unwrap().is_empty());
    }

    #[test]
    fn users_reject_garbage_and_zero() {
        assert_eq!(
            parse_users("1, abc").unwrap_err(),
            "“abc” isn’t a UserId (a whole number)."
        );
        assert!(parse_users("0").is_err());
    }

    #[test]
    fn attributes_blank_object_and_non_object() {
        assert!(parse_attributes(" ").unwrap().is_empty());
        assert_eq!(
            parse_attributes("{\"schema\": 3}").unwrap()["schema"],
            json!(3)
        );
        assert_eq!(
            parse_attributes("[1]").unwrap_err(),
            "Attributes must be a JSON object, like {\"schema\": 3}."
        );
    }

    #[test]
    fn short_cuts_compact_json() {
        assert_eq!(short(&json!({"a": 1}), 50), "{\"a\":1}");
        assert_eq!(short(&json!("abcdefgh"), 4), "\"abc…");
    }
}
