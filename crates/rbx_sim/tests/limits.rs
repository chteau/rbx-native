//! The engine limits a game script runs into (the ScriptLimits golden):
//! re-entrancy caps reported through `LogService.MessageOut`, and frames
//! that overrun under wall clock pacing (FrameOverrun).

use rbx_dom::{Variant, WeakDom};
use rbx_sim::{conformance, Session};

fn place(source: &str) -> WeakDom {
    let mut dom = conformance::bare_place();
    let sss = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| {
            dom.get(*r)
                .is_some_and(|i| i.class() == "ServerScriptService")
        })
        .expect("ServerScriptService");
    let script = dom.new_instance("Script", "Limits", Some(sss));
    let _ = dom.set_property(script, "Source", Variant::String(source.into()));
    dom.take_changes();
    dom
}

/// What the server printed over `frames` frames.
fn run(source: &str, frames: usize) -> Vec<String> {
    let mut session = Session::new(place(source)).expect("the place runs");
    session.start();
    for _ in 0..frames {
        session.step();
    }
    session
        .take_output()
        .into_iter()
        .flat_map(|(_, output)| output.lines().to_vec())
        .collect()
}

#[test]
fn a_defer_chain_stops_at_80_and_says_so() {
    let lines = run(
        r#"
        local logs = {}
        game:GetService("LogService").MessageOut:Connect(function(message, kind)
            table.insert(logs, kind.Name .. " " .. message)
        end)
        local count = 0
        local function again()
            count += 1
            task.defer(again)
        end
        task.defer(again)
        task.wait(0.1)
        print("count", count)
        print("log", logs[1])
        "#,
        20,
    );
    assert!(lines.iter().any(|l| l == "count 80"), "{lines:?}");
    let log = lines.iter().find(|l| l.starts_with("log ")).unwrap();
    assert!(
        log.starts_with(
            "log MessageError Maximum re-entrancy depth (80) exceeded calling task.defer\n    \
             Stack Begin\n    Script 'ServerScriptService.Limits', Line 9 - function again"
        ),
        "{log}"
    );
}

#[test]
fn an_event_firing_itself_stops_at_6() {
    let lines = run(
        r#"
        local fired, changes = 0, 0
        local event = Instance.new("BindableEvent")
        event.Event:Connect(function()
            fired += 1
            event:Fire()
        end)
        event:Fire()
        local value = Instance.new("IntValue")
        value.Changed:Connect(function()
            changes += 1
            value.Value += 1
        end)
        value.Value = 1
        task.wait(0.1)
        print("counts", fired, changes)
        "#,
        20,
    );
    assert!(lines.iter().any(|l| l == "counts 6 6"), "{lines:?}");
    assert!(lines.iter().any(|l| l
        == "Maximum event re-entrancy depth exceeded for BindableEvent.Event when calling \
            anonymous function on line 4 in ServerScriptService.Limits"));
    assert!(lines.iter().any(|l| l
        == "Maximum event re-entrancy depth exceeded for IntValue.Changed when calling \
            anonymous function on line 10 in ServerScriptService.Limits"));
}

#[test]
fn an_overrun_frame_stretches_the_next_step_only() {
    let source = r#"
        local RunService = game:GetService("RunService")
        local seen = {}
        local frame = 0
        RunService.Heartbeat:Connect(function(dt)
            frame += 1
            if frame == 3 then
                local t = os.clock()
                while os.clock() - t < 0.2 do end
            end
            if frame >= 3 and frame <= 5 then
                table.insert(seen, string.format("%.2f", dt))
            end
            if frame == 5 then
                print(table.concat(seen, " "), string.format("%.3f", time()))
            end
        end)
    "#;
    let mut session = Session::new(place(source)).expect("the place runs");
    session.set_pacing(rbx_sim::Pacing::Scripts);
    session.start();
    for _ in 0..6 {
        session.step();
    }
    let lines: Vec<String> = session
        .take_output()
        .into_iter()
        .flat_map(|(_, output)| output.lines().to_vec())
        .collect();
    let line = lines
        .iter()
        .find(|l| l.starts_with("0.02 "))
        .expect("printed");
    // The step after the stall reports its real length; the world moves
    // on 3 frames (7 frames in all by frame 5's Heartbeat: 1+1+1+3+1).
    let parts: Vec<&str> = line.split(' ').collect();
    assert!(parts[1].parse::<f64>().unwrap() >= 0.2, "{line}");
    assert_eq!(parts[3], "0.117", "{line}");
}
