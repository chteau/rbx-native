//! The places Home's New tab creates. Values were measured from real
//! Studio's own templates (2026-10-13).

use std::path::Path;

use rbx_dom::{CFrameData, Color3Data, Content, Ref, Variant, Vector3Data, WeakDom};
use rbx_terrain::edit::region::StudBox;

use crate::save::{self, Format};

/// Home's New tab: the plain starting places of real Studio's own New
/// list. Its themed ones (Village, Castle, Obby, …) are whole games built
/// from Roblox's assets, not something to redraw here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Template {
    Baseplate,
    ClassicBaseplate,
    FlatTerrain,
}

impl Template {
    /// In the order Home shows them.
    pub(crate) const ALL: [Template; 3] = [
        Template::Baseplate,
        Template::ClassicBaseplate,
        Template::FlatTerrain,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Template::Baseplate => "Baseplate",
            Template::ClassicBaseplate => "Classic Baseplate",
            Template::FlatTerrain => "Flat Terrain",
        }
    }

    /// Home's one-line description under the name.
    pub(crate) fn blurb(self) -> &'static str {
        match self {
            Template::Baseplate => "Empty place with a spawn",
            Template::ClassicBaseplate => "The studded grey floor of old",
            Template::FlatTerrain => "A plain of grass terrain",
        }
    }

    /// Writes a new binary place at `path`. Refuses to overwrite a file.
    pub(crate) fn create(self, path: &Path) -> Result<(), String> {
        if path.exists() {
            return Err(format!("{} already exists", path.display()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        save::save(&self.dom(), Format::Binary, path)
    }

    fn dom(self) -> WeakDom {
        let mut dom = WeakDom::new();
        // The services a fresh Studio place shows in the Explorer; a place
        // file's roots are its services, with no DataModel above them.
        let service = |dom: &mut WeakDom, class| dom.new_instance(class, class, None);
        let workspace = service(&mut dom, "Workspace");
        let mut lighting = None;
        for class in [
            "Players",
            "Lighting",
            "ReplicatedFirst",
            "ReplicatedStorage",
            "ServerScriptService",
            "ServerStorage",
            "StarterGui",
            "StarterPack",
            "Teams",
            "SoundService",
            "TextChatService",
        ] {
            let made = service(&mut dom, class);
            if class == "Lighting" {
                lighting = Some(made);
            }
        }
        let lighting = lighting.expect("Lighting is in the list");
        let player = service(&mut dom, "StarterPlayer");
        dom.new_instance("StarterPlayerScripts", "StarterPlayerScripts", Some(player));
        dom.new_instance(
            "StarterCharacterScripts",
            "StarterCharacterScripts",
            Some(player),
        );
        let camera = dom.new_instance("Camera", "Camera", Some(workspace));
        set(&mut dom, workspace, "CurrentCamera", Variant::Ref(camera));
        let (eye, focus) = self.camera();
        set(&mut dom, camera, "CFrame", Variant::CFrame(eye));
        set(&mut dom, camera, "Focus", focus);
        set(&mut dom, camera, "FieldOfView", float(70.0));
        let terrain = dom.new_instance("Terrain", "Terrain", Some(workspace));

        match self {
            Template::Baseplate => {
                modern_workspace(&mut dom, workspace);
                modern_lighting(&mut dom, lighting);
                set(&mut dom, player, "CameraMaxZoomDistance", float(128.0));
                set(&mut dom, player, "CameraMinZoomDistance", float(0.5));
            }
            Template::ClassicBaseplate => {
                classic_workspace(&mut dom, workspace);
                classic_lighting(&mut dom, lighting);
                set(&mut dom, player, "CameraMaxZoomDistance", float(128.0));
            }
            Template::FlatTerrain => {
                grass_plain(&mut dom, terrain);
                modern_lighting(&mut dom, lighting);
                set(&mut dom, player, "CameraMaxZoomDistance", float(400.0));
            }
        }
        dom
    }

    /// Where the template's camera opens, and its focus: each template
    /// saves its own, read off real Studio's `CurrentCamera`.
    fn camera(self) -> (CFrameData, Variant) {
        let (eye, focus) = match self {
            Template::Baseplate => (
                [
                    -19.696_095,
                    14.091_624,
                    -19.303_886,
                    -0.706_150_9,
                    0.312_965_84,
                    -0.635_140_5,
                    0.0,
                    0.897_013_4,
                    0.442_003_8,
                    0.708_061_5,
                    0.312_121_36,
                    -0.633_426_7,
                ],
                [-18.425_814, 13.207_617, -18.037_033],
            ),
            Template::ClassicBaseplate => (
                [
                    -30.734_314,
                    16.456_863,
                    14.594_533,
                    0.714_467_9,
                    0.195_197_8,
                    -0.671_888,
                    0.0,
                    0.960_295_1,
                    0.278_986_28,
                    0.699_668_2,
                    -0.199_326_75,
                    0.686_100_07,
                ],
                [-29.390_537, 15.898_891, 13.222_333],
            ),
            Template::FlatTerrain => (
                [
                    58.453_537,
                    31.631_642,
                    -219.402_5,
                    -0.112_963_67,
                    0.267_421_66,
                    -0.956_935_2,
                    0.0,
                    0.963_1,
                    0.269_144_42,
                    0.993_599_1,
                    0.030_403_541,
                    -0.108_795_31,
                ],
                [60.367_41, 31.093_353, -219.184_9],
            ),
        };
        let [x, y, z, rotation @ ..] = eye;
        let eye = CFrameData {
            position: Vector3Data { x, y, z },
            rotation,
        };
        (eye, at(focus[0], focus[1], focus[2]))
    }
}

fn set(dom: &mut WeakDom, r: Ref, key: &str, value: Variant) {
    dom.set_property(r, key, value).expect("instance exists");
}

fn child(dom: &mut WeakDom, parent: Ref, class: &str, name: &str) -> Ref {
    dom.new_instance(class, name, Some(parent))
}

fn float(value: f32) -> Variant {
    Variant::Float32(value)
}

fn boolean(value: bool) -> Variant {
    Variant::Bool(value)
}

fn enumeration(value: u32) -> Variant {
    Variant::Enum(value)
}

fn content(uri: &str) -> Variant {
    Variant::Content(Content::Uri(uri.to_string()))
}

fn color3(r: f32, g: f32, b: f32) -> Variant {
    Variant::Color3(Color3Data { r, g, b })
}

fn grey(value: f32) -> Variant {
    color3(value, value, value)
}

fn vector3(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

fn at(x: f32, y: f32, z: f32) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data { x, y, z },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    })
}

