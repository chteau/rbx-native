//! Studio's own API from the Command Bar, checked against what real
//! Studio 0.748's Command Bar printed for the same code.

use rbx_dom::WeakDom;
use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;

fn run(source: &str) -> Vec<String> {
    let mut dom = WeakDom::new();
    dom.new_instance("Workspace", "Workspace", None);
    let mut runtime = Runtime::new(dom, ReflectionDatabase::embedded()).unwrap();
    let output = runtime.run(source).unwrap_or_else(|e| panic!("{e:?}"));
    output.lines().to_vec()
}

fn one(source: &str) -> String {
    run(source).join("\n")
}

#[test]
fn settings_tree_matches_studio() {
    assert_eq!(
        one(
            "print(settings().Name, settings().ClassName, UserSettings().Name, typeof(settings()))"
        ),
        "Global Settings GlobalSettings GlobalBasicSettings Instance"
    );
    assert_eq!(
        one("local t = {} for _, c in settings():GetChildren() do table.insert(t, c.Name) end print(table.concat(t, ','))"),
        "Game Options,Lua,Diagnostics,Physics,Studio,Network,Rendering,Task Scheduler"
    );
    assert_eq!(
        one("local s = settings().Studio print(s.Parent.Name, s:GetFullName(), settings():GetService('Studio') == s, settings().Rendering.ClassName)"),
        "Global Settings Global Settings.Studio true RenderSettings"
    );
    assert_eq!(
        one("print(pcall(function() return settings():GetService('Nope') end))"),
        "false 'Nope' is not a valid Service name"
    );
    assert_eq!(
        one("print(UserSettings().GameSettings.ClassName)"),
        "UserGameSettings"
    );
}

#[test]
fn studio_values_and_writes() {
    let s = "local s = settings().Studio ";
    assert_eq!(
        one(&format!(
            "{s} print(s['Auto Indent Rule'], s['Animation Skeleton Scale'], s['Active Color'])"
        )),
        "Enum.AutoIndentRule.Absolute 1 1, 0.5, 0"
    );
    // Studio's coercions.
    assert_eq!(
        one(&format!("{s} s['Animate Hover Over'] = 5 local a = s['Animate Hover Over'] s['Animation Skeleton Scale'] = 'abc' print(a, s['Animation Skeleton Scale'])")),
        "true 0"
    );
    assert_eq!(
        one(&format!(
            "{s} print(pcall(function() s['Active Color'] = 5 end))"
        )),
        "false Unable to assign property Active Color. Color3 expected, got number"
    );
    assert_eq!(
        one(&format!("{s} print(pcall(function() s['Auto Indent Rule'] = 999 end))")),
        "false Unable to assign property Auto Indent Rule. Invalid value 999 for enum AutoIndentRule"
    );
    assert_eq!(
        one(&format!("{s} print(pcall(function() s['Auto Indent Rule'] = Enum.Material.Plastic end))")),
        "false Unable to assign property Auto Indent Rule. EnumItem of type AutoIndentRule expected, got an EnumItem of type Material"
    );
    assert_eq!(
        one(&format!(
            "{s} s['Auto Indent Rule'] = 0 print(s['Auto Indent Rule'])"
        )),
        "Enum.AutoIndentRule.Off"
    );
    assert_eq!(
        one(&format!(
            "{s} print(pcall(function() return s.NoSuchThing end))"
        )),
        "false NoSuchThing is not a valid member of Studio \"Studio\""
    );
    assert_eq!(
        one(&format!("{s} print(pcall(function() return s.Font end))")),
        "false Unable to get property Font, type QFont"
    );
    assert_eq!(
        one("print(pcall(function() settings().Diagnostics.RobloxVersion = 'x' end))"),
        "false Unable to assign property RobloxVersion. Property is read only"
    );
    assert_eq!(
        one("print(settings().Rendering:GetMaxQualityLevel())"),
        "22"
    );
}

