//! Command bar runs checked against what Studio's command bar prints for
//! the same code in edit mode.

use rbx_dom::Variant;

use super::*;

fn lines(source: &str) -> Vec<String> {
    lines_with(source, None)
}

/// `signal_behavior` is the Workspace's stored `SignalBehavior2`.
fn lines_with(source: &str, signal_behavior: Option<u32>) -> Vec<String> {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    if let Some(behavior) = signal_behavior {
        dom.set_property(workspace, "SignalBehavior2", Variant::Enum(behavior))
            .unwrap();
    }
    match Runtime::new(dom, ReflectionDatabase::embedded())
        .unwrap()
        .run(source)
    {
        Ok(output) => output.lines().to_vec(),
        Err(error) => panic!("{error} after {:?}", error.output.lines()),
    }
}

#[test]
fn an_on_invoke_that_yields_hands_back_its_value() {
    let printed = lines(
        "local f = Instance.new('BindableFunction') \
         f.OnInvoke = function(x) task.wait(0.01) return x * 2, 'done' end \
         print(f:Invoke(21)) \
         local r = Instance.new('RemoteFunction') \
         r.OnServerInvoke = function(player, x) task.wait() return player, x end \
         print(r:InvokeServer(5)) \
         print(pcall(function() return Instance.new('BindableFunction'):Invoke() end))",
    );
    assert_eq!(
        printed,
        [
            "42 done",
            "nil 5",
            "false OnInvoke is not set on this BindableFunction"
        ]
    );
}

#[test]
fn parenting_fires_changed_for_parent() {
    let printed = lines(
        "local p = Instance.new('Part') local got = {} local n = 0 \
         p.Changed:Connect(function(name) table.insert(got, name) end) \
         p:GetPropertyChangedSignal('Parent'):Connect(function() n += 1 end) \
         p.Parent = workspace p.Parent = nil task.wait() \
         print(table.concat(got, ','), n) \
         local v = Instance.new('IntValue') local heard = 0 \
         v.Changed:Connect(function() heard += 1 end) \
         v.Parent = workspace task.wait() print(heard)",
    );
    assert_eq!(printed, ["Parent,Parent 2", "0"]);
}

#[test]
fn a_destroyed_instance_has_its_parent_locked() {
    let printed = lines(
        "local p = Instance.new('Part') p:Destroy() \
         print(pcall(function() p.Parent = workspace end)) \
         print(pcall(function() p.Parent = nil end))",
    );
    assert_eq!(
        printed,
        [
            "false The Parent property of Part is locked, current parent: NULL, new parent Workspace",
            "false The Parent property of Part is locked, current parent: NULL, new parent NULL",
        ]
    );
}

#[test]
fn edit_mode_has_no_players_and_is_not_running() {
    let printed = lines(
        "local players = game:GetService('Players') \
         print(#players:GetPlayers(), players.LocalPlayer) \
         local rs = game:GetService('RunService') \
         print(rs:IsStudio(), rs:IsEdit(), rs:IsClient(), rs:IsServer(), rs:IsRunning(), rs:IsRunMode())",
    );
    assert_eq!(printed, ["0 nil", "true true true true false false"]);
}

#[test]
fn fire_waits_for_the_next_resumption_point_under_deferred_signals() {
    let source = "local e = Instance.new('BindableEvent') local v \
                  e.Event:Connect(function(a) v = a end) \
                  e:Fire(5) print(v) task.wait() print(v)";
    // Studio, on a place saved with `SignalBehavior` Deferred.
    assert_eq!(lines_with(source, Some(2)), ["nil", "5"]);
    // `Default` is still `Immediate` (Roblox's SignalBehavior reference).
    assert_eq!(lines_with(source, Some(0)), ["5", "5"]);
    assert_eq!(lines(source), ["5", "5"]);
}

#[test]
fn shared_g_and_globals_outlive_a_run() {
    let mut dom = WeakDom::new();
    dom.new_instance("Workspace", "Workspace", None);
    let mut runtime = Runtime::new(dom, ReflectionDatabase::embedded()).unwrap();
    runtime.run("shared.count = 1 _G.n = 2 x = 3").unwrap();
    let output = runtime.run(
        "shared.count += 1 \
         print(typeof(shared), shared.count, _G.n, x, _G.print, rawequal(_G, getfenv()))",
    );
    assert_eq!(output.unwrap().lines(), ["table 2 2 3 nil false"]);
}
