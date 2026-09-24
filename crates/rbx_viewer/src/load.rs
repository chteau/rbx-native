//! Everything a frame is drawn from, read once from a place file: the scene,
//! the decals and textures painted on it, the place's `Lighting` and its local
//! lights. The windowed, offscreen and embedded paths all start here.

mod fetcher;
mod resident;
mod resolve;

use std::path::Path;

use rbx_assets::AssetRef;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

pub(crate) use fetcher::Source;
pub(crate) use resident::{Answered, Resident};

use crate::fonts::Library;
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
    parse_place(&bytes, path)
}

/// [`read_place`] for bytes already in memory — what a browser, which has a
/// dropped file's contents but no path to read, starts from. `path` only
/// names the file in an error.
pub(crate) fn parse_place(bytes: &[u8], path: &Path) -> Result<WeakDom, String> {
    let database = ReflectionDatabase::shared();
    // Both parsers keep a property under whatever name the file used, and a
    // hand-written place may say `Color` or `Size` where Studio saves
    // `Color3uint8` and `size`. Renamed once here, so nothing that reads the
    // tree needs to know both — for a binary place by the names its
    // per-class property chunks carry, rather than by visiting every
    // instance.
    let mut dom = if rbx_xml::is_xml(bytes) {
        let text = std::str::from_utf8(bytes)
            .map_err(|err| format!("{path:?} is not valid UTF-8 XML: {err}"))?;
        let mut dom =
            rbx_xml::deserialize(text).map_err(|err| format!("failed to parse {path:?}: {err}"))?;
        database.normalize_names(&mut dom);
        dom
    } else {
        let (mut dom, names) = rbx_binary::deserialize_with_names(bytes)
            .map_err(|err| format!("failed to parse {path:?}: {err}"))?;
        database.normalize_spellings(
            &mut dom,
            names
                .iter()
                .map(|(class, property)| (class.as_str(), property.as_str())),
        );
        dom
    };
    // A parser builds the tree through the same `insert`/`set_parent` an edit
    // uses, so the DOM comes back with a change log of its own construction
    // — one entry per instance in the file. The log is what happened *since*
    // the tree stood, and to a caller feeding it to `Headless::apply_changes`
    // a whole place's worth of "new" instances would be an edit of the whole
    // place; it starts empty here instead.
    dom.take_changes();
    Ok(dom)
}

/// What a load is allowed to download, the hour its sky is lit at, and the
/// one place property a command line may force.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Toggles {
    pub(crate) textures: bool,
    pub(crate) materials: bool,
    pub(crate) lights: bool,
    pub(crate) clock_time: Option<f32>,
    /// Force every `StarterGui.ShowDevelopmentGui` on — see
    /// [`show_development_gui`]. Only [`Loaded::read`] can honour it: it is
    /// an edit of the tree, and every other entry point owns the DOM it
    /// hands in and shows the place as saved.
    pub(crate) show_development_gui: bool,
}

/// Turns `ShowDevelopmentGui` on for every `StarterGui` in `dom`, so a place
/// that saved the flag off still shows its screens — what `rbxview`'s
/// `--show-development-gui` asks for, since the flag is a Studio view toggle
/// and a screenshot is often wanted of what a player would see.
pub(crate) fn show_development_gui(dom: &mut WeakDom) {
    // A service is never subclassed, so the exact class name is the whole
    // test and no reflection database is needed here. Collected first
    // because the walk borrows the DOM the writes need.
    let services: Vec<Ref> = crate::scene::descendants(dom)
        .filter(|&referent| {
            dom.get(referent)
                .is_some_and(|instance| instance.class() == "StarterGui")
        })
        .collect();
    for service in services {
        let _ = dom.set_property(service, "ShowDevelopmentGui", rbx_dom::Variant::Bool(true));
    }
}

