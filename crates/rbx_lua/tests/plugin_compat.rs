//! Studio behaviours a real plugin (Archimedes) depends on: its scripts sit
//! under a `Plugin` instance, `SelectionChanged` reaches the plugin that
//! set the selection, settings keep Roblox values the way Studio's files
//! do, and the camera, theme, joint and debugger members answer.

use std::path::PathBuf;

use rbx_dom::{CFrameData, Variant, Vector3Data, WeakDom};
use rbx_lua::plugin::{
    Context, Input, Origin, Outcome, OutputKind, PluginHost, PluginSource, SourceBody,
};
use rbx_reflection::ReflectionDatabase;

struct Host {
    host: PluginHost,
    dom: WeakDom,
    viewport: Option<[f32; 2]>,
    _dir: Dir,
}

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Host {
    fn send(&mut self, input: Input) -> Outcome {
        let context = Context {
            now: 1.0,
            viewport_size: self.viewport,
            ..Context::default()
        };
        self.host.dispatch(&mut self.dom, context, input)
    }
}

fn prints(out: &Outcome) -> Vec<String> {
    for line in &out.output {
        assert_ne!(line.kind, OutputKind::Error, "plugin error: {}", line.text);
    }
    out.output.iter().map(|l| l.text.clone()).collect()
}

