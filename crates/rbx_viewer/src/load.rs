//! Everything a frame is drawn from, read once from a place file: the scene,
//! the decals and textures painted on it, the place's `Lighting` and its local
//! lights. The windowed, offscreen and embedded paths all start here.

use std::path::Path;

use rbx_assets::AssetRef;
use rbx_dom::WeakDom;
use rbx_reflection::ReflectionDatabase;

use crate::assets;
use crate::lighting::{self, Lighting, LocalLight};
use crate::renderer::World;
use crate::scene::Scene;
use crate::textures::{self, Decor};

/// Reads `path` and parses it into a DOM tree, sniffing whether the bytes are
/// the XML place/model format or the binary one.
///
/// The one shared spot every entry point that opens a place file goes through,
/// so `rbxview`, `rbxstudio` and their embedders never re-implement the sniff.
pub fn read_place(path: &Path) -> Result<WeakDom, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("failed to read {path:?}: {err}"))?;
    if rbx_xml::is_xml(&bytes) {
        let text = std::str::from_utf8(&bytes)
            .map_err(|err| format!("{path:?} is not valid UTF-8 XML: {err}"))?;
        rbx_xml::deserialize(text).map_err(|err| format!("failed to parse {path:?}: {err}"))
    } else {
        rbx_binary::deserialize(&bytes).map_err(|err| format!("failed to parse {path:?}: {err}"))
    }
}

/// What a load is allowed to download, and the hour its sky is lit at.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Toggles {
    pub(crate) textures: bool,
    pub(crate) materials: bool,
    pub(crate) lights: bool,
    pub(crate) clock_time: Option<f32>,
}

/// A place file turned into the four pieces every render path needs.
///
/// Owning them together is what lets [`Loaded::world`] hand out a `World` whose
/// four borrows are guaranteed to come from the same file.
pub(crate) struct Loaded {
    scene: Scene,
    decor: Decor,
    lighting: Lighting,
    lights: Vec<LocalLight>,
    /// Asset-fetch/decode warnings collected while building this `Loaded`,
    /// still empty until [`Loaded::take_warnings`] drains them — see
    /// `Headless`, which accumulates them across reloads for the Output dock.
    warnings: Vec<String>,
}

impl Loaded {
    /// Parses `path`, builds the reflection database it needs and downloads
    /// everything it asks for. Blocks on the network: callers with a UI
    /// thread must run this before showing one.
    ///
    /// Callers that reload repeatedly (an embedder replaying script or
    /// Properties-panel edits) should keep the `ReflectionDatabase` this
    /// builds and call [`Loaded::from_dom`] directly instead: it parses the
    /// several-megabyte embedded API dump (see
    /// [`rbx_reflection::ReflectionDatabase::embedded`]), and re-parsing it on
    /// every edit is pure waste.
    pub(crate) fn read(path: &Path, toggles: Toggles) -> Result<Self, String> {
        let dom = read_place(path)?;
        let database = ReflectionDatabase::embedded();
        Self::from_dom(&dom, &database, toggles)
            .map_err(|err| format!("nothing to show in {path:?}: {err}"))
    }

    /// The same pipeline as [`Loaded::read`], starting from a DOM and a
    /// reflection database already in memory instead of a path — what the
    /// command bar's reload takes after a script mutates the tree, since
    /// re-serializing it to disk just to re-parse it would be wasted work.
    pub(crate) fn from_dom(
        dom: &WeakDom,
        database: &ReflectionDatabase,
        toggles: Toggles,
    ) -> Result<Self, String> {
        let mut scene = Scene::from_dom(dom, database).map_err(|err| err.to_string())?;
        let mut warnings = Vec::new();
        // File meshes first: a MeshPart that gets real geometry stops drawing the
        // box its decals would otherwise be projected onto.
        warnings.extend(resolve_file_meshes(&mut scene, toggles.textures));
        // Unions next, for the same reason: a recovered pre-CSG part replaces
        // the union's box before materials are joined to every part at once.
        warnings.extend(resolve_unions(&mut scene));
        warnings.extend(resolve_materials(&mut scene, toggles.materials));

        let (decor, decor_warnings) = decor(dom, database, &scene, toggles.textures);
        warnings.extend(decor_warnings);
        let lighting = Lighting::from_dom(dom, database, toggles.clock_time);
        let lights = local_lights(dom, database, &scene, toggles.lights);

        Ok(Loaded {
            scene,
            decor,
            lighting,
            lights,
            warnings,
        })
    }