fn rgb8(r: u8, g: u8, b: u8) -> Variant {
    Variant::Color3uint8 { r, g, b }
}

fn modern_workspace(dom: &mut WeakDom, workspace: Ref) {
    let baseplate = child(dom, workspace, "Part", "Baseplate");
    set(dom, baseplate, "Anchored", boolean(true));
    set(dom, baseplate, "Locked", boolean(true));
    set(dom, baseplate, "size", vector3(2048.0, 16.0, 2048.0));
    set(dom, baseplate, "CFrame", at(0.0, -8.0, 0.0));
    set(dom, baseplate, "Color3uint8", rgb8(91, 91, 91));
    let texture = child(dom, baseplate, "Texture", "Texture");
    set(dom, texture, "Texture", content("rbxassetid://6372755229"));
    set(dom, texture, "Face", enumeration(1));
    set(dom, texture, "Transparency", float(0.8));
    set(dom, texture, "StudsPerTileU", float(8.0));
    set(dom, texture, "StudsPerTileV", float(8.0));
    set(dom, texture, "Color3", grey(0.0));

    let spawn = child(dom, workspace, "SpawnLocation", "SpawnLocation");
    set(dom, spawn, "Anchored", boolean(true));
    set(dom, spawn, "size", vector3(12.0, 1.0, 12.0));
    set(dom, spawn, "CFrame", at(0.0, 0.5, 0.0));
    set(dom, spawn, "Color3uint8", rgb8(163, 162, 165));
    let decal = child(dom, spawn, "Decal", "Decal");
    set(
        dom,
        decal,
        "Texture",
        content("rbxasset://textures/SpawnLocation.png"),
    );
    set(dom, decal, "Face", enumeration(1));
}