#[test]
fn studio_changes_fire_signals() {
    assert_eq!(
        one("local s = settings().Studio local seen = {}
             s.Changed:Connect(function(name) table.insert(seen, 'changed ' .. name) end)
             s:GetPropertyChangedSignal('Show Animation Skeleton'):Connect(function() table.insert(seen, 'grid') end)
             s['Show Animation Skeleton'] = not s['Show Animation Skeleton']
             task.wait()
             print(table.concat(seen, ','))"),
        "grid,changed Show Animation Skeleton"
    );
    assert_eq!(
        one("print(pcall(function() settings().Studio:GetPropertyChangedSignal('Nope') end))"),
        "false Nope is not a valid property name."
    );
}

#[test]
fn themes_match_studio() {
    assert_eq!(
        one("local s = settings().Studio local t = s:GetAvailableThemes() print(#t, t[1].Name, t[2].Name, t[1].ClassName, t[1].Parent, s.Theme == t[2])"),
        "2 Light Dark StudioTheme nil true"
    );
    // MainBackground: white in Light, (46, 46, 46)-ish greys in Dark.
    assert_eq!(
        one("local t = settings().Studio:GetAvailableThemes() print(t[1]:GetColor(Enum.StudioStyleGuideColor.MainBackground), t[1]:GetColor('MainBackground', 'Hover'))"),
        "1, 1, 1 1, 1, 1"
    );
    assert_eq!(
        one("print(pcall(function() return settings().Studio.Theme:GetColor('NotAColor') end))"),
        "false Unable to cast string to StudioStyleGuideColor"
    );
}

#[test]
fn studio_services_match_studio() {
    let lines = run(r#"
        local function show(...) print(...) end
        show(pcall(function() return game:GetService("ScriptContext"):SetTimeout(10) end))
        show(game:GetJobsInfo()[1][1], game:GetJobsInfo()[1][6])
        game:SetPlaceId(1234) show(game.PlaceId)
        local SPS = game:GetService("ScriptProfilerService")
        show(pcall(function() return SPS:DeserializeJSON("{}") end))
        show(pcall(function() return SPS:DeserializeJSON("nope") end))
        show(pcall(function() return SPS:ClientStart(nil) end))
        local IFS = game:GetService("InstanceFileSyncService")
        show(#IFS:GetAllInstances(), IFS:GetStatus(workspace), IFS:GetSyncedInstance("C:/nope.lua"))
        local PCS = game:GetService("PluginConnectionService")
        show(PCS:CanHaveConnectionType(Enum.PluginConnectionTargetType.Edit), PCS:CanHaveConnectionType(Enum.PluginConnectionTargetType.Test))
        local SDS = game:GetService("ScriptDebuggerService")
        show(#SDS:GetThreads(), pcall(function() return SDS:GetStackTrace(1) end))
        show(SDS:Evaluate("1+1").Type, pcall(function() return SDS:AddBreakpoint(workspace, { line = 3 }) end))
        show(SDS:RemoveBreakpoint(workspace, 4))
        show(game:GetService("StudioTestService").EditModeActive, game:GetService("StudioCaptureService"):CanCaptureScreenshot())
        show(pcall(function() game:GetService("Players"):Chat("hi") end))
        local DS = game:GetService("StudioDeviceSimulatorService")
        show(DS:GetDeviceAsync(), DS:GetOrientationAsync(), pcall(function() return DS:GetResolutionAsync() end))
        show(DS:GetDeviceInfoAsync("ps4").Name, pcall(function() return DS:GetDeviceInfoAsync("nope") end))
        local phone = DS:GetDeviceInfoAsync("iphone_13")
        show(#DS:GetDeviceListAsync(), phone.Width, phone.PixelDensity, phone.LandscapeKeyboardHeight, phone.DeviceForm)
        DS:SetDeviceAsync("ps4") show(DS:GetScalingModeAsync(), DS:GetResolutionAsync()) DS:StopSimulationAsync()
        local p = Instance.new("Part") p.Parent = workspace
        workspace:IKMoveTo(p, CFrame.new(5, 20, 0)) show(p.Position)
    "#);
    assert_eq!(
        lines,
        [
            "true",
            "name isRunning",
            "1234",
            "false Unsupported ScriptProfiler JSON version",
            "true",
            "false First argument to ScriptProfilerClientStart must be a valid Player",
            "0 Enum.InstanceFileSyncStatus.NotSynced nil",
            "false true",
            "0 false Invalid threadId",
            "Tuple false required field 'Line' missing",
            "false",
            "true false",
            "false No local Player to chat from",
            "default Enum.ScreenOrientation.LandscapeRight false StudioDeviceSimulatorService: no device is active — call SetDeviceAsync() first",
            "PS4 false StudioDeviceSimulatorService: device not found: nope",
            "43 844 460 176 Enum.DeviceForm.Phone",
            "Enum.DeviceSimulatorScalingMode.ScaleToPhysicalSize 1920, 1080",
            "5, 20, 0",
        ]
    );
}

#[test]
fn test_service_reports_like_studio() {
    let lines = run(r#"
        local TS = game:GetService("TestService")
        TS:Check(true, "probe check", workspace, 7)
        TS:Checkpoint("probe point", workspace, 8)
        TS:Done()
        TS:Error("probe error", workspace, 9)
        TS:Fail("probe fail", workspace, 10)
        TS:Message("probe message", workspace, 11)
        TS:Require(false, "probe require", workspace, 12)
        TS:Warn(false, "probe warn", workspace, 13)
        TS:Warn(true, "fine", workspace, 14)
        TS:Message("probe noline", workspace)
        TS:Message("probe nosource")
        print(TS.ErrorCount, TS.TestCount, TS.WarnCount)
        local Stats = game:GetService("Stats")
        print(Stats:GetMemoryUsageMbForTag(Enum.DeveloperMemoryTag.LuaHeap), Stats:GetTotalMemoryUsageMb() > 0)
        print(pcall(function() return Stats:GetMemoryCategoryNames() end))
        print(pcall(function() return TS:ListTests() end))
    "#);
    assert_eq!(
        lines,
        [
            "TestService.Workspace(8): checkpoint probe point",
            "TestService.Workspace(9): probe error",
            "TestService.Workspace(10): fatal probe fail",
            "TestService.Workspace(11): probe message",
            "TestService.Workspace(12): fatal probe require",
            "TestService.Workspace(13): probe warn",
            "TestService.Workspace(0): probe noline",
            "TestService: probe nosource",
            "3 6 1",
            "0 true",
            "false The current thread cannot call 'GetMemoryCategoryNames' (lacking capability InternalTest)",
            "false TestService:ListTests only works while playtesting (press Play/Run in Studio) or on a Team Test server -- not in Edit mode or the command bar. (TestService.Enabled is false here.)",
        ]
    );
}
