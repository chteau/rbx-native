//! `Outcome::before`: the place as it stood before a dispatch's first
//! script write to it, for edits no user input caused.

use std::path::PathBuf;

use rbx_dom::WeakDom;
use rbx_lua::plugin::{Context, Input, Origin, Outcome, PluginHost, PluginSource, SourceBody};
use rbx_reflection::ReflectionDatabase;

const SCRIPT: &str = r#"
local own = Instance.new("Folder")
own.Name = "Own"
local part = workspace:WaitForChild("Part")
local n = 0
game:GetService("RunService").Heartbeat:Connect(function()
    n += 1
    if n == 1 then
        own.Name = "Renamed"
    elseif n == 2 then
        part.Name = "Moved"
        part.Name = "MovedTwice"
    elseif n == 3 then
        Instance.new("Part", workspace).Name = "Made"
    end
end)
"#;

fn send(host: &mut PluginHost, dom: &mut WeakDom, now: f64, input: Input) -> Outcome {
    let ctx = Context {
        now,
        ..Context::default()
    };
    host.dispatch(dom, ctx, input)
}

fn names(dom: &WeakDom) -> Vec<String> {
    let workspace = dom.root_refs()[0];
    dom.get(workspace)
        .unwrap()
        .children()
        .iter()
        .map(|r| dom.get(*r).unwrap().name().to_string())
        .collect()
}

#[test]
fn background_writes_carry_the_place_before_them() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    dom.new_instance("Part", "Part", Some(workspace));
    dom.take_changes();
    let mut host = PluginHost::new(ReflectionDatabase::embedded(), None);
    host.add(PluginSource {
        name: "Ticker".to_string(),
        body: SourceBody::Luau(SCRIPT.to_string()),
        origin: Origin::Roblox {
            path: PathBuf::from("Ticker.lua"),
        },
    });
    assert!(send(&mut host, &mut dom, 1.0, Input::Start)
        .before
        .is_none());

    // Its own instance only: no undo point.
    let out = send(&mut host, &mut dom, 2.0, Input::Tick);
    assert!(out.before.is_none());

    // Two writes to the place: the copy predates the first.
    let out = send(&mut host, &mut dom, 3.0, Input::Tick);
    assert_eq!(names(&dom), ["MovedTwice"]);
    let before = out.before.expect("a place write leaves an undo point");
    assert_eq!(names(&before), ["Part"]);
    assert_eq!(
        before.root_refs(),
        dom.root_refs(),
        "no plugin roots in the copy"
    );

    // A new instance parented into the place.
    let out = send(&mut host, &mut dom, 4.0, Input::Tick);
    assert_eq!(names(&dom), ["MovedTwice", "Made"]);
    assert_eq!(names(&out.before.unwrap()), ["MovedTwice"]);

    let out = send(&mut host, &mut dom, 5.0, Input::Tick);
    assert!(out.before.is_none());
}