fn classic_workspace(dom: &mut WeakDom, workspace: Ref) {
    let baseplate = child(dom, workspace, "Part", "Baseplate");
    set(dom, baseplate, "Anchored", boolean(true));
    set(dom, baseplate, "Locked", boolean(true));
    set(dom, baseplate, "size", vector3(512.0, 20.0, 512.0));
    set(dom, baseplate, "CFrame", at(0.0, -10.0, 0.0));
    set(dom, baseplate, "Color3uint8", rgb8(99, 95, 98));
    set(dom, baseplate, "TopSurface", enumeration(3));
    set(dom, baseplate, "BottomSurface", enumeration(4));
}

fn modern_lighting(dom: &mut WeakDom, lighting: Ref) {
    set(dom, lighting, "Ambient", grey(70.0 / 255.0));
    set(dom, lighting, "OutdoorAmbient", grey(70.0 / 255.0));
    set(dom, lighting, "Brightness", float(3.0));
    set(
        dom,
        lighting,
        "TimeOfDay",
        Variant::String("14:30:00".to_string()),
    );
    set(dom, lighting, "GeographicLatitude", float(0.0));
    set(dom, lighting, "EnvironmentDiffuseScale", float(1.0));
    set(dom, lighting, "EnvironmentSpecularScale", float(1.0));
    set(dom, lighting, "GlobalShadows", boolean(true));
    set(dom, lighting, "ShadowSoftness", float(0.2));
    set(dom, lighting, "FogEnd", float(100000.0));

    let sky = child(dom, lighting, "Sky", "Sky");
    for key in ["SkyboxBk", "SkyboxFt", "SkyboxLf", "SkyboxRt"] {
        set(dom, sky, key, content("rbxassetid://6444884337"));
    }
    set(dom, sky, "SkyboxDn", content("rbxassetid://6444884785"));
    set(dom, sky, "SkyboxUp", content("rbxassetid://6412503613"));
    set(dom, sky, "SunTextureId", content("rbxassetid://6196665106"));
    set(
        dom,
        sky,
        "MoonTextureId",
        content("rbxassetid://6444320592"),
    );
    set(dom, sky, "StarCount", Variant::Int32(3000));
    set(dom, sky, "SunAngularSize", float(11.0));
    set(dom, sky, "MoonAngularSize", float(11.0));

    let rays = child(dom, lighting, "SunRaysEffect", "SunRays");
    set(dom, rays, "Intensity", float(0.01));
    set(dom, rays, "Spread", float(0.1));

    let atmosphere = child(dom, lighting, "Atmosphere", "Atmosphere");
    set(dom, atmosphere, "Color", grey(0.780392));
    set(
        dom,
        atmosphere,
        "Decay",
        color3(0.415686, 0.439216, 0.490196),
    );
    set(dom, atmosphere, "Density", float(0.3));
    set(dom, atmosphere, "Offset", float(0.25));
    set(dom, atmosphere, "Glare", float(0.0));
    set(dom, atmosphere, "Haze", float(0.0));

    let bloom = child(dom, lighting, "BloomEffect", "Bloom");
    set(dom, bloom, "Size", float(24.0));
    set(dom, bloom, "Intensity", float(1.0));
    set(dom, bloom, "Threshold", float(2.0));

    let depth = child(dom, lighting, "DepthOfFieldEffect", "DepthOfField");
    set(dom, depth, "Enabled", boolean(false));
}

fn classic_lighting(dom: &mut WeakDom, lighting: Ref) {
    set(dom, lighting, "Ambient", grey(138.0 / 255.0));
    set(dom, lighting, "OutdoorAmbient", grey(128.0 / 255.0));
    set(dom, lighting, "Brightness", float(2.0));
    set(
        dom,
        lighting,
        "TimeOfDay",
        Variant::String("14:00:00".to_string()),
    );
    set(dom, lighting, "GeographicLatitude", float(41.7333));
    set(dom, lighting, "EnvironmentDiffuseScale", float(0.0));
    set(dom, lighting, "EnvironmentSpecularScale", float(0.0));
}

