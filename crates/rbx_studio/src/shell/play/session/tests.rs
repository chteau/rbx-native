use super::*;

fn config() -> ProbeConfig {
    ProbeConfig {
        base_url: "https://example.test".to_owned(),
        token: "t".to_owned(),
        session: "s".to_owned(),
    }
}

fn has_probe(dom: &WeakDom) -> bool {
    let mut stack: Vec<_> = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        let Some(inst) = dom.get(r) else {
            continue;
        };
        if inst.name() == rbx_play::inject::PROBE_NAME && inst.class() == "Script" {
            return true;
        }
        stack.extend(inst.children().iter().copied());
    }
    false
}

fn log(side: Side, level: LogLevel) -> Uplink {
    Uplink::Log {
        side,
        level,
        message: "m".to_owned(),
        time: 0.0,
    }
}

fn hello(http: bool, loadstring: bool) -> Uplink {
    Uplink::Hello {
        session: "s".to_owned(),
        place_version: 3,
        http_enabled: http,
        loadstring_enabled: loadstring,
        is_studio: false,
        job_id: String::new(),
    }
}

#[test]
fn prepare_injects_into_a_copy() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/tests/TestPlace.rbxl"
    );
    let dom = rbx_binary::deserialize(&std::fs::read(path).unwrap()).unwrap();
    assert!(!has_probe(&dom));
    let prepared = prepare(&dom, &config()).unwrap();
    let back = rbx_binary::deserialize(prepared.bytes.as_slice()).unwrap();
    assert!(has_probe(&back));
    assert!(!has_probe(&dom));
}

#[test]
fn log_rows_follow_side_and_level() {
    let (source, row) = feedback_for(&log(Side::Server, LogLevel::Error)).unwrap();
    assert_eq!(source, SERVER_SOURCE);
    assert!(matches!(row, Feedback::Error(_)));
    let (source, row) = feedback_for(&log(Side::Client, LogLevel::Warning)).unwrap();
    assert_eq!(source, CLIENT_SOURCE);
    assert!(matches!(row, Feedback::Warning(_)));
    let (_, row) = feedback_for(&log(Side::Client, LogLevel::Info)).unwrap();
    assert!(matches!(row, Feedback::Output(_)));
    let (_, row) = feedback_for(&log(Side::Server, LogLevel::Output)).unwrap();
    assert!(matches!(row, Feedback::Output(_)));
}

#[test]
fn hello_reports_missing_flags() {
    let (_, row) = feedback_for(&hello(false, true)).unwrap();
    assert!(matches!(&row, Feedback::Error(text) if text.contains("HTTP")));
    let (_, row) = feedback_for(&hello(true, true)).unwrap();
    assert!(matches!(row, Feedback::Output(_)));
}

#[test]
fn acks_show_only_failures() {
    let ok = Uplink::Ack {
        id: 1,
        ok: true,
        result: "x".to_owned(),
    };
    assert!(feedback_for(&ok).is_none());
    let bad = Uplink::Ack {
        id: 1,
        ok: false,
        result: "boom".to_owned(),
    };
    let (_, row) = feedback_for(&bad).unwrap();
    assert!(matches!(row, Feedback::Error(_)));
}

#[test]
fn stop_follows_the_developers_player() {
    let mut own = None;
    assert_eq!(stop_for(&Uplink::Joined { user_id: 5 }, &mut own), None);
    assert_eq!(stop_for(&Uplink::Left { user_id: 7 }, &mut own), None);
    assert_eq!(
        stop_for(&Uplink::Left { user_id: 5 }, &mut own),
        Some(StopReason::PlayerLeft)
    );
    assert_eq!(
        stop_for(&Uplink::Bye, &mut None),
        Some(StopReason::ServerClosed)
    );
    assert_eq!(
        stop_for(&Uplink::Left { user_id: 9 }, &mut None),
        Some(StopReason::PlayerLeft)
    );
}

#[test]
fn every_stop_reason_has_a_message() {
    for reason in [
        StopReason::Stopped,
        StopReason::ClientExited,
        StopReason::WindowClosed,
        StopReason::PlayerLeft,
        StopReason::ServerClosed,
        StopReason::Failed("x".to_owned()),
    ] {
        assert!(!reason.message().is_empty());
    }
}