fn run(tag: &str, src: &str) -> (Host, Vec<String>) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    dom.new_instance("Part", "Part", Some(workspace));
    dom.new_instance("Players", "Players", None);
    let camera = dom.new_instance("Camera", "Camera", Some(workspace));
    dom.set_property(
        camera,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    )
    .unwrap();
    dom.set_property(workspace, "CurrentCamera", Variant::Ref(camera))
        .unwrap();
    dom.take_changes();
    let dir = Dir(std::env::temp_dir().join(format!(
        "rbx_lua_plugin_compat_{tag}_{}",
        std::process::id()
    )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let mut host = PluginHost::new(ReflectionDatabase::embedded(), Some(dir.0.clone()));
    host.add(PluginSource {
        name: "Compat".to_string(),
        body: SourceBody::Luau(src.to_string()),
        origin: Origin::Roblox {
            path: PathBuf::from("Compat.lua"),
        },
    });
    let mut h = Host {
        host,
        dom,
        viewport: Some([800.0, 600.0]),
        _dir: dir,
    };
    let out = h.send(Input::Start);
    let lines = prints(&out);
    (h, lines)
}

#[test]
fn a_plugins_scripts_sit_under_a_plugin_instance() {
    let (_, out) = run(
        "root",
        r#"
        local p = script:FindFirstAncestorWhichIsA("Plugin")
        p:SetSetting("k", 7)
        print(p.ClassName, plugin:GetSetting("k"))
        "#,
    );
    assert_eq!(out, ["Plugin 7"]);
}

#[test]
fn selection_changed_reaches_the_plugin_that_set_it() {
    let (mut h, out) = run(
        "selection",
        r#"
        local Selection = game:GetService("Selection")
        Selection.SelectionChanged:Connect(function()
            print("changed", #Selection:Get())
        end)
        local toolbar = plugin:CreateToolbar("T")
        toolbar:CreateButton("Pick", "", "").Click:Connect(function()
            Selection:Set({ workspace.Part })
        end)
        "#,
    );
    assert!(out.is_empty());
    let button = h.host.ui().toolbars[0].buttons[0].id;
    let out = h.send(Input::ButtonClicked(button));
    assert_eq!(prints(&out), ["changed 1"]);
    assert_eq!(out.selection.map(|s| s.len()), Some(1));
}

#[test]
fn settings_keep_roblox_values_in_studios_form() {
    let (h, out) = run(
        "settings",
        r#"
        plugin:SetSetting("s", { face = Enum.NormalId.Top, at = Vector3.new(1, 2, 3) })
        local back = plugin:GetSetting("s")
        print(back.face == Enum.NormalId.Top, back.at.Y)
        "#,
    );
    assert_eq!(out, ["true 2"]);
    let text = std::fs::read_to_string(h._dir.0.join("Compat.json")).unwrap();
    assert!(text.contains(r#""serializedType": "NormalId""#), "{text}");
    assert!(text.contains(r#""serializedType": "Vector3""#), "{text}");
}

#[test]
fn camera_theme_joint_and_debugger_members_answer() {
    let (_, out) = run(
        "members",
        r#"
        local camera = workspace.CurrentCamera
        local ray = camera:ScreenPointToRay(400, 300)
        print(camera.ViewportSize.X, camera.ViewportSize.Y, math.round(ray.Direction.Z))
        local studio = settings().Studio
        local bg = studio.Theme:GetColor(Enum.StudioStyleGuideColor.MainBackground)
        print(typeof(studio.ThemeChanged), math.round(bg.R * 255), math.round(bg.B * 255))
        workspace:JoinToOutsiders({ workspace.Part }, Enum.JointCreationMode.All)
        workspace.Part:BreakJoints()
        print(plugin:GetJoinMode() == Enum.JointCreationMode.None, workspace.Part:GetRootPart() == workspace.Part)
        local debugger = plugin:GetDebuggerManager():AddDebugger(script)
        debugger:SetBreakpoint(2)
        print(#debugger:GetBreakpoints())
        "#,
    );
    assert_eq!(
        out,
        ["800 600 -1", "RBXScriptSignal 25 31", "true true", "1"]
    );
}

fn model_plugin(tag: &str, scripts: &[(&str, &str, &str)]) -> (Host, Vec<String>) {
    let mut model = WeakDom::new();
    let root = model.new_instance("Folder", "Root", None);
    let mut made = std::collections::HashMap::new();
    for (class, name, source) in scripts {
        let parent = match name.rsplit_once('/') {
            Some((parent, _)) => made[parent],
            None => root,
        };
        let short = name.rsplit('/').next().unwrap();
        let r = model.new_instance(class, short, Some(parent));
        model
            .set_property(r, "Source", Variant::String(source.to_string()))
            .unwrap();
        made.insert(name.to_string(), r);
    }
    let (mut h, _) = run(tag, "");
    h.host.add(PluginSource {
        name: "Model".to_string(),
        body: SourceBody::Model(model),
        origin: Origin::Roblox {
            path: PathBuf::from("Model.rbxm"),
        },
    });
    let out = h.send(Input::Start);
    let mut lines = prints(&out);
    for _ in 0..5 {
        let mut context = Context {
            now: 2.0,
            viewport_size: h.viewport,
            ..Context::default()
        };
        context.now += 1.0;
        let out = h.host.dispatch(&mut h.dom, context, Input::Tick);
        lines.extend(prints(&out));
    }
    (h, lines)
}

#[test]
fn scripts_get_their_own_env_require_paths_and_shared_tables() {
    let (_, out) = model_plugin(
        "env",
        &[
            ("ModuleScript", "Lib", "task.wait() return { value = 7 }"),
            ("ModuleScript", "Lib/Inner", "return 'inner'"),
            (
                "Script",
                "Main",
                r#"
                local lib = require("./Lib")
                _G.seen = lib.value
                print(lib.value, require(script.Parent.Lib).value, getfenv(0).script == script)
                print(require("../Root/Lib/Inner"))
                "#,
            ),
            (
                "LocalScript",
                "Other",
                "task.wait() task.wait() print('shared', _G.seen)",
            ),
        ],
    );
    assert!(out.contains(&"7 7 true".to_string()), "{out:?}");
    assert!(out.contains(&"inner".to_string()), "{out:?}");
    assert!(out.contains(&"shared 7".to_string()), "{out:?}");
}

#[test]
fn waits_inside_wrapped_coroutines_and_parked_runner_threads() {
    let (_, out) = run(
        "coroutines",
        r#"
        local ticks = 0
        coroutine.wrap(function() while ticks < 2 do task.wait() ticks += 1 print("tick", ticks) end end)()
        -- A signal library's runner thread: parked by a plain yield, resumed
        -- with work by whoever holds it.
        local runner = coroutine.create(function()
            while true do
                local fn, arg = coroutine.yield()
                fn(arg)
            end
        end)
        coroutine.resume(runner)
        task.spawn(runner, print, "ran")
        "#,
    );
    assert!(out.contains(&"ran".to_string()), "{out:?}");
}

#[test]
fn value_objects_pass_their_value_to_changed() {
    let (_, out) = run(
        "values",
        r#"
        local v = Instance.new("NumberValue")
        v.Changed:Connect(function(value) print("changed", value) end)
        v.Value = 3
        "#,
    );
    assert_eq!(out, ["changed 3"]);
}

#[test]
fn activated_plugins_get_the_views_mouse() {
    let (mut h, out) = run(
        "mouse",
        r#"
        local mouse = plugin:GetMouse()
        mouse.Button1Down:Connect(function() print("down", mouse.X, mouse.Y) end)
        game:GetService("UserInputService").InputBegan:Connect(function(input)
            print("began", input.UserInputType.Name)
        end)
        plugin:Activate(true)
        print(plugin:IsActivated(), plugin:IsActivatedWithExclusiveMouse())
        "#,
    );
    assert_eq!(out, ["true true"]);
    assert!(h.host.active_plugin().is_some());
    let context = Context {
        now: 1.0,
        mouse_position: Some([10.0, 20.0]),
        viewport_size: h.viewport,
        ..Context::default()
    };
    let out = h.host.dispatch(
        &mut h.dom,
        context,
        Input::Mouse(rbx_lua::plugin::MouseInput::Down(1)),
    );
    let lines = prints(&out);
    assert!(
        lines.contains(&"began MouseButton1".to_string()),
        "{lines:?}"
    );
    assert!(lines.contains(&"down 10 20".to_string()), "{lines:?}");
}

#[test]
fn stand_in_services_and_web_free_members() {
    let (_, out) = run(
        "standins",
        r#"
        local s = Instance.new("Script")
        s.Source = "print(1)"
        print(game:GetService("ScriptEditorService"):GetEditorSource(s))
        game:GetService("ContentProvider"):PreloadAsync({})
        local cas = game:GetService("ContextActionService")
        cas:BindAction("x", function() end, false, Enum.KeyCode.E)
        print(cas:GetBoundActionInfo("x").callback ~= nil)
        print(game:GetService("Players"):GetUserThumbnailAsync(1, Enum.ThumbnailType.HeadShot, Enum.ThumbnailSize.Size150x150))
        "#,
    );
    assert_eq!(out[0], "print(1)");
    assert_eq!(out[1], "true");
    assert_eq!(
        out[2],
        "rbxthumb://type=AvatarHeadShot&id=1&w=150&h=150 true"
    );
}
