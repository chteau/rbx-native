//! The plugin host through its public API only: scripts go in as plugins,
//! editor inputs are dispatched, and the outcome, UI and DOM are inspected.

use std::path::PathBuf;

use rbx_dom::{Change, Ref, Variant, Vector3Data, WeakDom};
use rbx_lua::plugin::{
    Answer, Context, GuiEvent, GuiEventKind, HandleEvent, HandleEventKind, Input, Manifest, Origin,
    OutputKind, Outcome, PluginHost, PluginSource, PromptKind, Request, SourceBody,
};
use rbx_reflection::ReflectionDatabase;

struct Fixture {
    host: PluginHost,
    dom: WeakDom,
    part: Ref,
    workspace: Ref,
    now: f64,
    _dir: tempdir::Dir,
}

mod tempdir {
    use std::path::PathBuf;

    pub struct Dir(pub PathBuf);

    impl Dir {
        pub fn new(tag: &str) -> Dir {
            let path = std::env::temp_dir().join(format!(
                "rbx_lua_plugin_host_{}_{}",
                tag,
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Dir(path)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn roblox(src: &str) -> PluginSource {
    PluginSource {
        name: "TestPlugin".to_string(),
        body: SourceBody::Luau(src.to_string()),
        origin: Origin::Roblox {
            path: PathBuf::from("TestPlugin.lua"),
        },
    }
}

fn fixture(tag: &str, source: PluginSource) -> Fixture {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "Part", Some(workspace));
    dom.set_property(
        part,
        "Size",
        Variant::Vector3(Vector3Data {
            x: 4.0,
            y: 1.0,
            z: 2.0,
        }),
    )
    .unwrap();
    dom.take_changes();
    let dir = tempdir::Dir::new(tag);
    let mut host = PluginHost::new(ReflectionDatabase::embedded(), Some(dir.0.clone()));
    host.add(source);
    let mut f = Fixture {
        host,
        dom,
        part,
        workspace,
        now: 100.0,
        _dir: dir,
    };
    f.send(Input::Start);
    f
}

impl Fixture {
    fn context(&self) -> Context {
        Context {
            now: self.now,
            ..Context::default()
        }
    }

    fn send(&mut self, input: Input) -> Outcome {
        let ctx = self.context();
        self.host.dispatch(&mut self.dom, ctx, input)
    }

    fn send_ctx(&mut self, ctx: Context, input: Input) -> Outcome {
        self.host.dispatch(&mut self.dom, ctx, input)
    }
}

fn started(tag: &str, src: &str) -> (Fixture, Outcome) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "Part", Some(workspace));
    dom.take_changes();
    let dir = tempdir::Dir::new(tag);
    let mut host = PluginHost::new(ReflectionDatabase::embedded(), Some(dir.0.clone()));
    host.add(roblox(src));
    let mut f = Fixture {
        host,
        dom,
        part,
        workspace,
        now: 100.0,
        _dir: dir,
    };
    let out = f.send(Input::Start);
    (f, out)
}

fn texts(out: &Outcome, kind: OutputKind) -> Vec<String> {
    out.output
        .iter()
        .filter(|l| l.kind == kind)
        .map(|l| l.text.clone())
        .collect()
}

fn all_refs(dom: &WeakDom) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        out.push(r);
        if let Some(i) = dom.get(r) {
            stack.extend_from_slice(i.children());
        }
    }
    out
}

#[test]
fn button_click_renames_part() {
    let src = r#"
        local tb = plugin:CreateToolbar("Tools")
        local btn = tb:CreateButton("rename", "Rename it", "", "Rename")
        btn.Click:Connect(function() workspace.Part.Name = "Renamed" end)
    "#;
    let mut f = fixture("click", roblox(src));
    let ui = f.host.ui();
    assert_eq!(ui.toolbars.len(), 1);
    assert_eq!(ui.toolbars[0].name, "Tools");
    assert_eq!(ui.toolbars[0].buttons.len(), 1);
    assert_eq!(ui.toolbars[0].buttons[0].key, "rename");
    assert_eq!(ui.toolbars[0].buttons[0].tooltip, "Rename it");
    let id = ui.toolbars[0].buttons[0].id;
    let out = f.send(Input::ButtonClicked(id));
    assert!(out.changes.contains(&Change::Property {
        referent: f.part,
        name: "Name".to_string()
    }));
    assert_eq!(f.dom.get(f.part).unwrap().name(), "Renamed");
}