    /// Drains the asset warnings this `Loaded` collected while it was built —
    /// see the struct's `warnings` field. Left empty by a second call: a
    /// caller that keeps several `Loaded`s alive (`Headless::reload` replaces
    /// its own) takes each exactly once.
    pub(crate) fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// The parts a single-instance Properties-panel edit patches in place —
    /// see `Headless::patch_instance`.
    pub(crate) fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    pub(crate) fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Replaces the constant lighting terms and local lights wholesale — what
    /// `Headless::update_lighting` recomputes off a mutated DOM without
    /// touching `self.scene`/`self.decor` at all.
    pub(crate) fn set_lighting(&mut self, lighting: Lighting, lights: Vec<LocalLight>) {
        self.lighting = lighting;
        self.lights = lights;
    }

    pub(crate) fn world(&self) -> World<'_> {
        World {
            scene: &self.scene,
            decor: &self.decor,
            lighting: &self.lighting,
            lights: &self.lights,
        }
    }
}

/// Collects the place's own `PointLight`s, `SpotLight`s and `SurfaceLight`s.
///
/// The scene's centre only matters to a place with more lights than the GPU
/// buffer holds, where it decides which of them are kept (see
/// [`lighting::local_lights`]).
fn local_lights(
    dom: &rbx_dom::WeakDom,
    database: &rbx_reflection::ReflectionDatabase,
    scene: &Scene,
    enabled: bool,
) -> Vec<LocalLight> {
    if !enabled {
        return Vec::new();
    }
    lighting::local_lights(dom, database, scene.bounds().center())
}

/// Downloads the texture packs of every `BasePart.Material` the scene uses.
///
/// Runs after [`resolve_file_meshes`], whose resolved instances carry a material
/// of their own. Nothing here can fail the run: with `--no-materials` or with no
/// network, a part keeps its colour and is drawn as plain plastic.
fn resolve_materials(scene: &mut Scene, enabled: bool) -> Vec<String> {
    let references = if enabled {
        scene.material_assets()
    } else {
        Vec::new()
    };
    let (images, warnings) = assets::load(&references);
    scene.resolve_materials(images);
    warnings
}

/// Works out what the DOM wants painted, then downloads it.
///
/// Nothing here can fail the run: with `--no-textures`, with no network, or
/// with an asset that refuses to decode, the viewer falls back to the plain
/// boxes it has always drawn.
fn decor(
    dom: &rbx_dom::WeakDom,
    database: &rbx_reflection::ReflectionDatabase,
    scene: &Scene,
    enabled: bool,
) -> (Decor, Vec<String>) {
    if !enabled {
        return (Decor::default(), Vec::new());
    }

    let plan = textures::plan(dom, database, &scene.placements());
    let references = plan.references();
    if references.is_empty() {
        return (Decor::default(), Vec::new());
    }

    let (images, warnings) = assets::load(&references);
    (Decor::assemble(plan, &images), warnings)
}

/// Downloads the geometry (and, unless `--no-textures`, the textures) every
/// `MeshPart`/file `SpecialMesh` in the scene needs, then joins them in place.
///
/// Mesh geometry always downloads. Nothing here can fail the run: an unresolved
/// instance simply keeps drawing the box `Scene::from_dom` already gave it.
/// Downloads the legacy union assets and swaps each union's box for the
/// original parts found inside. Nothing here can fail the run either.
fn resolve_unions(scene: &mut Scene) -> Vec<String> {
    let references: Vec<AssetRef> = scene
        .union_assets()
        .into_iter()
        .map(|(_, reference)| reference)
        .collect();
    if references.is_empty() {
        return Vec::new();
    }
    let (bytes, warnings) = assets::load_bytes(&references);
    scene.resolve_unions(bytes);
    warnings
}

