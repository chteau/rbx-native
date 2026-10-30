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
        parts: Vec::new(),
        origin: None,
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
    let prepared = prepare(&dom, &config(), &[], 10.0).unwrap();
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
    assert!(matches!(row, Feedback::Info(_)));
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

fn left(user_id: u64, teleport: bool) -> Uplink {
    Uplink::Left { user_id, teleport }
}

fn bye(job_id: &str) -> Uplink {
    Uplink::Bye {
        job_id: job_id.to_owned(),
    }
}

fn hello_from(job_id: &str) -> Uplink {
    Uplink::Hello {
        session: "s".to_owned(),
        place_version: 1,
        http_enabled: true,
        loadstring_enabled: true,
        is_studio: false,
        job_id: job_id.to_owned(),
    }
}

#[test]
fn stop_follows_the_developers_player() {
    let mut p = Presence::default();
    assert_eq!(follow(&Uplink::Joined { user_id: 5 }, &mut p), None);
    assert_eq!(follow(&left(7, false), &mut p), None);
    assert_eq!(
        follow(&left(5, false), &mut p),
        Some(Follow::Stop(StopReason::PlayerLeft))
    );
    assert_eq!(
        follow(&bye(""), &mut Presence::default()),
        Some(Follow::Stop(StopReason::ServerClosed))
    );
    assert_eq!(
        follow(&left(9, false), &mut Presence::default()),
        Some(Follow::Stop(StopReason::PlayerLeft))
    );
}

#[test]
fn a_teleport_is_followed_into_the_next_server() {
    let mut p = Presence::default();
    follow(&hello_from("old"), &mut p);
    follow(&Uplink::Joined { user_id: 5 }, &mut p);
    assert_eq!(follow(&left(5, true), &mut p), Some(Follow::Leaving));
    // The emptied server closes while the player is between servers…
    assert_eq!(follow(&bye("old"), &mut p), None);
    assert_eq!(follow(&hello_from("new"), &mut p), None);
    assert_eq!(
        follow(&Uplink::Joined { user_id: 5 }, &mut p),
        Some(Follow::Arrived)
    );
    // …or after it arrived.
    assert_eq!(follow(&bye("old"), &mut p), None);
    assert_eq!(
        follow(&bye("new"), &mut p),
        Some(Follow::Stop(StopReason::ServerClosed))
    );
}

#[test]
fn a_reserved_server_seen_first_still_counts_as_arrival() {
    let mut p = Presence::default();
    follow(&hello_from("old"), &mut p);
    follow(&Uplink::Joined { user_id: 5 }, &mut p);
    follow(&hello_from("reserved"), &mut p);
    assert_eq!(follow(&Uplink::Joined { user_id: 5 }, &mut p), None);
    // The old server's leave lands after the join, flagged or not.
    assert_eq!(follow(&left(5, false), &mut p), Some(Follow::Arrived));
    assert_eq!(follow(&bye("old"), &mut p), None);
    assert_eq!(
        follow(&left(5, false), &mut p),
        Some(Follow::Stop(StopReason::PlayerLeft))
    );
}

