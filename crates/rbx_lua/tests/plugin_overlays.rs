//! Plugin ScreenGuis parented to CoreGui surface as overlay widgets.

use std::path::PathBuf;

use rbx_dom::WeakDom;
use rbx_lua::plugin::{
    Context, Input, Origin, PluginHost, PluginSource, SourceBody, Widget, WidgetKind,
};
use rbx_reflection::ReflectionDatabase;

const SCRIPT: &str = r#"
local g = Instance.new("ScreenGui")
g.Name = "Arc"
local f = Instance.new("Frame", g)
g.Parent = game:GetService("CoreGui")
local n = 0
game:GetService("RunService").Heartbeat:Connect(function()
    n += 1
    if n == 1 then
        f.BackgroundTransparency = 0.5
    elseif n == 2 then
        g.Enabled = false
    elseif n == 3 then
        g:Destroy()
    end
end)
"#;

fn send(host: &mut PluginHost, dom: &mut WeakDom, now: f64, input: Input) {
    let ctx = Context {
        now,
        ..Context::default()
    };
    host.dispatch(dom, ctx, input);
}

fn overlays(host: &PluginHost) -> Vec<Widget> {
    host.ui().widgets
}

#[test]
fn overlay_lifecycle() {
    let mut dom = WeakDom::new();
    dom.new_instance("Workspace", "Workspace", None);
    dom.take_changes();
    let mut host = PluginHost::new(ReflectionDatabase::embedded(), None);
    host.add(PluginSource {
        name: "Overlay".to_string(),
        body: SourceBody::Luau(SCRIPT.to_string()),
        origin: Origin::Roblox {
            path: PathBuf::from("Overlay.lua"),
        },
    });
    send(&mut host, &mut dom, 1.0, Input::Start);

    let widgets = overlays(&host);
    assert_eq!(widgets.len(), 1);
    let w = widgets[0].clone();
    assert_eq!(w.title, "Arc");
    assert!(w.enabled);
    assert!(matches!(w.kind, WidgetKind::Overlay { display_order: 0 }));
    let view = host.widget_dom(w.id).unwrap();
    let root = view.get(w.root).unwrap();
    assert_eq!(root.class(), "ScreenGui");
    assert_eq!(root.children().len(), 1);
    assert_eq!(view.get(root.children()[0]).unwrap().class(), "Frame");

    send(&mut host, &mut dom, 2.0, Input::Tick);
    let after = overlays(&host)[0].clone();
    assert!(after.revision > w.revision);

    send(&mut host, &mut dom, 3.0, Input::Tick);
    assert!(!overlays(&host)[0].enabled);

    send(&mut host, &mut dom, 4.0, Input::Tick);
    assert!(overlays(&host).is_empty());
}