fn resolve_file_meshes(scene: &mut Scene, textures_enabled: bool) -> Vec<String> {
    let (mesh_refs, texture_refs) = scene.file_mesh_assets();
    let (meshes, mut warnings) = assets::load_meshes(&mesh_refs);
    let images = if textures_enabled {
        let (images, texture_warnings) = assets::load(&texture_refs);
        warnings.extend(texture_warnings);
        images
    } else {
        std::collections::HashMap::new()
    };
    scene.resolve_file_meshes(meshes, images);
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data};

    /// A `Workspace` with one `Part` carrying a `Decal` whose `Texture` names a
    /// package `rbxasset://` never groups its content under — resolving it
    /// fails locally (`AssetError::UnknownNativePackage`, see
    /// `rbx_assets::native::package_candidates_for_path`) before any network
    /// call would be attempted, which is what keeps this test offline-safe.
    fn dom_with_unresolvable_decal() -> WeakDom {
        let mut dom = WeakDom::new();
        let workspace = Ref::new(9100);
        dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
        dom.set_parent(workspace, None);

        let part_ref = Ref::new(9101);
        let mut part = Instance::new(part_ref, "Part", "Part");
        part.properties_mut().insert(
            "size".to_string(),
            Variant::Vector3(Vector3Data {
                x: 4.0,
                y: 4.0,
                z: 4.0,
            }),
        );
        part.properties_mut().insert(
            "CFrame".to_string(),
            Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }),
        );
        dom.insert(part);
        dom.set_parent(part_ref, Some(workspace));

        let decal_ref = Ref::new(9102);
        let mut decal = Instance::new(decal_ref, "Decal", "Decal");
        let properties = decal.properties_mut();
        properties.insert(
            "Texture".to_string(),
            Variant::String("rbxasset://unknown-native-package/none.png".to_string()),
        );
        properties.insert("Face".to_string(), Variant::Enum(0));
        dom.insert(decal);
        dom.set_parent(decal_ref, Some(part_ref));

        dom
    }

    #[test]
    fn from_dom_surfaces_a_warning_for_a_decal_that_cannot_resolve() {
        let database = ReflectionDatabase::embedded();
        let dom = dom_with_unresolvable_decal();
        let toggles = Toggles {
            textures: true,
            materials: false,
            lights: false,
            clock_time: None,
        };

        let mut loaded =
            Loaded::from_dom(&dom, &database, toggles).expect("scene should still load");
        let warnings = loaded.take_warnings();

        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("unknown-native-package")),
            "expected a warning naming the failed asset, got {warnings:?}"
        );
        // A second drain finds nothing: a `Loaded` yields its warnings once.
        assert!(loaded.take_warnings().is_empty());
    }

    #[test]
    fn read_place_sniffs_xml_and_builds_a_part() {
        let xml = r#"<roblox version="4"><Item class="Workspace" referent="RBX0"><Properties><string name="Name">Workspace</string></Properties><Item class="Part" referent="RBX1"><Properties><string name="Name">P</string><Vector3 name="size"><X>4</X><Y>1</Y><Z>2</Z></Vector3><CoordinateFrame name="CFrame"><X>0</X><Y>0</Y><Z>0</Z><R00>1</R00><R01>0</R01><R02>0</R02><R10>0</R10><R11>1</R11><R12>0</R12><R20>0</R20><R21>0</R21><R22>1</R22></CoordinateFrame></Properties></Item></Item></roblox>"#;

        let dir = std::env::temp_dir();
        let path = dir.join("rbx_viewer_read_place_test.rbxlx");
        std::fs::write(&path, xml).expect("write temp fixture");

        let dom = read_place(&path).expect("xml place should parse");
        std::fs::remove_file(&path).ok();

        let workspace = dom.get(dom.root_refs()[0]).expect("root instance");
        assert_eq!(workspace.class(), "Workspace");
        let part = dom.get(workspace.children()[0]).expect("part instance");
        assert_eq!(part.class(), "Part");
        assert_eq!(part.name(), "P");
    }

    // Stands in for `Headless::reload`, which needs a GPU: this is the shared
    // pipeline it calls, so proving it reacts to a mutated DOM is proving the
    // reload path works without one.
    #[test]
    fn from_dom_reflects_a_property_mutated_after_the_file_was_read() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/tests/TestPlace.rbxl");
        let mut dom = read_place(&fixture).expect("fixture should parse");
        let toggles = Toggles {
            textures: false,
            materials: false,
            lights: false,
            clock_time: None,
        };

        let part = crate::scene::descendants(&dom)
            .find(|referent| {
                dom.get(*referent)
                    .is_some_and(|instance| instance.properties().contains_key("size"))
            })
            .expect("fixture should have a sized part");

        let database = ReflectionDatabase::embedded();
        let before = Loaded::from_dom(&dom, &database, toggles).expect("first load");
        let before_corners = before.world().scene.bounds().corners();

        // Grown far past whatever the fixture already spans, so the bounds
        // change is unambiguous however the part sat in the scene.
        dom.set_property(
            part,
            "size",
            rbx_dom::Variant::Vector3(rbx_dom::Vector3Data {
                x: 500.0,
                y: 500.0,
                z: 500.0,
            }),
        )
        .expect("the fixture part should still exist");

        let after = Loaded::from_dom(&dom, &database, toggles).expect("reload after mutation");
        let after_corners = after.world().scene.bounds().corners();

        assert_eq!(
            before.world().scene.parts().len(),
            after.world().scene.parts().len(),
            "mutating a property must not add or remove parts"
        );
        assert_ne!(before_corners, after_corners);
    }
}
