//! Joining a place to the assets that have decoded so far — once when it is
//! read, and again every time the background pool lands more of them.
//!
//! The steps run in the order they do for the same reasons a single build
//! always ran them in it: a `MeshPart` that gets real geometry stops drawing
//! the box its decals would be projected onto, a recovered pre-CSG part
//! replaces a union's box before materials are joined to every part at once,
//! and the decals are assembled against whatever is still drawn once both are
//! done.
//!
//! What is new is that every step has to survive being run again over a
//! superset of the same assets and leave the place exactly as one run with all
//! of them would have. Each of the three is idempotent for its own reason —
//! see `Scene::resolve_file_meshes`, `Scene::resolve_unions` (which ignores a
//! union it has already merged, since it appends the parts it recovers) and
//! `scene::material::Catalog::resolve`.

use rbx_assets::AssetRef;

use super::{Loaded, Resident};
use crate::fonts::Family;
use crate::textures::Decor;

impl Loaded {
    /// Re-joins the place to everything `resident` has decoded, and asks it
    /// for whatever is still missing.
    ///
    /// Returns the warnings of assets that failed for good. Cheap to call
    /// with nothing new: the joins are a walk over plans that are already in
    /// memory, with no decode, no DOM and no network anywhere in them.
    pub(crate) fn resolve(&mut self, resident: &mut Resident) -> Vec<String> {
        let mut warnings = Vec::new();
        self.wanted.clear();

        warnings.extend(self.resolve_file_meshes(resident));
        warnings.extend(self.resolve_unions(resident));
        warnings.extend(self.resolve_materials(resident));
        warnings.extend(self.resolve_decor(resident));
        warnings.extend(self.resolve_effect_images(resident));
        self.resolve_fonts(resident);
        warnings
    }

    /// Downloads the geometry (and, unless `--no-textures`, the textures)
    /// every `MeshPart`/file `SpecialMesh` in the scene needs, then joins them
    /// in place.
    ///
    /// Mesh geometry always downloads. Nothing here can fail the run: an
    /// unresolved instance simply keeps drawing the box `Scene::from_dom`
    /// already gave it.
    fn resolve_file_meshes(&mut self, resident: &mut Resident) -> Vec<String> {
        let (mesh_refs, texture_refs) = self.scene.file_mesh_assets();
        self.want(&mesh_refs);
        let (meshes, mut warnings) = resident.meshes(&mesh_refs);
        let images = if self.toggles.textures {
            self.want(&texture_refs);
            let (images, texture_warnings) = resident.images(&texture_refs);
            warnings.extend(texture_warnings);
            images
        } else {
            std::collections::HashMap::new()
        };
        self.scene.resolve_file_meshes(meshes, images);
        warnings
    }

    /// Downloads the legacy union assets and swaps each union's box for the
    /// original parts found inside. Nothing here can fail the run either.
    fn resolve_unions(&mut self, resident: &mut Resident) -> Vec<String> {
        let references: Vec<AssetRef> = self
            .scene
            .union_assets()
            .into_iter()
            .map(|(_, reference)| reference)
            .collect();
        if references.is_empty() {
            return Vec::new();
        }
        self.want(&references);
        // Only what was never carved needs its bytes: a known asset resolves
        // out of the evaluation an earlier scene left behind (see
        // `scene::union::resolve`), so a tick that has nothing new to carve
        // copies nothing out of the resident table at all.
        let uncarved: Vec<AssetRef> = references
            .iter()
            .filter(|reference| !resident.unions.is_known(reference))
            .cloned()
            .collect();
        let (bytes, warnings) = resident.bytes(&uncarved);
        self.scene.resolve_unions(bytes, &mut resident.unions);
        warnings
    }

    /// Downloads the texture packs of every `BasePart.Material` the scene
    /// uses.
    ///
    /// Runs after the two above, whose resolved instances carry a material of
    /// their own. Nothing here can fail the run: with `--no-materials` or with
    /// no network, a part keeps its colour and is drawn as plain plastic.
    fn resolve_materials(&mut self, resident: &mut Resident) -> Vec<String> {
        let references = if self.toggles.materials {
            self.scene.material_assets()
        } else {
            Vec::new()
        };
        self.want(&references);
        let (images, warnings) = resident.images(&references);
        self.scene.resolve_materials(images);
        warnings
    }

    /// Joins what the DOM wanted painted to whatever downloaded.
    ///
    /// Nothing here can fail the run: with `--no-textures`, with no network,
    /// or with an asset that refuses to decode, the viewer falls back to the
    /// plain boxes it has always drawn.
    fn resolve_decor(&mut self, resident: &mut Resident) -> Vec<String> {
        let references = self.decor_plan.references();
        if references.is_empty() {
            return Vec::new();
        }
        self.want(&references);
        let (images, warnings) = resident.images(&references);
        self.decor = Decor::assemble(&self.decor_plan, &images, &self.scene.placements());
        warnings
    }

