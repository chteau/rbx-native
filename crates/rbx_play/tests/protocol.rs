use rbx_play::protocol::{Command, CommandBody, DeviceReport, Metrics, Side, Uplink};
use serde_json::json;

fn set_tags() -> CommandBody {
    CommandBody::SetTags {
        side: Side::Client,
        target: 7,
        tags: vec!["A".into()],
    }
}

#[test]
fn set_tags_body_serializes_to_wire_shape() {
    let value = serde_json::to_value(set_tags()).expect("serialize SetTags");
    assert_eq!(
        value,
        json!({
            "type": "set_tags",
            "side": "client",
            "target": 7,
            "tags": ["A"],
        })
    );
}

#[test]
fn stats_uplink_with_partial_metrics_fills_the_rest_with_none() {
    let uplink: Uplink = serde_json::from_str(
        r#"{"type":"stats","side":"server","metrics":{"fps":60,"players":1}}"#,
    )
    .expect("deserialize stats");

    let Uplink::Stats { side, metrics } = uplink else {
        panic!("expected Uplink::Stats");
    };
    assert_eq!(side, Side::Server);
    assert_eq!(metrics.fps, Some(60.0));
    assert_eq!(metrics.players, Some(1));
    assert_eq!(
        metrics,
        Metrics {
            fps: Some(60.0),
            players: Some(1),
            ..Metrics::default()
        }
    );
}

#[test]
fn stats_uplink_with_empty_metrics_is_all_none() {
    let uplink: Uplink = serde_json::from_str(r#"{"type":"stats","side":"client","metrics":{}}"#)
        .expect("deserialize stats");

    assert_eq!(
        uplink,
        Uplink::Stats {
            side: Side::Client,
            metrics: Metrics::default(),
        }
    );
}

#[test]
fn device_uplink_with_full_report_deserializes() {
    let uplink: Uplink = serde_json::from_str(
        r#"{"type":"device","viewport_width":852,"viewport_height":393,"touch_enabled":true,"keyboard_enabled":false,"gamepad_enabled":false,"ten_foot":false,"preferred_input":"Touch","on_screen_keyboard_visible":false,"text_focus":true,"last_input":"Touch"}"#,
    )
    .expect("deserialize device");

    assert_eq!(
        uplink,
        Uplink::Device(DeviceReport {
            viewport_width: 852.0,
            viewport_height: 393.0,
            touch_enabled: true,
            keyboard_enabled: false,
            gamepad_enabled: false,
            ten_foot: false,
            preferred_input: Some("Touch".into()),
            on_screen_keyboard_visible: false,
            text_focus: true,
            last_input: Some("Touch".into()),
        })
    );
}

#[test]
fn device_uplink_with_only_type_fills_report_with_defaults() {
    let uplink: Uplink =
        serde_json::from_str(r#"{"type":"device"}"#).expect("deserialize bare device");

    assert_eq!(uplink, Uplink::Device(DeviceReport::default()));
}

#[test]
fn device_report_round_trips_flat_with_type_tag() {
    let report = DeviceReport {
        viewport_width: 844.0,
        viewport_height: 390.0,
        touch_enabled: true,
        keyboard_enabled: true,
        gamepad_enabled: true,
        ten_foot: true,
        preferred_input: Some("Gamepad".into()),
        on_screen_keyboard_visible: true,
        text_focus: true,
        last_input: Some("Gamepad1".into()),
    };
    let value = serde_json::to_value(Uplink::Device(report)).expect("serialize device");
    assert_eq!(value["type"], "device");
    assert_eq!(value["viewport_width"], 844.0);
    assert!(value.get("device").is_none());

    let parsed: Uplink = serde_json::from_value(value).expect("deserialize device");
    assert_eq!(
        parsed,
        Uplink::Device(DeviceReport {
            viewport_width: 844.0,
            viewport_height: 390.0,
            touch_enabled: true,
            keyboard_enabled: true,
            gamepad_enabled: true,
            ten_foot: true,
            preferred_input: Some("Gamepad".into()),
            on_screen_keyboard_visible: true,
            text_focus: true,
            last_input: Some("Gamepad1".into()),
        })
    );
}

#[test]
fn command_with_set_tags_round_trips() {
    let original = Command {
        id: 3,
        body: set_tags(),
    };
    let text = serde_json::to_string(&original).expect("serialize Command");
    let parsed: Command = serde_json::from_str(&text).expect("deserialize Command");
    assert_eq!(parsed, original);
}
