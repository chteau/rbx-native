use rbx_dom::{Ref, Variant, WeakDom};
use rbx_lua::test_runner::{discover, discover_integration, Report, Status, TestResult};

use super::*;
use crate::conformance::bare_place;

fn service(dom: &WeakDom, class: &str) -> Ref {
    *dom.root_refs()
        .iter()
        .find(|r| dom.get(**r).is_some_and(|i| i.class() == class))
        .expect("bare_place has it")
}

fn add(dom: &mut WeakDom, parent: Ref, class: &str, name: &str, source: &str) {
    let made = dom.new_instance(class, name, Some(parent));
    if !source.is_empty() {
        dom.set_property(made, "Source", Variant::String(source.into()))
            .unwrap();
    }
}

/// The bare place with an `Add` RemoteFunction the server answers, and
/// `spec` as `ServerStorage.Game.integration.spec`.
fn place(spec: &str) -> WeakDom {
    let mut dom = bare_place();
    let storage = service(&dom, "ReplicatedStorage");
    add(&mut dom, storage, "RemoteFunction", "Add", "");
    let scripts = service(&dom, "ServerScriptService");
    add(
        &mut dom,
        scripts,
        "Script",
        "Server",
        "game.ReplicatedStorage.Add.OnServerInvoke = function(_, a, b) return a + b end",
    );
    let specs = service(&dom, "ServerStorage");
    add(
        &mut dom,
        specs,
        "ModuleScript",
        "Game.integration.spec",
        spec,
    );
    dom
}

fn run_with(dom: &WeakDom, config: &Config) -> Report {
    let files = discover_integration(dom, &[]).unwrap();
    assert_eq!(files.len(), 1);
    run(
        dom,
        &files,
        ReflectionDatabase::shared(),
        &Options::default(),
        config,
    )
}

fn result<'a>(report: &'a Report, name: &str) -> &'a TestResult {
    report
        .results()
        .find(|r| r.path.last().map(String::as_str) == Some(name))
        .unwrap_or_else(|| panic!("no test {name}: {report:#?}"))
}

#[test]
fn integration_specs_are_found_apart_from_plain_ones() {
    let dom = place("return function() end");
    assert!(discover(&dom, &[]).unwrap().is_empty());
    assert_eq!(
        discover_integration(&dom, &[]).unwrap()[0].name,
        "ServerStorage.Game.integration.spec"
    );
}

#[test]
fn players_join_act_and_clients_answer_evals() {
    let report = run_with(
        &place(
            r#"
return function()
    it("joins, evals and leaves", function()
        local one = LocalRun.addPlayer()
        local two = LocalRun.addPlayer("Guest")
        expect(one.Name).to.equal("Player1")
        expect(one.UserId).to.equal(-1)
        expect(two.UserId < -1000).to.equal(true)
        expect(one.Character).to.be.ok()
        local name, sum = LocalRun.client(two):eval([[
            return game:GetService("Players").LocalPlayer.Name,
                game.ReplicatedStorage.Add:InvokeServer(...)
        ]], 20, 22)
        expect(name).to.equal("Guest")
        expect(sum).to.equal(42)
        local character = LocalRun.client(one):eval("return game.Players.LocalPlayer.Character")
        expect(character).to.equal(one.Character)
        LocalRun.removePlayer(two)
        expect(#game.Players:GetPlayers()).to.equal(1)
    end)
    it("runs the simulated clock", function()
        local started = time()
        LocalRun.runFor(2)
        expect(time() - started).to.be.near(2, 0.02)
        local n = 0
        local value = LocalRun.waitUntil(function()
            n += 1
            return n == 30 and "done"
        end)
        expect(value).to.equal("done")
    end)
end
"#,
        ),
        &Config::default(),
    );
    assert!(report.passed(), "{}", test_runner::human(&report));
    assert_eq!(report.count(Status::Passed), 2);
}

#[test]
fn a_failing_spec_says_why_where_and_what_the_clients_printed() {
    let report = run_with(
        &place(
            r#"
return function()
    it("sees the wrong sum", function()
        local player = LocalRun.addPlayer()
        local sum = LocalRun.client(player):eval([[
            print("client adds")
            return game.ReplicatedStorage.Add:InvokeServer(1, 1)
        ]])
        expect(sum).to.equal(3)
    end)
    it("times out", function()
        LocalRun.waitUntil(function() return false end, 0.5)
    end)
    it("reports a client error", function()
        LocalRun.client(game.Players.Player1):eval("error('boom', 0)")
    end)
    it("passes after the failures", function() end)
end
"#,
        ),
        &Config::default(),
    );
    assert!(!report.passed());
    let sum = result(&report, "sees the wrong sum");
    assert_eq!(sum.status, Status::Failed);
    assert_eq!(
        sum.message.as_deref(),
        Some("Expected 3 (number), got 2 (number)")
    );
    let at = sum.location.as_ref().expect("located");
    assert_eq!(
        (at.name.as_str(), at.line),
        ("ServerStorage.Game.integration.spec", 9)
    );
    assert!(
        sum.output
            .lines()
            .contains(&"[Player1] client adds".to_string()),
        "{:?}",
        sum.output
    );
    let timeout = result(&report, "times out");
    assert!(
        timeout
            .message
            .as_deref()
            .is_some_and(|m| m.contains("LocalRun.waitUntil timed out after 0.5 s")),
        "{timeout:?}"
    );
    let error = result(&report, "reports a client error");
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|m| m.contains("Player1: boom")),
        "{error:?}"
    );
    assert_eq!(
        result(&report, "passes after the failures").status,
        Status::Passed
    );
}

