use rbx_play::protocol::{Command, CommandBody, Metrics, Side, Uplink};
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
fn command_with_set_tags_round_trips() {
    let original = Command {
        id: 3,
        body: set_tags(),
    };
    let text = serde_json::to_string(&original).expect("serialize Command");
    let parsed: Command = serde_json::from_str(&text).expect("deserialize Command");
    assert_eq!(parsed, original);
}