/// Grass 2048 × 2048 studs about the origin, 64 thick with its top at y = 8.
fn grass_plain(dom: &mut WeakDom, terrain: Ref) {
    let mut grid = rbx_terrain::VoxelGrid::new();
    rbx_terrain::edit::region::fill(
        &mut grid,
        &StudBox::from_center_size([0.0, -24.0, 0.0], [2048.0, 64.0, 2048.0]),
        rbx_terrain::Material::Grass,
    );
    crate::terrain::write_encoded(
        dom,
        terrain,
        rbx_terrain::smooth_grid::encode(&grid),
        Some(rbx_terrain::physics_grid::encode(&grid)),
    )
    .expect("terrain exists");
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn read(template: Template) -> WeakDom {
        // Tests run in parallel and several read the same template.
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = template.name().replace(' ', "_");
        let path =
            std::env::temp_dir().join(format!("rbx_home_{name}_{}_{n}.rbxl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        template.create(&path).unwrap();
        assert!(template.create(&path).is_err(), "never overwrites");
        let dom = rbx_viewer::read_place(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        dom
    }

    #[test]
    fn every_template_opens_on_studios_own_camera() {
        for template in Template::ALL {
            let camera = crate::camera::PlaceCamera::from_dom(&read(template))
                .unwrap_or_else(|| panic!("{} has no CurrentCamera", template.name()));
            let (eye, _) = template.camera();
            assert_eq!(camera.eye, [eye.position.x, eye.position.y, eye.position.z]);
            assert_eq!(camera.fov_degrees, 70.0);
        }
    }

    fn service(dom: &WeakDom, class: &str) -> Ref {
        dom.root_refs()
            .iter()
            .copied()
            .find(|&r| dom.get(r).unwrap().class() == class)
            .unwrap()
    }

    fn children(dom: &WeakDom, parent: Ref) -> Vec<(String, String)> {
        dom.get(parent)
            .unwrap()
            .children()
            .iter()
            .map(|&r| {
                let inst = dom.get(r).unwrap();
                (inst.class().to_string(), inst.name().to_string())
            })
            .collect()
    }

    fn named(dom: &WeakDom, parent: Ref, name: &str) -> Option<Ref> {
        dom.get(parent)
            .unwrap()
            .children()
            .iter()
            .copied()
            .find(|&r| dom.get(r).unwrap().name() == name)
    }

    #[test]
    fn baseplate_has_a_textured_floor_and_a_spawn() {
        let dom = read(Template::Baseplate);
        let workspace = service(&dom, "Workspace");
        let floor = named(&dom, workspace, "Baseplate").unwrap();
        assert!(named(&dom, workspace, "SpawnLocation").is_some());
        assert!(named(&dom, floor, "Texture").is_some());
        let lighting = service(&dom, "Lighting");
        assert!(children(&dom, lighting).iter().any(|(c, _)| c == "Sky"));
    }

    #[test]
    fn classic_baseplate_is_studded_with_no_spawn() {
        let dom = read(Template::ClassicBaseplate);
        let workspace = service(&dom, "Workspace");
        let floor = named(&dom, workspace, "Baseplate").unwrap();
        assert_eq!(
            dom.get(floor).unwrap().properties().get("TopSurface"),
            Some(&Variant::Enum(3))
        );
        assert!(named(&dom, workspace, "SpawnLocation").is_none());
        let lighting = service(&dom, "Lighting");
        assert!(!children(&dom, lighting).iter().any(|(c, _)| c == "Sky"));
    }

    #[test]
    fn flat_terrain_is_grass_up_to_y_eight() {
        let dom = read(Template::FlatTerrain);
        let workspace = service(&dom, "Workspace");
        assert!(!children(&dom, workspace).iter().any(|(c, _)| c == "Part"));
        let terrain = named(&dom, workspace, "Terrain").unwrap();
        let Some(Variant::Unknown { raw, .. }) =
            dom.get(terrain).unwrap().properties().get("SmoothGrid")
        else {
            panic!("SmoothGrid is raw bytes");
        };
        let grid = rbx_terrain::smooth_grid::decode(raw.as_slice()).unwrap();
        assert_eq!(grid.get([0, 1, 0]).material, rbx_terrain::Material::Grass);
        assert!(grid.get([0, 3, 0]).is_air());
    }
}