#[test]
fn plugin_instances_never_reach_the_place() {
    let src = r#"
        local info = DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Float, true, false, 200, 100, 100, 50)
        local w = plugin:CreateDockWidgetPluginGui("w", info)
        local b = Instance.new("TextButton")
        b.Parent = w
        local loose = Instance.new("Part")
        loose.Name = "Loose"
    "#;
    let (f, out) = started("hidden", src);
    assert!(texts(&out, OutputKind::Error).is_empty());
    for r in all_refs(&f.dom) {
        let class = f.dom.get(r).unwrap().class();
        let name = f.dom.get(r).unwrap().name();
        assert_ne!(class, "Script");
        assert_ne!(class, "DockWidgetPluginGui");
        assert_ne!(class, "TextButton");
        assert_ne!(name, "Loose");
    }
    assert_eq!(f.host.ui().widgets.len(), 1);
}

#[test]
fn selection_set_and_changed() {
    let src = r#"
        local Selection = game:GetService("Selection")
        Selection:Set({workspace.Part})
        Selection.SelectionChanged:Connect(function() print("selection changed") end)
    "#;
    let (mut f, out) = started("selection", src);
    assert_eq!(out.selection, Some(vec![f.part]));
    let ctx = Context {
        now: f.now,
        selection: vec![f.part],
        ..Context::default()
    };
    let out = f.send_ctx(ctx, Input::SelectionChanged);
    assert!(texts(&out, OutputKind::Print)
        .iter()
        .any(|t| t.contains("selection changed")));
}

#[test]
fn wait_resolves_on_tick_after_time_passes() {
    let src = r#"
        task.wait(0.5)
        print("woke")
    "#;
    let (mut f, out) = started("wait", src);
    assert!(texts(&out, OutputKind::Print).is_empty());
    assert!(f.host.wants_tick(f.now + 0.6) || f.host.wants_tick(f.now));
    f.now += 0.1;
    let out = f.send(Input::Tick);
    assert!(texts(&out, OutputKind::Print).is_empty());
    assert!(f.host.wants_tick(f.now));
    f.now += 0.6;
    let out = f.send(Input::Tick);
    assert!(texts(&out, OutputKind::Print)
        .iter()
        .any(|t| t.contains("woke")));
}

#[test]
fn widget_dom_and_gui_click() {
    let src = r#"
        local info = DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Float, true, false, 200, 100, 100, 50)
        local w = plugin:CreateDockWidgetPluginGui("w", info)
        local b = Instance.new("TextButton")
        b.Name = "B"
        b.Parent = w
        b.MouseButton1Click:Connect(function() print("gui clicked") end)
    "#;
    let (mut f, _) = started("widget", src);
    let ui = f.host.ui();
    let widget = &ui.widgets[0];
    let wdom = f.host.widget_dom(widget.id).expect("widget dom");
    let screen = wdom
        .root_refs()
        .iter()
        .copied()
        .find(|&r| wdom.get(r).unwrap().class() == "ScreenGui")
        .expect("ScreenGui");
    assert_eq!(screen, widget.root);
    let button = wdom
        .get(screen)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|&r| wdom.get(r).unwrap().class() == "TextButton")
        .expect("button copy");
    let out = f.send(Input::Gui(GuiEvent {
        referent: button,
        kind: GuiEventKind::Click {
            button: 1,
            x: 1.0,
            y: 1.0,
        },
    }));
    assert!(texts(&out, OutputKind::Print)
        .iter()
        .any(|t| t.contains("gui clicked")));
}

#[test]
fn part_parented_to_workspace_is_added() {
    let src = r#"
        local p = Instance.new("Part")
        p.Name = "Made"
        p.Parent = workspace
    "#;
    let (f, out) = started("added", src);
    let made = f
        .dom
        .get(f.workspace)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|&r| f.dom.get(r).unwrap().name() == "Made")
        .expect("part in workspace");
    assert!(out.changes.contains(&Change::Added(made)));
}

#[test]
fn print_warn_error_outputs() {
    let src = r#"
        print("hello")
        warn("careful")
        error("boom")
    "#;
    let (_, out) = started("output", src);
    assert!(texts(&out, OutputKind::Print)
        .iter()
        .any(|t| t.contains("hello")));
    assert!(texts(&out, OutputKind::Warn)
        .iter()
        .any(|t| t.contains("careful")));
    assert!(texts(&out, OutputKind::Error)
        .iter()
        .any(|t| t.contains("boom")));
    assert!(out.output.iter().all(|l| l.source == "TestPlugin"));
}

