use rbx_cloud::quick_login::{cookie_from_set_cookie, parse_create, parse_status, LoginStatus};

const CREATE_BODY: &str = r#"{"code":"8P8499","status":"Created","privateKey":"pk","expirationTime":"2026-10-13T23:35:37.89","imagePath":"/v1/login/qr-code-image?key=k&code=8P8499"}"#;

#[test]
fn parse_create_reads_fields() {
    let code = parse_create(CREATE_BODY).expect("valid create body");
    assert_eq!(code.code, "8P8499");
    assert_eq!(code.expires, "2026-10-13T23:35:37.89");
    assert_eq!(code.qr_path, "/v1/login/qr-code-image?key=k&code=8P8499");
}

#[test]
fn parse_create_debug_hides_private_key() {
    let code = parse_create(CREATE_BODY).expect("valid create body");
    let debug = format!("{code:?}");
    assert!(
        !debug.contains("pk"),
        "Debug leaked the private key: {debug}"
    );
}

#[test]
fn parse_create_rejects_empty_object() {
    assert!(parse_create("{}").is_err());
}

#[test]
fn parse_create_rejects_non_json() {
    assert!(parse_create("not json").is_err());
}

#[test]
fn parse_status_maps_live_states() {
    assert_eq!(
        parse_status(200, r#"{"status":"Created"}"#),
        Ok(LoginStatus::Created)
    );
    assert_eq!(
        parse_status(200, r#"{"status":"UserLinked"}"#),
        Ok(LoginStatus::UserLinked)
    );
    assert_eq!(
        parse_status(200, r#"{"status":"Validated"}"#),
        Ok(LoginStatus::Validated)
    );
    assert_eq!(
        parse_status(200, r#"{"status":"Cancelled"}"#),
        Ok(LoginStatus::Cancelled)
    );
}

#[test]
fn parse_status_400_is_expired() {
    assert_eq!(
        parse_status(400, r#""CodeInvalid""#),
        Ok(LoginStatus::Expired)
    );
    assert_eq!(parse_status(400, ""), Ok(LoginStatus::Expired));
}

#[test]
fn parse_status_other_codes_are_errors() {
    assert!(parse_status(500, "x").is_err());
}

#[test]
fn parse_status_unknown_state_is_error() {
    assert!(parse_status(200, r#"{"status":"Weird"}"#).is_err());
}

#[test]
fn cookie_found_among_set_cookie_headers() {
    let headers = vec![
        "RBXEventTrackerV2=x; path=/".to_string(),
        ".ROBLOSECURITY=_|WARNING:-abc; domain=.roblox.com; path=/; secure; HttpOnly".to_string(),
    ];
    let cookie = cookie_from_set_cookie(&headers).expect("cookie present");
    assert_eq!(cookie.expose_secret(), "_|WARNING:-abc");
}

#[test]
fn cookie_absent_is_none() {
    let headers = vec!["RBXEventTrackerV2=x; path=/".to_string()];
    assert!(cookie_from_set_cookie(&headers).is_none());
}

#[test]
fn cookie_empty_value_is_none() {
    let headers = vec![".ROBLOSECURITY=; domain=.roblox.com; path=/".to_string()];
    assert!(cookie_from_set_cookie(&headers).is_none());
}