    /// Asks for the images the renderer's own passes sample — `Beam`,
    /// `Trail` and `ParticleEmitter` textures, an `ImageHandleAdornment`'s
    /// own, and every `ImageLabel` in the place's GUIs.
    ///
    /// A pass here reads an answer instead of resolving one itself — see
    /// `Answered`, whose "no answer yet" is what keeps an emitter alive until
    /// its texture lands. That is also why the warnings have to be carried
    /// back out from here: by the time a pass sees `Some(None)`, the failure
    /// that produced it has been reduced to "no image", and the pass has
    /// nothing left to report. A streaming loader reports these through
    /// `Resident::poll` instead, so this return value is what a *blocking*
    /// one (the CLI viewer, and every test) would otherwise lose.
    fn resolve_effect_images(&mut self, resident: &mut Resident) -> Vec<String> {
        let mut references: Vec<AssetRef> = Vec::new();
        let mut push = |reference: &AssetRef| {
            if *reference != AssetRef::Empty && !references.contains(reference) {
                references.push(reference.clone());
            }
        };
        for emitter in self.scene.particle_emitters() {
            push(&emitter.texture);
        }
        for beam in self.scene.beams() {
            push(&beam.texture);
        }
        for trail in self.scene.trails() {
            push(&trail.texture);
        }
        for reference in &self.scene.adornment_images() {
            push(reference);
        }
        for reference in &self.scene.gui_assets() {
            push(reference);
        }

        if references.is_empty() {
            self.images.clear();
            return Vec::new();
        }
        self.want(&references);
        let (_, warnings) = resident.images(&references);
        self.images = resident.answered(&references);
        warnings
    }

    /// Asks for the images the freshly re-planned GUI trees name, and joins
    /// whichever of them have already decoded — the GUI half of
    /// [`Loaded::resolve_effect_images`], on its own because a patched edit
    /// re-plans the GUI without touching the effect lists.
    ///
    /// Without this an `ImageLabel` pointed at an asset the place never
    /// showed would draw the placeholder for the rest of the session: the
    /// re-plan names the reference, but nothing asks for it, and a reference
    /// nobody asked for never lands (see `Headless::take_landed_assets`).
    /// Merged into `images` rather than replacing it, so the effect textures
    /// resolved beside them are left alone.
    ///
    /// Warnings are dropped like [`Loaded::resolve_fonts`]'s are: a
    /// streaming loader reports those through `Resident::poll`, and this
    /// path only ever runs on one.
    pub(crate) fn resolve_gui_images(&mut self, resident: &mut Resident) {
        let references = self.scene.gui_assets();
        if references.is_empty() {
            return;
        }
        self.want(&references);
        let (_, _warnings) = resident.images(&references);
        self.images.extend(resident.answered(&references));
    }

    /// Asks for the fonts the GUI trees' text wants, in the two stages a
    /// Roblox font takes: a family is a JSON file listing its faces, and only
    /// once that is in is it known which face file a weight and style come to
    /// (see `fonts::Family::closest`). A streaming loader answers the first
    /// stage on one landing and the second on the next; a blocking one runs
    /// both here.
    ///
    /// Warnings are dropped like the effect images' are: a face that will not
    /// download leaves its text in the typesetter's fallback face, which is
    /// what a missing font looks like in Roblox too.
    pub(crate) fn resolve_fonts(&mut self, resident: &mut Resident) {
        let faces = self.scene.gui_fonts();
        let families: Vec<AssetRef> = faces
            .iter()
            .map(|face| face.family.clone())
            .filter(|family| !self.fonts.families.contains_key(family))
            .collect();
        self.want(&families);
        let (answered, _) = resident.bytes(&families);
        for (reference, bytes) in answered {
            if let Some(family) = Family::parse(&bytes) {
                self.fonts.families.insert(reference, family);
            }
        }

        let files: Vec<AssetRef> = faces
            .iter()
            .filter_map(|face| Some(self.fonts.entry_of(face)?.asset.clone()))
            .filter(|file| !self.fonts.faces.contains_key(file))
            .collect();
        self.want(&files);
        let (answered, _) = resident.bytes(&files);
        for (reference, bytes) in answered {
            self.fonts
                .faces
                .insert(reference, std::sync::Arc::new(bytes));
        }
    }

    fn want(&mut self, references: &[AssetRef]) {
        for reference in references {
            if !self.wanted.contains(reference) {
                self.wanted.push(reference.clone());
            }
        }
    }
}
