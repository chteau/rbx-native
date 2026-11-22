//! Instance streaming: the streaming probe (`conformance/streaming/`) run
//! in a Session, against the goldens recorded on a real server by the
//! `record_streaming` example. One test per probe place.

use std::collections::{BTreeMap, BTreeSet};

use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_sim::conformance::{self, streaming};
use rbx_sim::link::{Network, Way};
use rbx_sim::{Peer, Session};
use serde_json::Value;

mod common;

/// Parts a stud or two from a cube edge at the target radius: whether they
/// are in depends on where on the spawn the character lands (random, on
/// Roblox as here).
const EDGE: &[&str] = &["P1088", "D1536", "P288", "P320"];

type Sections = BTreeMap<String, Value>;

fn golden(variant: &str) -> Sections {
    let text = std::fs::read_to_string(streaming::golden_path(variant)).expect("golden");
    let file: Value = serde_json::from_str(&text).expect("json");
    serde_json::from_value(file["sections"].clone()).expect("sections")
}

fn names(sections: &Sections, snapshot: &str) -> BTreeSet<String> {
    let list = sections[snapshot]["names"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    list.iter()
        .filter_map(|n| n.as_str())
        .filter(|n| !EDGE.contains(n))
        .map(str::to_owned)
        .collect()
}

/// The log's entries of kind `kind` (`+`, `-`, `paused`, ...) as (time,
/// rest).
fn events(sections: &Sections, kind: &str) -> Vec<(f64, Value)> {
    sections["log"]
        .as_array()
        .map(|log| {
            log.iter()
                .filter(|e| e[1] == kind)
                .map(|e| (e[0].as_f64().unwrap_or(0.0), e[2].clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn check(variant: &str) {
    let real = golden(variant);
    let ours = streaming::run_in_sim(variant, 400.0).expect("probe");
    let mut failures = Vec::new();
    let mut expect = |what: &str, ok: bool, detail: String| {
        if !ok {
            failures.push(format!("{what}: {detail}"));
        }
    };

    // What the client has at game.Loaded: Workspace minus every part.
    let loaded = |s: &Sections| s["first"]["loaded"].clone();
    expect(
        "loaded",
        loaded(&ours) == loaded(&real),
        format!("{} vs {}", loaded(&ours), loaded(&real)),
    );

    // What it has at each point of the probe.
    for snapshot in [
        "settled",
        "afterRequest",
        "afterRequest5",
        "afterTimeoutRequest",
        "atTarget",
        "back",
        "focused",
    ] {
        // What arrived or left on Roblox within 0.1 s of its snapshot was
        // on the edge of the network's timing.
        let at = real[snapshot]["at"].as_f64().unwrap_or(0.0);
        let flux: BTreeSet<String> = ["+", "-"]
            .iter()
            .flat_map(|kind| events(&real, kind))
            .filter(|(t, _)| (t - at).abs() < 0.1)
            .filter_map(|(_, name)| name.as_str().map(str::to_owned))
            .collect();
        let (mut o, mut r) = (names(&ours, snapshot), names(&real, snapshot));
        o.retain(|n| !flux.contains(n));
        r.retain(|n| !flux.contains(n));
        expect(
            snapshot,
            o == r,
            format!(
                "ours has {:?} more, {:?} fewer",
                o.difference(&r).collect::<Vec<_>>(),
                r.difference(&o).collect::<Vec<_>>()
            ),
        );
        for key in ["farRef", "nearRef", "primary"] {
            let (o, r) = (&ours[snapshot][key], &real[snapshot][key]);
            expect(&format!("{snapshot}.{key}"), o == r, format!("{o} vs {r}"));
        }
    }

    // Held references: the same object, back with the server's values.
    expect(
        "identity",
        ours["identity"] == real["identity"],
        format!("{} vs {}", ours["identity"], real["identity"]),
    );
    // The server keeps everything.
    expect(
        "server",
        ours["server"] == real["server"],
        format!("{}", ours["server"]),
    );

    // WaitForChild on a part that never streams in: nil after the timeout,
    // and the infinite-yield warning 5 s after the call.
    let wait = |s: &Sections| s["result"]["waitTimeout"].clone();
    expect(
        "waitTimeout",
        wait(&ours)["got"] == false
            && (wait(&ours)["took"].as_f64().unwrap_or(0.0) - 3.0).abs() < 0.1,
        format!("{}", wait(&ours)),
    );
    let warning = |s: &Sections| {
        let w = &s["warnings"][0];
        (
            w[1].clone(),
            w[0].as_f64().unwrap_or(0.0) - s["result"]["scriptStart"].as_f64().unwrap_or(0.0),
        )
    };
    let ((om, ot), (rm, _)) = (warning(&ours), warning(&real));
    expect(
        "warning",
        om == rm && (ot - 5.0).abs() < 0.2,
        format!("{om} after {ot:.2} s vs {rm}"),
    );

    // PersistentLoaded once, right after the persistent models.
    let loaded_at = |s: &Sections| events(s, "persistentLoaded").first().map(|e| e.0);
    let persist_at = |s: &Sections| {
        events(s, "+")
            .iter()
            .find(|e| e.1 == "Persist_1")
            .map(|e| e.0)
    };
    let (o, p) = (loaded_at(&ours), persist_at(&ours));
    expect(
        "persistentLoaded",
        o.zip(p).is_some_and(|(l, p)| l >= p && l - p < 0.05),
        format!("{o:?} after Persist {p:?}"),
    );
    expect(
        "persistentLoaded once",
        events(&ours, "persistentLoaded").len() == 1,
        String::new(),
    );

    // Times on the client's own clock (on Roblox its DistributedGameTime
    // runs a second or two behind the server's), from when the teleport's
    // target area arrived.
    let arrived = |s: &Sections| {
        events(s, "+")
            .iter()
            .find(|e| e.1 == "T0")
            .map_or(0.0, |e| e.0)
    };
    // Streamed-out parts go with it (or 5 s on for the minimum radius
    // the focus left).
    let out_after = |s: &Sections, name: &str| {
        let anchor = arrived(s);
        events(s, "-")
            .iter()
            .find(|e| e.1 == name && e.0 > anchor - 1.0)
            .map(|e| e.0 - anchor)
    };
    for name in ["P256", "P16"] {
        let (o, r) = (out_after(&ours, name), out_after(&real, name));
        let close = match (o, r) {
            (Some(o), Some(r)) => (o - r).abs() < 0.25,
            (None, None) => true,
            _ => false,
        };
        expect(
            &format!("stream out {name}"),
            close,
            format!("{o:?} vs {r:?}"),
        );
    }

    // The integrity pause around the teleport: on, then off once the
    // area is in.
    let pauses = |s: &Sections| {
        let anchor = arrived(s);
        events(s, "paused")
            .into_iter()
            .filter(|e| e.0 > anchor - 1.0)
            .map(|e| (e.0 - anchor, e.1))
            .collect::<Vec<_>>()
    };
    let (o, r) = (pauses(&ours), pauses(&real));
    let same = o.len() == r.len()
        && o.iter()
            .zip(&r)
            .all(|(a, b)| a.1 == b.1 && (a.0 - b.0).abs() < 0.15);
    expect("pause", same, format!("{o:?} vs {r:?}"));
    // RequestStreamAroundAsync returns after a frame or so.
    let took = ours["marks"]["requestTook"].as_f64().unwrap_or(1.0);
    expect("requestTook", took < 0.1, format!("{took}"));

    assert!(failures.is_empty(), "{variant}:\n{}", failures.join("\n"));
}

#[test]
fn streaming_default() {
    check("Default");
}

#[test]
fn streaming_opportunistic() {
    check("Opportunistic");
}

#[test]
fn streaming_minimum_radius_pause() {
    check("MinimumRadiusPause");
}

#[test]
fn streaming_pause_outside_loaded_area() {
    check("PauseOutsideLoadedArea");
}

fn service(dom: &WeakDom, class: &str) -> Ref {
    dom.root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == class))
        .unwrap_or_else(|| panic!("no {class}"))
}

/// A bare place, streaming on or off, with a part `Far` 3000 studs out, a
/// model with a folder in it, a server Script and a LocalScript in
/// ReplicatedFirst.
fn place(streaming: bool, server: &str, client: &str) -> WeakDom {
    let mut dom = conformance::bare_place();
    let workspace = service(&dom, "Workspace");
    let _ = dom.set_property(workspace, "StreamingEnabled", Variant::Bool(streaming));
    let far = dom.new_instance("Part", "Far", Some(workspace));
    let _ = dom.set_property(far, "Anchored", Variant::Bool(true));
    let _ = dom.set_property(
        far,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 3000.0,
                y: 5.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    );
    let model = dom.new_instance("Model", "Nest", Some(workspace));
    let folder = dom.new_instance("Folder", "Inner", Some(model));
    dom.new_instance("StringValue", "Leaf", Some(folder));
    let sss = service(&dom, "ServerScriptService");
    let script = dom.new_instance("Script", "Server", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(server.into()));
    let first = service(&dom, "ReplicatedFirst");
    let local = dom.new_instance("LocalScript", "Client", Some(first));
    let _ = dom.set_property(local, "Source", Variant::String(client.into()));
    let rs = service(&dom, "ReplicatedStorage");
    dom.new_instance("RemoteEvent", "Ping", Some(rs));
    dom.take_changes();
    dom
}

/// Runs `seconds` of a one-player session; what each side printed.
fn run(dom: WeakDom, seconds: f64) -> Vec<(Peer, String)> {
    run_over(dom, seconds, Network::default())
}

fn run_over(dom: WeakDom, seconds: f64, network: Network) -> Vec<(Peer, String)> {
    let mut session = Session::new(dom).expect("the place runs");
    session.set_network(network);
    session.start();
    session.join("Player1", -1, None).expect("joins");
    let mut lines = Vec::new();
    while session.time() < seconds {
        session.step();
        for (peer, output) in session.take_output() {
            lines.extend(
                output
                    .lines()
                    .iter()
                    .filter(|l| !common::animation_warning(l))
                    .map(|l| (peer, l.clone())),
            );
        }
    }
    lines
}

fn client_printed(lines: &[(Peer, String)]) -> Vec<&str> {
    lines
        .iter()
        .filter(|(p, _)| *p == Peer::Client(0))
        .map(|(_, l)| l.as_str())
        .collect()
}

#[test]
fn a_part_moved_into_range_streams_in_with_its_events() {
    let lines = run(
        place(
            true,
            r#"
            task.wait(1)
            local far = workspace.Far
            game.ReplicatedStorage.Ping:FireAllClients(far)
            far.Position = Vector3.new(20, 5, 0)
            task.wait(1)
            far.Color = Color3.new(1, 0, 0)
            "#,
            r#"
            game.ReplicatedStorage:WaitForChild("Ping").OnClientEvent:Connect(function(part)
                print("remote", tostring(part))
            end)
            workspace.ChildAdded:Connect(function(child)
                if child.Name == "Far" then
                    print("added", math.floor(child.Position.X))
                    child:GetPropertyChangedSignal("Color"):Connect(function()
                        print("color", child.Color.R)
                    end)
                end
            end)
            "#,
        ),
        3.0,
    );
    assert_eq!(
        client_printed(&lines),
        ["remote nil", "added 20", "color 1"],
        "{lines:#?}"
    );
}

#[test]
fn an_added_replication_focus_streams_its_area_in() {
    let lines = run(
        place(
            true,
            r#"
            local player = game.Players:GetPlayers()[1] or game.Players.PlayerAdded:Wait()
            task.wait(1)
            local anchor = Instance.new("Part")
            anchor.Anchored = true
            anchor.Position = Vector3.new(3000, 5, 40)
            anchor.Parent = workspace
            player:AddReplicationFocus(anchor)
            "#,
            r#"
            game.Loaded:Wait()
            task.wait(0.5)
            print("before", workspace:FindFirstChild("Far") ~= nil)
            print("after", workspace:WaitForChild("Far", 3) ~= nil)
            "#,
        ),
        3.0,
    );
    assert_eq!(
        client_printed(&lines),
        ["before false", "after true"],
        "{lines:#?}"
    );
}

#[test]
fn without_streaming_the_whole_place_arrives_and_requests_return() {
    let lines = run(
        place(
            false,
            r#"
            local player = game.Players:GetPlayers()[1] or game.Players.PlayerAdded:Wait()
            local t = time()
            player:RequestStreamAroundAsync(Vector3.new(0, 0, 9000))
            print("request", time() - t < 0.05)
            "#,
            r#"
            game.Loaded:Wait()
            print("far", workspace:FindFirstChild("Far") ~= nil)
            "#,
        ),
        1.0,
    );
    assert!(client_printed(&lines).contains(&"far true"), "{lines:#?}");
    assert!(
        lines.contains(&(Peer::Server, "request true".into())),
        "{lines:#?}"
    );
}

#[test]
fn descendant_added_fires_once_per_instance_on_the_join() {
    let lines = run(
        place(
            false,
            "",
            r#"
            local seen = {}
            workspace.DescendantAdded:Connect(function(d)
                seen[d.Name] = (seen[d.Name] or 0) + 1
            end)
            game.Loaded:Wait()
            task.wait()
            print("counts", seen.Nest, seen.Inner, seen.Leaf)
            "#,
        ),
        1.0,
    );
    assert!(
        client_printed(&lines).contains(&"counts 1 1 1"),
        "{lines:#?}"
    );
}

/// What streams in crosses the link like the rest: the server decides, the
/// client sees it a latency later.
#[test]
fn streamed_parts_arrive_over_the_link() {
    let arrival = |latency: f64| {
        let way = Way {
            latency,
            jitter: 0.0,
            loss: 0.0,
        };
        let lines = run_over(
            place(
                true,
                r#"
                task.wait(1)
                workspace.Far.Position = Vector3.new(20, 5, 0)
                "#,
                r#"
                workspace.ChildAdded:Connect(function(child)
                    if child.Name == "Far" then
                        print("added", time())
                    end
                end)
                "#,
            ),
            3.0,
            Network { down: way, up: way },
        );
        client_printed(&lines)
            .iter()
            .find_map(|l| l.strip_prefix("added "))
            .and_then(|t| t.parse::<f64>().ok())
            .unwrap_or_else(|| panic!("never streamed in: {lines:#?}"))
    };
    let instant = arrival(0.0);
    let slow = arrival(0.25);
    assert!(
        (slow - instant - 0.25).abs() < 0.05,
        "instant {instant}, slow {slow}"
    );
}
