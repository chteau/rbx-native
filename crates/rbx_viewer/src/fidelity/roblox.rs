//! The place the real client draws the scenes in: every scene's world and
//! `Lighting` children stored under `ReplicatedStorage.Fidelity`, and a
//! client script that shows them one at a time.
//!
//! Per scene the script empties `Lighting`, sets every scriptable `Lighting`
//! property to its class default and then to the scene's values, clones the
//! scene in, holds the camera (Scriptable, `CFrame.lookAt`, the field of
//! view), waits for the picture to settle and prints [`READY`] with the
//! scene's name and `GetSunDirection`/`GetMoonDirection`, which the recorder
//! answers with a capture.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::Scene;

pub const READY: &str = "FIDELITY_READY ";
pub const QUALITY: &str = "FIDELITY_QUALITY ";
pub const DONE: &str = "FIDELITY_DONE";
/// Seconds a scene stands before [`READY`] (shadow maps, voxel lighting and
/// the sky all catch up over a few frames), and after it for the capture.
const SETTLE: f32 = 4.0;
const HOLD: f32 = 2.5;

/// The `Lighting` properties a script may set, reset to their class
/// defaults before every scene.
const SCRIPTABLE: [&str; 15] = [
    "Ambient",
    "Brightness",
    "ColorShift_Bottom",
    "ColorShift_Top",
    "EnvironmentDiffuseScale",
    "EnvironmentSpecularScale",
    "ExposureCompensation",
    "FogColor",
    "FogEnd",
    "FogStart",
    "GeographicLatitude",
    "GlobalShadows",
    "OutdoorAmbient",
    "ShadowSoftness",
    "TimeOfDay",
];

/// The `Lighting` properties no game script may write (`Technology` is
/// Roblox-only, the other two want the plugin capability): they come from
/// the place file, so scenes that differ in them are published apart.
pub const PLACE_LEVEL: [&str; 3] = ["Technology", "LightingStyle", "PrioritizeLightingQuality"];

fn place_level(scene: &Scene) -> Vec<(&'static str, Variant)> {
    scene
        .lighting
        .iter()
        .filter(|(key, _)| PLACE_LEVEL.contains(key))
        .cloned()
        .collect()
}

/// `scenes` split into the runs one place can show: alike in their
/// [`PLACE_LEVEL`] properties, in the order first seen.
pub fn groups(scenes: Vec<Scene>) -> Vec<Vec<Scene>> {
    type Key = Vec<(&'static str, Variant)>;
    let mut groups: Vec<(Key, Vec<Scene>)> = Vec::new();
    for scene in scenes {
        let key = place_level(&scene);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.push(scene),
            None => groups.push((key, vec![scene])),
        }
    }
    groups.into_iter().map(|(_, group)| group).collect()
}

/// The place to publish: `scenes` (one of [`groups`]) under
/// `ReplicatedStorage.Fidelity`, `Lighting` holding their [`PLACE_LEVEL`]
/// values, no characters, and the client script in `ReplicatedFirst`.
pub fn place(scenes: &[Scene], database: &ReflectionDatabase) -> WeakDom {
    let mut dom = WeakDom::new();
    let service = |dom: &mut WeakDom, class: &str| dom.new_instance(class, class, None);
    let workspace = service(&mut dom, "Workspace");
    let camera = dom.new_instance("Camera", "Camera", Some(workspace));
    set(&mut dom, workspace, "CurrentCamera", Variant::Ref(camera));
    dom.new_instance("Terrain", "Terrain", Some(workspace));
    let players = service(&mut dom, "Players");
    set(
        &mut dom,
        players,
        "CharacterAutoLoads",
        Variant::Bool(false),
    );
    let lighting = service(&mut dom, "Lighting");
    for (key, value) in scenes.first().map(place_level).unwrap_or_default() {
        set(&mut dom, lighting, key, value);
    }
    for class in [
        "ServerScriptService",
        "ServerStorage",
        "StarterGui",
        "StarterPack",
        "SoundService",
    ] {
        service(&mut dom, class);
    }
    let player = service(&mut dom, "StarterPlayer");
    dom.new_instance("StarterPlayerScripts", "StarterPlayerScripts", Some(player));
    let storage = service(&mut dom, "ReplicatedStorage");
    let root = dom.new_instance("Folder", "Fidelity", Some(storage));
    for scene in scenes {
        let folder = dom.new_instance("Folder", scene.name, Some(root));
        let world = dom.new_instance("Model", "World", Some(folder));
        let lighting = dom.new_instance("Folder", "Lighting", Some(folder));
        let gui = dom.new_instance("Folder", "Gui", Some(folder));
        let materials = dom.new_instance("Folder", "Materials", Some(folder));
        dom = scene.build_into(dom, [world, lighting, gui, materials]);
    }
    let first = service(&mut dom, "ReplicatedFirst");
    let script = dom.new_instance("LocalScript", "Fidelity", Some(first));
    set(
        &mut dom,
        script,
        "Source",
        Variant::String(source(scenes, database)),
    );
    dom
}

fn set(dom: &mut WeakDom, instance: Ref, key: &str, value: Variant) {
    dom.set_property(instance, key, value)
        .expect("the instance exists");
}