#[test]
fn leaving_after_a_teleport_still_ends_the_play() {
    let mut p = Presence::default();
    follow(&Uplink::Joined { user_id: 5 }, &mut p);
    follow(&left(5, true), &mut p);
    follow(&Uplink::Joined { user_id: 5 }, &mut p);
    assert_eq!(
        follow(&left(5, false), &mut p),
        Some(Follow::Stop(StopReason::PlayerLeft))
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

#[test]
fn stats_line_leads_with_rate_and_skips_unknowns() {
    use rbx_play::protocol::Metrics;
    let client = Metrics {
        fps: Some(59.6),
        ping_ms: Some(44.8),
        memory_mb: Some(811.7),
        instances: Some(9000),
        ..Metrics::default()
    };
    assert_eq!(
        stats_line(Side::Client, &client),
        "60 fps \u{b7} 45 ms ping \u{b7} 812 MB"
    );
    let server = Metrics {
        fps: Some(60.0),
        players: Some(1),
        ..Metrics::default()
    };
    assert_eq!(stats_line(Side::Server, &server), "60 Hz \u{b7} 1 player");
    assert_eq!(stats_line(Side::Server, &Metrics::default()), "\u{2014}");
    let details = stats_details(&client);
    assert!(details.contains("Ping: 44.8 ms"));
    assert!(details.contains("Instances: 9000"));
    assert!(!details.contains("Players"));
}

#[test]
fn scripts_with_breakpoints_publish_with_hooks() {
    let mut dom = WeakDom::new();
    let sss = dom.new_instance("ServerScriptService", "ServerScriptService", None);
    let main = dom.new_instance("Script", "Main", Some(sss));
    let broken = dom.new_instance("Script", "Broken", Some(sss));
    assert!(crate::script_editor::source::write(
        &mut dom,
        main,
        "local a = 1\nprint(a)\n"
    ));
    assert!(crate::script_editor::source::write(
        &mut dom,
        broken,
        "local = \n"
    ));
    let hook = |line| Hook {
        line,
        server: true,
        ..Hook::default()
    };
    let requests = [
        DebugRequest {
            script: main,
            hooks: vec![hook(2)],
        },
        DebugRequest {
            script: broken,
            hooks: vec![hook(1)],
        },
    ];
    let prepared = prepare(&dom, &config(), &requests, 10.0).unwrap();
    assert_eq!(prepared.debug.len(), 1);
    assert_eq!(prepared.debug[0].script, main);
    assert_eq!(prepared.debug[0].lines, vec![1, 2]);
    assert!(prepared
        .warnings
        .iter()
        .any(|w| w.starts_with("Broken runs without breakpoints")));
    // The edited place keeps its own source.
    assert_eq!(
        crate::script_editor::source::read(&dom, main).as_deref(),
        Some("local a = 1\nprint(a)\n")
    );
}

fn source_named(dom: &WeakDom, name: &str) -> Option<String> {
    let mut stack: Vec<_> = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        let inst = dom.get(r)?;
        if inst.name() == name {
            return crate::script_editor::source::read(dom, r);
        }
        stack.extend(inst.children().iter().copied());
    }
    None
}

#[test]
fn marketplace_scripts_are_rewritten_in_the_copy_only() {
    let mut dom = WeakDom::new();
    let sss = dom.new_instance("ServerScriptService", "ServerScriptService", None);
    let shop = dom.new_instance("Script", "Shop", Some(sss));
    let plain = dom.new_instance("Script", "Plain", Some(sss));
    let shop_src = "local m = game:GetService(\"MarketplaceService\")\n";
    assert!(crate::script_editor::source::write(
        &mut dom, shop, shop_src
    ));
    assert!(crate::script_editor::source::write(
        &mut dom,
        plain,
        "local x = 1\n"
    ));
    let prepared = prepare(&dom, &config(), &[], 10.0).unwrap();
    let back = rbx_binary::deserialize(prepared.bytes.as_slice()).unwrap();
    assert!(source_named(&back, "Shop")
        .unwrap()
        .contains("__rbxnative_market"));
    assert_eq!(
        source_named(&back, "Plain").as_deref(),
        Some("local x = 1\n")
    );
    assert_eq!(source_named(&dom, "Shop").as_deref(), Some(shop_src));
}

#[test]
fn printing_scripts_shadow_print_on_their_first_line_in_the_copy_only() {
    let mut dom = WeakDom::new();
    let sss = dom.new_instance("ServerScriptService", "ServerScriptService", None);
    let main = dom.new_instance("Script", "Main", Some(sss));
    let src = "--!strict\nprint({1})\n";
    assert!(crate::script_editor::source::write(&mut dom, main, src));
    let prepared = prepare(&dom, &config(), &[], 10.0).unwrap();
    let back = rbx_binary::deserialize(prepared.bytes.as_slice()).unwrap();
    let published = source_named(&back, "Main").unwrap();
    assert_eq!(
        published,
        format!("--!strict\n{}print({{1}})\n", rbx_play::print::PREFIX)
    );
    assert_eq!(source_named(&dom, "Main").as_deref(), Some(src));
    assert!(source_named(&back, rbx_play::print::PRINT_NAME).is_some());
}

#[test]
fn input_scripts_go_through_the_touch_stand_in_except_a_forked_player_module() {
    let mut dom = WeakDom::new();
    let starter = dom.new_instance("StarterPlayer", "StarterPlayer", None);
    let scripts = dom.new_instance(
        "StarterPlayerScripts",
        "StarterPlayerScripts",
        Some(starter),
    );
    let controls = dom.new_instance("LocalScript", "Controls", Some(scripts));
    let module = dom.new_instance("ModuleScript", "PlayerModule", Some(scripts));
    let keyboard = dom.new_instance("ModuleScript", "Keyboard", Some(module));
    let src = "local UIS = game:GetService(\"UserInputService\")\n";
    for script in [controls, keyboard] {
        assert!(crate::script_editor::source::write(&mut dom, script, src));
    }
    let prepared = prepare(&dom, &config(), &[], 10.0).unwrap();
    let back = rbx_binary::deserialize(prepared.bytes.as_slice()).unwrap();
    assert_eq!(
        source_named(&back, "Controls").unwrap(),
        format!(
            "local UIS = {}game:GetService(\"UserInputService\"))\n",
            rbx_play::touch::WRAP_OPEN
        )
    );
    assert_eq!(source_named(&back, "Keyboard").as_deref(), Some(src));
    assert_eq!(source_named(&dom, "Controls").as_deref(), Some(src));
    assert!(source_named(&back, rbx_play::touch::TOUCH_INPUT_NAME).is_some());
}
