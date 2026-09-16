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

use crate::lighting::{self, Lighting, LocalLight};
use crate::renderer::World;
use crate::scene::{Placement, Scene};
use crate::textures::{self, Decor};

/// Reads `path` and parses it into a DOM tree, sniffing whether the bytes are
/// the XML place/model format or the binary one.
///
/// The one shared spot every entry point that opens a place file goes through,
/// so `rbxview`, `rbxstudio` and their embedders never re-implement the sniff.
pub fn read_place(path: &Path) -> Result<WeakDom, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("failed to read {path:?}: {err}"))?;
    let mut dom = if rbx_xml::is_xml(&bytes) {
        let text = std::str::from_utf8(&bytes)
            .map_err(|err| format!("{path:?} is not valid UTF-8 XML: {err}"))?;
        rbx_xml::deserialize(text).map_err(|err| format!("failed to parse {path:?}: {err}"))?
    } else {
        rbx_binary::deserialize(&bytes).map_err(|err| format!("failed to parse {path:?}: {err}"))?
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
        let dom = read_place(path)?;
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

    /// Re-projects one part's `Decal`/`Texture` faces after its placement
    /// changed, keeping the decor plan in step with the scene the way
    /// `Renderer::sync_part` keeps the GPU in step with it. Without this a
    /// later asset landing would re-assemble the decals at the placement the
    /// part had when the file was read.
    pub(crate) fn replan_faces(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Ref,
        placement: &Placement,
    ) {
        if !self.toggles.textures {
            return;
        }
        let faces = textures::faces(dom, database, referent, placement);
        self.decor_plan.replace_faces(referent, faces);
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