#[test]
fn the_same_seed_runs_the_same_way() {
    let dom = place(
        r#"
return function()
    it("rolls", function()
        print(math.random(1, 1e9), Random.new():NextInteger(1, 1e9))    end)
end
"#,
    );
    let rolls = |seed| {
        let report = run_with(
            &dom,
            &Config {
                seed,
                ..Config::default()
            },
        );
        report.files[0].output.text()
    };
    assert_eq!(rolls(7), rolls(7));
    assert_ne!(rolls(7), rolls(8));
}

#[test]
fn the_network_starts_from_the_config_and_profiles_switch_it() {
    let report = run_with(
        &place(
            r#"
return function()
    it("pings", function()
        local player = LocalRun.addPlayer()
        expect(player:GetNetworkPing()).to.be.near(0.05, 1e-6)
        LocalRun.setNetwork({ down = { latency = 0.1 }, up = { latency = 0.2 } })
        expect(player:GetNetworkPing()).to.be.near(0.3, 1e-6)
        expect(function() LocalRun.setNetwork("dialup") end).to.throw()
    end)
end
"#,
        ),
        &Config {
            network: net(way(25.0, 0.0, 0.0), way(25.0, 0.0, 0.0)),
            ..Config::default()
        },
    );
    assert!(report.passed(), "{}", test_runner::human(&report));
}

#[test]
fn profiles_are_the_network_simulators_presets() {
    for preset in rbx_play::netsim::PRESETS {
        let local = |w: rbx_play::netsim::OneWay| way(w.latency_ms, w.jitter_ms, w.loss_percent);
        let network = net(
            local(preset.conditions.inbound),
            local(preset.conditions.outbound),
        );
        assert!(
            PROFILES.iter().any(|(_, p)| *p == network),
            "{} is missing",
            preset.name
        );
    }
    assert_eq!(profile("3G"), profile("3g"));
    assert_eq!(profile("none"), Some(Network::default()));
}

#[test]
fn act_clicks_gui_walks_on_keys_and_chats() {
    let mut dom = place(
        r#"
return function()
    it("clicks a button", function()
        local player = LocalRun.addPlayer()
        LocalRun.act(player, { click = "PlayerGui.Shop.Buy" })
        local bought = LocalRun.waitUntil(function()
            return game.ReplicatedStorage:GetAttribute("bought")
        end, 1)
        expect(bought).to.equal("Player1")
    end)
    it("walks while W is down", function()
        local root = game.Players.Player1.Character.HumanoidRootPart
        local start = root.Position
        LocalRun.act(game.Players.Player1, { keyDown = "W" })
        LocalRun.runFor(1)
        LocalRun.act(game.Players.Player1, { keyUp = "W" })
        expect((root.Position - start).Magnitude > 5).to.equal(true)
    end)
    it("chats", function()
        local client = LocalRun.client(game.Players.Player1)
        client:eval([[
            game:GetService("TextChatService").MessageReceived:Connect(function(m)
                _G.heard = m.Text
            end)
        ]])
        LocalRun.act(game.Players.Player1, { chat = "hello" })
        local heard = LocalRun.waitUntil(function()
            return client:eval("return _G.heard")
        end, 2)
        expect(heard).to.equal("hello")
    end)
end
"#,
    );
    let storage = service(&dom, "ReplicatedStorage");
    add(&mut dom, storage, "RemoteEvent", "Buy", "");
    let scripts = service(&dom, "ServerScriptService");
    add(
        &mut dom,
        scripts,
        "Script",
        "Shop",
        "game.ReplicatedStorage.Buy.OnServerEvent:Connect(function(p) \
         game.ReplicatedStorage:SetAttribute('bought', p.Name) end)",
    );
    let starter = service(&dom, "StarterGui");
    let gui = dom.new_instance("ScreenGui", "Shop", Some(starter));
    let button = dom.new_instance("TextButton", "Buy", Some(gui));
    add(
        &mut dom,
        button,
        "LocalScript",
        "Click",
        "script.Parent.Size = UDim2.fromOffset(200, 50)\n\
         script.Parent.Position = UDim2.fromOffset(300, 200)\n\
         script.Parent.Activated:Connect(function() game.ReplicatedStorage.Buy:FireServer() end)",
    );
    let report = run_with(&dom, &Config::default());
    assert!(report.passed(), "{}", test_runner::human(&report));
}
