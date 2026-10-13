//! Parsing and update-mask tests over canned Open Cloud payloads.

use super::*;
use serde_json::json;

fn parse(text: &str) -> Result<ExperienceSettings, CloudError> {
    parse_experience(serde_json::from_str(text).unwrap())
}

const FULL: &str = r#"{
  "path": "universes/12",
  "displayName": "Obby",
  "description": "Jump around",
  "visibility": "PUBLIC",
  "ageRating": "AGE_RATING_9_PLUS",
  "voiceChatEnabled": true,
  "privateServerPriceRobux": 25,
  "desktopEnabled": true,
  "mobileEnabled": false,
  "tabletEnabled": true,
  "consoleEnabled": false,
  "vrEnabled": true,
  "rootPlace": "universes/12/places/34",
  "group": "groups/5"
}"#;

#[test]
fn parses_every_field() {
    let s = parse(FULL).unwrap();
    assert_eq!(s.display_name, "Obby");
    assert_eq!(s.description, "Jump around");
    assert_eq!(s.visibility, Visibility::Public);
    assert_eq!(s.age_rating.as_deref(), Some("9_PLUS"));
    assert!(s.voice_chat_enabled);
    assert_eq!(s.private_server_price_robux, Some(25));
    assert!(s.desktop_enabled && !s.mobile_enabled && s.tablet_enabled);
    assert!(!s.console_enabled && s.vr_enabled);
    assert_eq!(s.root_place_id, 34);
}

#[test]
fn private_servers_absent_and_user_owner() {
    let s = parse(
        r#"{"displayName":"A","visibility":"PRIVATE","user":"users/9",
            "rootPlace":"universes/1/places/2"}"#,
    )
    .unwrap();
    assert_eq!(s.private_server_price_robux, None);
    assert_eq!(s.visibility, Visibility::Private);
    assert_eq!(s.description, "");
    assert!(!s.voice_chat_enabled);
}

#[test]
fn null_private_server_price_is_off() {
    let s =
        parse(r#"{"privateServerPriceRobux":null,"rootPlace":"universes/1/places/2"}"#).unwrap();
    assert_eq!(s.private_server_price_robux, None);
}

#[test]
fn age_rating_mapping() {
    for (raw, want) in [
        ("AGE_RATING_ALL", Some("ALL")),
        ("AGE_RATING_13_PLUS", Some("13_PLUS")),
        ("AGE_RATING_17_PLUS", Some("17_PLUS")),
        ("AGE_RATING_UNSPECIFIED", None),
    ] {
        let v = json!({"ageRating": raw, "rootPlace": "universes/1/places/2"});
        assert_eq!(parse_experience(v).unwrap().age_rating.as_deref(), want);
    }
    let missing = parse(r#"{"rootPlace":"universes/1/places/2"}"#).unwrap();
    assert_eq!(missing.age_rating, None);
}

#[test]
fn unparseable_root_place_is_unexpected_shape() {
    let err = parse(r#"{"rootPlace":"nonsense"}"#).unwrap_err();
    assert!(matches!(err, CloudError::UnexpectedShape(_)));
}

#[test]
fn parses_place() {
    let p = parse_place(json!({
        "path": "universes/1/places/2",
        "displayName": "Lobby",
        "description": "Start here",
        "serverSize": 50,
        "root": true
    }))
    .unwrap();
    assert_eq!(p.display_name, "Lobby");
    assert_eq!(p.description, "Start here");
    assert_eq!(p.server_size, 50);
    assert!(p.root);
}

#[test]
fn place_defaults_missing_fields() {
    let p = parse_place(json!({"path": "universes/1/places/2"})).unwrap();
    assert_eq!(p.display_name, "");
    assert_eq!(p.server_size, 0);
    assert!(!p.root);
}

#[test]
fn single_field_request() {
    let u = ExperienceUpdate {
        voice_chat_enabled: Some(true),
        ..Default::default()
    };
    let (mask, body) = u.request();
    assert_eq!(mask, "voiceChatEnabled");
    assert_eq!(Value::Object(body), json!({"voiceChatEnabled": true}));
}

#[test]
fn several_fields_follow_struct_order() {
    let u = ExperienceUpdate {
        vr_enabled: Some(false),
        desktop_enabled: Some(true),
        private_server_price_robux: Some(Some(10)),
        ..Default::default()
    };
    let (mask, body) = u.request();
    assert_eq!(mask, "privateServerPriceRobux,desktopEnabled,vrEnabled");
    assert_eq!(
        Value::Object(body),
        json!({"privateServerPriceRobux": 10, "desktopEnabled": true, "vrEnabled": false})
    );
}

#[test]
fn clearing_private_servers_sends_null() {
    let u = ExperienceUpdate {
        private_server_price_robux: Some(None),
        ..Default::default()
    };
    let (mask, body) = u.request();
    assert_eq!(mask, "privateServerPriceRobux");
    assert_eq!(
        Value::Object(body),
        json!({"privateServerPriceRobux": null})
    );
}

#[test]
fn place_request() {
    let u = PlaceUpdate {
        display_name: Some("New".into()),
        server_size: Some(30),
        ..Default::default()
    };
    let (mask, body) = u.request();
    assert_eq!(mask, "displayName,serverSize");
    assert_eq!(
        Value::Object(body),
        json!({"displayName": "New", "serverSize": 30})
    );
}

#[test]
fn is_empty() {
    assert!(ExperienceUpdate::default().is_empty());
    assert!(PlaceUpdate::default().is_empty());
    assert!(!ExperienceUpdate {
        private_server_price_robux: Some(None),
        ..Default::default()
    }
    .is_empty());
    assert!(!PlaceUpdate {
        description: Some(String::new()),
        ..Default::default()
    }
    .is_empty());
}