/// A Luau literal for `value`, for the property types scenes set.
fn literal(value: &Variant) -> Option<String> {
    Some(match value {
        Variant::Bool(b) => b.to_string(),
        Variant::Float32(f) => format!("{f}"),
        Variant::Float64(f) => format!("{f}"),
        Variant::Int32(i) => i.to_string(),
        Variant::Enum(e) => e.to_string(),
        Variant::String(s) => format!("{s:?}"),
        Variant::Color3(c) => format!("Color3.new({}, {}, {})", c.r, c.g, c.b),
        Variant::Color3uint8 { r, g, b } => format!("Color3.fromRGB({r}, {g}, {b})"),
        Variant::Vector3(v) => format!("Vector3.new({}, {}, {})", v.x, v.y, v.z),
        _ => return None,
    })
}

fn pairs<'a>(values: impl IntoIterator<Item = (&'a str, &'a Variant)>) -> String {
    values
        .into_iter()
        .filter_map(|(key, value)| Some(format!("{{{key:?}, {}}}", literal(value)?)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn source(scenes: &[Scene], database: &ReflectionDatabase) -> String {
    let defaults = pairs(
        SCRIPTABLE
            .iter()
            .filter_map(|key| Some((*key, database.stored_default("Lighting", key)?))),
    );
    let scenes = scenes
        .iter()
        .map(|scene| {
            let v = |[x, y, z]: [f32; 3]| format!("Vector3.new({x}, {y}, {z})");
            format!(
                "\t{{ name = {:?}, eye = {}, at = {}, fov = {}, lighting = {{ {} }} }},",
                scene.name,
                v(scene.eye),
                v(scene.look_at),
                scene.fov,
                pairs(
                    scene
                        .lighting
                        .iter()
                        .filter(|(k, _)| !PLACE_LEVEL.contains(k))
                        .map(|(k, v)| (*k, v))
                )
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"-- Generated by rbx_viewer::fidelity::roblox: shows each scene in turn.
local Lighting = game:GetService("Lighting")
local RunService = game:GetService("RunService")
local StarterGui = game:GetService("StarterGui")
game:GetService("ReplicatedFirst"):RemoveDefaultLoadingScreen()
if not game:IsLoaded() then
	game.Loaded:Wait()
end
local root = game:GetService("ReplicatedStorage"):WaitForChild("Fidelity")
local DEFAULTS = {{ {defaults} }}
local SCENES = {{
{scenes}
}}
local view = {{ eye = Vector3.new(0, 10, 10), at = Vector3.zero, fov = 70 }}
RunService:BindToRenderStep("Fidelity", Enum.RenderPriority.Last.Value, function()
	local camera = workspace.CurrentCamera
	camera.CameraType = Enum.CameraType.Scriptable
	camera.FieldOfView = view.fov
	camera.CFrame = CFrame.lookAt(view.eye, view.at)
end)
for _ = 1, 50 do
	if pcall(StarterGui.SetCoreGuiEnabled, StarterGui, Enum.CoreGuiType.All, false) then
		break
	end
	task.wait(0.2)
end
game:GetService("ContentProvider"):PreloadAsync({{ root }})
local quality = UserSettings():GetService("UserGameSettings").SavedQualityLevel
print("{QUALITY}" .. tostring(quality) .. " text " .. tostring(game:GetService("GuiService").PreferredTextSize))
local playerGui = game:GetService("Players").LocalPlayer:WaitForChild("PlayerGui")
local shown, screens, variants = nil, {{}}, {{}}
local MaterialService = game:GetService("MaterialService")
for _, scene in SCENES do
	if shown then
		shown:Destroy()
	end
	for _, variant in variants do
		variant:Destroy()
	end
	variants = {{}}
	for _, screen in screens do
		screen:Destroy()
	end
	screens = {{}}
	Lighting:ClearAllChildren()
	for _, pair in DEFAULTS do
		Lighting[pair[1]] = pair[2]
	end
	for _, pair in scene.lighting do
		Lighting[pair[1]] = pair[2]
	end
	local folder = root:WaitForChild(scene.name)
	for _, child in folder.Lighting:GetChildren() do
		child:Clone().Parent = Lighting
	end
	for _, child in folder.Materials:GetChildren() do
		local variant = child:Clone()
		variant.Parent = MaterialService
		table.insert(variants, variant)
	end
	shown = folder.World:Clone()
	shown.Parent = workspace
	for _, child in folder.Gui:GetChildren() do
		local screen = child:Clone()
		screen.Parent = playerGui
		table.insert(screens, screen)
	end
	view.eye, view.at, view.fov = scene.eye, scene.at, scene.fov
	task.wait({SETTLE})
	local sun, moon = Lighting:GetSunDirection(), Lighting:GetMoonDirection()
	print(string.format("{READY}%s %.5f %.5f %.5f %.5f %.5f %.5f", scene.name, sun.X, sun.Y, sun.Z, moon.X, moon.Y, moon.Z))
	task.wait({HOLD})
end
print("{DONE}")
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_is_stored_and_every_lighting_value_is_written() {
        let database = ReflectionDatabase::shared();
        let scenes = super::super::scenes();
        let dom = place(&scenes, database);
        let first = dom
            .root_refs()
            .iter()
            .filter_map(|&r| dom.get(r))
            .find(|i| i.class() == "ReplicatedFirst")
            .expect("ReplicatedFirst");
        let script = dom.get(first.children()[0]).expect("the client script");
        let Some(Variant::String(source)) = script.properties().get("Source") else {
            panic!("no source");
        };
        for scene in &scenes {
            assert!(source.contains(&format!("name = {:?}", scene.name)));
            for (key, value) in &scene.lighting {
                assert!(
                    literal(value).is_some(),
                    "{}: {key} has no literal",
                    scene.name
                );
            }
        }
        assert!(source.contains("{\"Ambient\", Color3.new(0.5, 0.5, 0.5)}"));
    }
}