#[test]
fn unload_fires_unloading_and_removes_ui() {
    let src = r#"
        local tb = plugin:CreateToolbar("Tools")
        tb:CreateButton("k", "tip", "", "K")
        plugin.Unloading:Connect(function() print("bye") end)
    "#;
    let (mut f, _) = started("unload", src);
    assert_eq!(f.host.ui().toolbars.len(), 1);
    let id = f.host.plugins()[0].id;
    let out = f.send(Input::Unload(id));
    assert!(texts(&out, OutputKind::Print)
        .iter()
        .any(|t| t.contains("bye")));
    assert!(f.host.ui().toolbars.is_empty());
}

#[test]
fn dom_changed_fires_changed_connection() {
    let src = r#"
        workspace.Part.Changed:Connect(function(prop) print("changed " .. prop) end)
    "#;
    let (mut f, _) = started("domchanged", src);
    f.dom
        .set_property(f.part, "Anchored", Variant::Bool(true))
        .unwrap();
    let changes = f.dom.take_changes();
    assert!(f.host.watches(&changes));
    let out = f.send(Input::DomChanged(changes));
    assert!(texts(&out, OutputKind::Print)
        .iter()
        .any(|t| t.contains("changed Anchored")));
}

#[test]
fn color_prompt_round_trip() {
    let src = r#"
        local c = plugin:PromptColor3(Color3.new(1, 0, 0))
        print("picked " .. tostring(c))
    "#;
    let (mut f, out) = started("prompt", src);
    let (id, kind) = out
        .requests
        .iter()
        .find_map(|r| match r {
            Request::Prompt { id, kind, .. } => Some((*id, kind.clone())),
            _ => None,
        })
        .expect("prompt request");
    assert!(matches!(kind, PromptKind::Color3 { .. }));
    let out = f.send(Input::PromptAnswered(id, Answer::Color3(Some([0.0, 1.0, 0.0]))));
    assert!(texts(&out, OutputKind::Print)
        .iter()
        .any(|t| t.contains("picked")));
}

#[test]
fn handles_adornments_and_drag() {
    let src = r#"
        local h = Instance.new("Handles")
        h.Adornee = workspace.Part
        h.Parent = game:GetService("CoreGui")
        h.MouseDrag:Connect(function(face, dist) print("drag " .. tostring(dist)) end)
    "#;
    let (mut f, _) = started("handles", src);
    let adornments = f.host.adornments();
    let handles = adornments
        .iter()
        .find(|i| i.class() == "Handles")
        .expect("handles adornment");
    let referent = handles.referent();
    let out = f.send(Input::Handle(HandleEvent {
        referent,
        kind: HandleEventKind::Drag {
            face: 1,
            distance: 2.0,
        },
    }));
    assert!(texts(&out, OutputKind::Print)
        .iter()
        .any(|t| t.contains("drag 2")));
}

#[test]
fn rbxnative_only_for_native_plugins() {
    let src = r#"print("native=" .. tostring(rbxnative ~= nil))"#;
    let (_, out) = started("roblox", src);
    assert!(texts(&out, OutputKind::Print)
        .iter()
        .any(|t| t.contains("native=false")));

    let manifest = Manifest {
        name: "Native".to_string(),
        version: "1.0".to_string(),
        entry: "init.luau".to_string(),
        ..Manifest::default()
    };
    let source = PluginSource {
        name: "Native".to_string(),
        body: SourceBody::Luau(src.to_string()),
        origin: Origin::Native {
            path: PathBuf::from("plugin.toml"),
            manifest,
        },
    };
    let f = fixture("native", source);
    assert!(f.host.plugins()[0].started);
    let mut f = f;
    let out = f.send(Input::Tick);
    let _ = out;
    let dir = tempdir::Dir::new("native2");
    let mut host = PluginHost::new(ReflectionDatabase::embedded(), Some(dir.0.clone()));
    let mut dom = WeakDom::new();
    dom.new_instance("Workspace", "Workspace", None);
    host.add(PluginSource {
        name: "Native".to_string(),
        body: SourceBody::Luau(src.to_string()),
        origin: Origin::Native {
            path: PathBuf::from("plugin.toml"),
            manifest: Manifest {
                name: "Native".to_string(),
                ..Manifest::default()
            },
        },
    });
    let out = host.dispatch(&mut dom, Context::default(), Input::Start);
    assert!(texts(&out, OutputKind::Print)
        .iter()
        .any(|t| t.contains("native=true")));
}

#[test]
fn waypoint_request() {
    let src = r#"
        game:GetService("ChangeHistoryService"):SetWaypoint("my step")
    "#;
    let (_, out) = started("waypoint", src);
    assert!(out
        .requests
        .contains(&Request::Waypoint("my step".to_string())));
}