/// A place file turned into the four pieces every render path needs.
///
/// Owning them together is what lets [`Loaded::world`] hand out a `World` whose
/// four borrows are guaranteed to come from the same file.
///
/// It also owns the *plans* those pieces were joined from, not just the
/// result. A streaming loader (see [`Resident`]) answers a first build with
/// whatever happened to be decoded already and fetches the rest in the
/// background, so the join has to be redoable — and redoable without the DOM,
/// which is sixty milliseconds to clone on a real place and is gone by the
/// time an asset lands. See [`Loaded::resolve`].
pub(crate) struct Loaded {
    scene: Scene,
    decor: Decor,
    /// What the DOM asked to be painted: the `Decal`/`Texture` faces, the six
    /// sky panels, the sun and the moon. Kept so a later landing can be joined
    /// to it again.
    decor_plan: textures::Plan,
    lighting: Lighting,
    lights: Vec<LocalLight>,
    /// Every image the renderer's own passes fetch for themselves —
    /// `ParticleEmitter`, `Beam` and `Trail` textures and the GUI atlas — as
    /// far as the loader has an answer for them. They are asked for here
    /// rather than inside those passes so that no pass ever resolves an asset
    /// on the thread that draws.
    images: Answered,
    /// Every font face the GUI trees' text asked for, as far as the loader
    /// has answered: a two-stage fetch (the family's JSON, then the face file
    /// it names) that the renderer's typesetter reads its faces out of.
    fonts: Library,
    toggles: Toggles,
    /// Every reference this place has asked the loader for. What
    /// `Headless` checks a landing against before re-resolving anything: a
    /// result for an asset the place stopped naming is filed and ignored.
    wanted: Vec<AssetRef>,
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
        let mut dom = read_place(path)?;
        if toggles.show_development_gui {
            show_development_gui(&mut dom);
        }
        let database = ReflectionDatabase::embedded();
        Self::from_dom(&dom, &database, toggles, &mut Resident::default())
            .map_err(|err| format!("nothing to show in {path:?}: {err}"))
    }

    /// The same pipeline as [`Loaded::read`], starting from a DOM and a
    /// reflection database already in memory instead of a path — what the
    /// command bar's reload takes after a script mutates the tree, since
    /// re-serializing it to disk just to re-parse it would be wasted work.
    ///
    /// `resident` is where every asset this decodes stays: hand the same one
    /// to every reload of the same place and only an asset the place never
    /// showed before — or whose fetch failed for a reason that may since
    /// have passed — is fetched and decoded again; see [`Resident`]. A
    /// streaming one makes this return without waiting for any of them; see
    /// [`Loaded::resolve`] for what finishes the job afterwards.
    pub(crate) fn from_dom(
        dom: &WeakDom,
        database: &ReflectionDatabase,
        toggles: Toggles,
        resident: &mut Resident,
    ) -> Result<Self, String> {
        let scene = Scene::from_dom(dom, database).map_err(|err| err.to_string())?;
        // Here and not deeper: one load is the unit a transient failure is
        // retried per, and every pass below asks through the same `resident`.
        resident.forget_failures();
        // Planned against the parts as built, before any mesh has suppressed
        // one: a face on a part a mesh later replaces is dropped at assembly
        // instead (see `Decor::assemble`), which is the same set of faces a
        // cold load with every asset resident ends up with.
        let decor_plan = if toggles.textures {
            textures::plan(dom, database, &scene.placements())
        } else {
            textures::Plan::default()
        };

        let mut loaded = Loaded {
            scene,
            decor: Decor::default(),
            decor_plan,
            lighting: Lighting::from_dom(dom, database, toggles.clock_time),
            lights: Vec::new(),
            images: Answered::default(),
            fonts: Library::default(),
            toggles,
            wanted: Vec::new(),
            warnings: Vec::new(),
        };
        loaded.lights = local_lights(dom, database, &loaded.scene, toggles.lights);
        loaded.warnings = loaded.resolve(resident);
        Ok(loaded)
    }

    /// Drains the asset warnings this `Loaded` collected while it was built —
    /// see the struct's `warnings` field. Left empty by a second call: a
    /// caller that keeps several `Loaded`s alive (`Headless::reload` replaces
    /// its own) takes each exactly once.
    pub(crate) fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Whether any of `references` is something this place actually draws
    /// through.
    ///
    /// The whole of the "an asset landed — is it worth re-resolving?"
    /// decision. A background fetch cannot be cancelled, so an edit that
    /// moves on (a `MeshId` typed, corrected and typed again) leaves results
    /// arriving for references nothing names any more; answering `false` for
    /// those is what keeps them from costing a rebuild, and keeps a stale
    /// mesh from being uploaded for an instance that no longer wants it.
    pub(crate) fn wants_any(&self, references: &[AssetRef]) -> bool {
        references
            .iter()
            .any(|reference| self.wanted.contains(reference))
    }

    /// Records that the place is now also waiting on `references` — what a
    /// single-instance edit adds when it names an asset the scene has never
    /// asked for, so [`Loaded::wants_any`] recognises the landing when it
    /// comes.
    pub(crate) fn also_wants(&mut self, references: &[AssetRef]) {
        for reference in references {
            if !self.wanted.contains(reference) {
                self.wanted.push(reference.clone());
            }
        }
    }

    /// The parts an edit patches in place — see `Headless::apply_changes`.
    pub(crate) fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    pub(crate) fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Re-reads every `Decal`/`Texture` hanging off `part` into the decor
    /// plan, against the placement this scene now draws that part at —
    /// keeping the plan in step the way `Renderer::sync_part` keeps the GPU
    /// in step.
    ///
    /// Owed by both halves of an edit that reaches a face: the part moving
    /// under it, and the face's own properties changing (a new image,
    /// another `Face`, a different tint, one added or taken away). The
    /// renderer is patched with the face there and then, but the *plan* is
    /// what every later asset landing re-assembles the decals from, so a
    /// plan left as the file was read would quietly undo the edit the moment
    /// anything else lands — including the very image the edit just asked
    /// for. A part this scene draws no box for has nothing to project onto
    /// and keeps whatever the plan holds, exactly as `Decor::assemble`
    /// already drops such a face.
    pub(crate) fn replan_faces(&mut self, dom: &WeakDom, database: &ReflectionDatabase, part: Ref) {
        if !self.toggles.textures {
            return;
        }
        let Some(placement) = self.scene.placement_of(part) else {
            return;
        };
        let faces = textures::faces(dom, database, part, &placement);
        self.decor_plan.replace_faces(part, faces);
    }

    /// Replaces the constant lighting terms wholesale — what
    /// `Headless::apply_changes` recomputes off a mutated DOM for a
    /// `Lighting` edit, without touching `self.scene`/`self.decor` at all.
    pub(crate) fn set_lighting(&mut self, lighting: Lighting) {
        self.lighting = lighting;
    }

    pub(crate) fn lights(&self) -> &[LocalLight] {
        &self.lights
    }

    /// Replaces the local lights wholesale — the same, for a `Light` (or the
    /// part one hangs off) that changed.
    pub(crate) fn set_lights(&mut self, lights: Vec<LocalLight>) {
        self.lights = lights;
    }

    pub(crate) fn world(&self) -> World<'_> {
        World {
            scene: &self.scene,
            decor: &self.decor,
            lighting: &self.lighting,
            lights: &self.lights,
            images: &self.images,
            fonts: &self.fonts,
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

#[cfg(test)]
#[path = "load/tests.rs"]
mod tests;
