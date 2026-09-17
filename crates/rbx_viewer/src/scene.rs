//! Turns a parsed DOM into the shaped instances the renderer draws.

mod beam;
mod bounds;
mod effects;
mod filemesh;
mod gui;
mod identity;
mod material;
mod particles;
mod patch;
mod resync;
mod shape;
mod trail;
mod union;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::{Mat4, Vec3, Vec4};
use rbx_assets::AssetRef;
use rbx_dom::{CFrameData, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::fonts::Face;

pub(crate) use beam::{Beam, TextureMode};
#[cfg(test)]
pub(crate) use bounds::tests_support;
pub(crate) use effects::EffectKind;
// Only a test (`renderer::beam::ribbon`'s) constructs a `Curve` directly —
// everything else reaches one through `Beam::curve`.
#[cfg(test)]
pub(crate) use beam::Curve;
pub(crate) use bounds::{of_part, Bounds};
pub(crate) use filemesh::{
    fit_of as file_mesh_fit, AlphaMode, Appearance, Resolved, ResolvedInstance,
};
pub(crate) use gui::gui_image_placeholder;
pub(crate) use gui::{
    resolve_canvas_with as gui_canvas_layout_with, resolve_with as gui_layout_with,
    span_face as gui_span_face, Align as GuiAlign, Anchor as GuiAnchor, Element as GuiElement,
    GradientKind as GuiGradientKind, GradientPx as GuiGradient, Grouped as GuiGroup,
    ImageScale as GuiImageScale, Join as GuiJoin, Painted, PixelRect as GuiPixelRect,
    Rect as GuiRect, Screen as GuiScreen, SpaceGui, Text as GuiText, TextMeasure as GuiTextMeasure,
    Tile as GuiTile, Typeset as GuiTypeset, ViewCamera as GuiViewCamera, Viewport as GuiViewport,
};
#[cfg(test)]
pub(crate) use gui::{GroupTint as GuiGroupTint, StrokePx as GuiStroke, TextSpan as GuiTextSpan};
pub(crate) use identity::PartId;
pub(crate) use material::{Catalog, Kind, Maps, Slot};
pub(crate) use particles::sequence::{eval_color, eval_number};
pub(crate) use particles::{Emitter, Simulation};
use resync::Standing;
pub(crate) use resync::{Drawn, PartSync};
pub(crate) use shape::{resolve as resolve_shape, ShapeKind};
pub(crate) use trail::{segments as trail_segments, Recorder as TrailRecorder, Trail};
pub(crate) use union::Evaluations as UnionEvaluations;

use crate::assets::Image;

// Roblox's own "Medium stone grey", the default part color.
const FALLBACK_COLOR: [u8; 3] = [163, 162, 165];
const PART_ANCESTOR: &str = "BasePart";
// Terrain is a BasePart whose `size` covers the whole voxel region, so drawing it as a
// box would swallow the rest of the scene and wreck the camera framing.
pub(crate) const EXCLUDED_CLASS: &str = "Terrain";
/// How solid a `ForceField` part is drawn, whatever its `Transparency`.
const FORCE_FIELD_ALPHA: f32 = 0.5;

/// One BasePart reduced to a unit mesh instance.
///
/// Everything starts out as one of these, a `UnionOperation`, a `MeshPart` and
/// a `SpecialMesh` FileMesh child included (see `shape::resolve`), and that box
/// doubles as the fallback for whichever of them fails to resolve real geometry
/// — see `id` and `suppressed`. A union that resolves is either one computed
/// mesh or, where its boolean could not be run, several of these: one per
/// additive piece recovered from its operation tree, each with an id of its own
/// (see [`PartId`]).
///
/// `Clone`/`Copy`: `Scene::resync_part` hands a value copy back to the
/// renderer rather than a borrow, so the caller is free of `Scene`'s own
/// borrow by the time it reaches into `Renderer`/`Offscreen`, both behind
/// other fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Part {
    pub(crate) kind: ShapeKind,
    /// Which texture-array layer shades it, and how — see `scene::material`.
    pub(crate) material: Slot,
    pub(crate) transform: Mat4,
    pub(crate) color: [f32; 3],
    /// `1 - Transparency`. Zero means the part is not drawn at all; anything
    /// below one sends it to the renderer's blended pass.
    pub(crate) alpha: f32,
    pub(crate) reflectance: f32,
    /// `BasePart.CastShadow`. A part that answers false is still drawn and
    /// still *receives* shadows; it simply never reaches the depth pass.
    casts_shadow: bool,
    /// Extent the unit mesh is scaled to, i.e. the scale folded into
    /// `transform`. Kept apart from it because a `Texture`'s studs-per-tile is a
    /// physical length along the part, which a full matrix no longer spells out.
    size: Vec3,
    /// Which box in the picture this is: the DOM instance it stands in for,
    /// so a later-resolved file mesh can find and hide it instead of drawing
    /// both on top of each other, plus which piece of that instance it is
    /// where one instance draws as several (see [`PartId`]). The renderer's
    /// own per-instance patch maps (`renderer::shaped`,
    /// `renderer::translucent`, `renderer::shadow::casters`) key themselves
    /// off the same id [`Scene::resync_part`] looks it up by.
    pub(crate) id: PartId,
    /// Set by [`Scene::resolve_file_meshes`] once a real mesh has taken over
    /// drawing this part; left `false` forever if that resolution fails, which
    /// is exactly what keeps this box as the fallback.
    suppressed: bool,
}

/// Where a face instance has to be projected: the unit mesh a part draws as,
/// the matrix that places it, and the extent that matrix scales it to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placement {
    pub(crate) kind: ShapeKind,
    pub(crate) model: Mat4,
    pub(crate) size: Vec3,
}

impl Part {
    /// The DOM instance this box belongs to — the same referent for every
    /// recovered piece of one union, since the union is what the DOM knows.
    pub(crate) fn referent(&self) -> Ref {
        self.id.referent()
    }

    /// Whether a real file mesh has replaced this box; the renderer skips
    /// drawing it when true so the two never overlap.
    pub(crate) fn is_suppressed(&self) -> bool {
        self.suppressed
    }

    /// Whether the renderer paints this part at all. `Transparency` 1 is how a
    /// place says "invisible", and Roblox draws nothing for it — but a `Decal`
    /// pinned to such a part still shows, which is why the part keeps its
    /// placement (see [`Scene::placements`]) even when nothing draws it.
    pub(crate) fn is_drawn(&self) -> bool {
        !self.suppressed && self.alpha > 0.0
    }

    /// Whether the shadow map has to draw it. Roblox lets a part with a
    /// `Transparency` below 1 cast a full shadow, so the only things left out
    /// are the invisible ones and those that opted out.
    pub(crate) fn casts_shadow(&self) -> bool {
        self.is_drawn() && self.casts_shadow
    }

    /// Whether it has to wait for the blended pass instead of going in with the
    /// opaque geometry.
    pub(crate) fn is_translucent(&self) -> bool {
        self.alpha < 1.0
    }

    /// Where this box is drawn — the entry [`Scene::placements`] would hold
    /// for it, for a renderer keeping its own copy of that map in step with
    /// a single edit.
    pub(crate) fn placement(&self) -> Placement {
        Placement {
            kind: self.kind,
            model: self.transform,
            size: self.size,
        }
    }
}

/// A scene: the boxes to draw, their bounding extent, and the file meshes
/// (`MeshPart`/`SpecialMesh` FileMesh) waiting on network resolution.
///
/// Terrain is excluded to avoid swallowing other parts; everything else that inherits
/// from BasePart becomes a box, file meshes included — that box is also their fallback
/// (see [`Part::is_suppressed`]) until [`Scene::resolve_file_meshes`] runs.
pub(crate) struct Scene {
    parts: Vec<Part>,
    /// Where each referent stands in `parts`, so an edit finds its box
    /// without a scan of the place — see [`Standing`]. Kept in step by
    /// [`Scene::push_part`] and `Scene::remove_part`, the only two places
    /// `parts` changes length.
    standing: HashMap<Ref, Standing>,
    /// The extent as it stands, grown on the spot by every part that moved
    /// past it since [`Scene::refresh_bounds`] last ran — see
    /// `Scene::note_extent`.
    bounds: Bounds,
    /// `bounds` as [`Scene::refresh_bounds`] last answered with, so it can
    /// say whether the extent moved since.
    reported: Bounds,
    /// A part that may have been holding an edge moved or went, so `bounds`
    /// is only known to be too large: the next [`Scene::refresh_bounds`]
    /// recounts every part instead of trusting it.
    extent_stale: bool,
    materials: Catalog,
    file_mesh_plan: filemesh::Plan,
    /// Empty until [`Scene::resolve_file_meshes`] runs. `Renderer::new` only
    /// ever sees a `Scene` after that call, so this is where its GPU-ready
    /// mesh/texture data has to live — `Renderer::new`'s signature has no room
    /// for a third, network-dependent argument.
    resolved_file_meshes: filemesh::Resolved,
    /// Every `ParticleEmitter` parented to a drawn `BasePart`; see
    /// [`Scene::particle_emitters`].
    emitters: Vec<Emitter>,
    /// Every placeable `Beam`; see [`Scene::beams`].
    beams: Vec<Beam>,
    /// Every placeable `Trail`; see [`Scene::trails`].
    trails: Vec<Trail>,
    /// Every enabled `ScreenGui`; see [`Scene::gui_screens`].
    gui: Vec<GuiScreen>,
    /// Every placeable `BillboardGui`/`SurfaceGui`; see [`Scene::gui_spaces`].
    gui_spaces: Vec<SpaceGui>,
    union_plan: union::Plan,
    /// What every [`Scene::resolve_unions`] so far contributed to the resolved
    /// set, kept apart from it because [`Scene::resolve_file_meshes`] rebuilds
    /// that set from the file mesh plan alone and would otherwise drop it.
    /// The recovered *parts* need no such copy: they are appended to `parts`
    /// once and nothing rebuilds those.
    unions_resolved: union::Merged,
    /// Cloned once so [`Scene::resolve_unions`] can classify the `BasePart`s a
    /// downloaded union's operation tree turns out to hold — that only runs
    /// once network results are in, long after the `&ReflectionDatabase`
    /// `Scene::from_dom`'s caller lent us has gone out of scope.
    database: ReflectionDatabase,
}

impl Scene {
    /// Extracts drawable parts from a DOM.
    ///
    /// Returns an error if no BasePart descendants exist: there is nothing to show,
    /// and computing camera framing on an empty scene would be meaningless.
    pub(crate) fn from_dom(dom: &WeakDom, database: &ReflectionDatabase) -> Result<Self, String> {
        let mut materials = Catalog::new(dom, database);
        let parts: Vec<Part> = workspace_descendants(dom, database)
            .filter(|&referent| is_drawable(dom, database, referent))
            .filter_map(|referent| build_part(dom, database, referent, &mut materials))
            .collect();

        let bounds = bounds::of(&parts).ok_or_else(|| "no BasePart to draw".to_string())?;
        let file_mesh_plan = filemesh::plan(dom, database, &mut materials);
        let union_plan = union::plan(dom, database, &mut materials);
        let standing = parts
            .iter()
            .enumerate()
            .map(|(index, part)| (part.referent(), Standing::whole(index)))
            .collect();
        // After the workspace, the file meshes and the unions have claimed
        // their layers: a `ViewportFrame`'s parts share the catalog.
        let gui = gui::plan(dom, database, &mut materials);
        let mut scene = Scene {
            parts,
            standing,
            bounds,
            reported: bounds,
            extent_stale: false,
            materials,
            file_mesh_plan,
            resolved_file_meshes: filemesh::Resolved::default(),
            emitters: Vec::new(),
            beams: Vec::new(),
            trails: Vec::new(),
            gui,
            gui_spaces: Vec::new(),
            union_plan,
            unions_resolved: union::Merged::default(),
            database: database.clone(),
        };
        // Needs `scene.placements()`, which only exists once `parts` is set —
        // an emitter's spawn volume is its parent's own placement, and a
        // `SurfaceGui`'s canvas covers one face of the part as drawn.
        let placements = scene.placements();
        scene.emitters = particles::plan(dom, database, &placements);
        scene.gui_spaces = gui::plan_space(dom, database, &placements, &mut scene.materials);
        // A beam resolves its own attachment chain straight off the DOM
        // instead (see `scene::beam::attachment`), so it needs no placement.
        scene.beams = beam::plan(dom, database);
        // Same attachment-chain resolution as beams, and the same reason it
        // needs no placement either.
        scene.trails = trail::plan(dom, database);
        Ok(scene)
    }

    pub(crate) fn parts(&self) -> &[Part] {
        &self.parts
    }

    pub(crate) fn bounds(&self) -> &Bounds {
        &self.bounds
    }

    pub(crate) fn materials(&self) -> &Catalog {
        &self.materials
    }

    /// Every `ParticleEmitter` this scene found parented to a drawn `BasePart`.
    ///
    /// Static definitions only — the renderer owns the per-frame simulation
    /// state (`Simulation`) that steps them, since a `Scene` has no GPU handle
    /// and no notion of `dt`.
    pub(crate) fn particle_emitters(&self) -> &[Emitter] {
        &self.emitters
    }

    /// Every `Beam` this scene could place: both `Attachment0`/`Attachment1`
    /// resolved to a world CFrame. The renderer owns per-frame ribbon
    /// building (`FaceCamera` needs the eye, which a `Scene` never has).
    pub(crate) fn beams(&self) -> &[Beam] {
        &self.beams
    }

    /// Every `Trail` this scene could place: both `Attachment0`/`Attachment1`
    /// resolved to a world position. The renderer owns the per-frame history
    /// (`Recorder`) those positions feed — a `Scene` never sees a second
    /// frame to record one itself.
    pub(crate) fn trails(&self) -> &[Trail] {
        &self.trails
    }

    /// Every enabled `ScreenGui` this scene found, as resolution-independent
    /// trees. The renderer owns their pixel layout: a `UDim2` only becomes a
    /// rectangle against a viewport, and a `Scene` never sees one.
    pub(crate) fn gui_screens(&self) -> &[GuiScreen] {
        &self.gui
    }

    /// Every `BillboardGui`/`SurfaceGui` this scene could place, each already
    /// carrying the pixel size of the canvas its tree is painted into. The
    /// renderer owns that canvas — a `Scene` has no GPU handle — and finishes
    /// a billboard's quad, which only exists once an eye does.
    pub(crate) fn gui_spaces(&self) -> &[SpaceGui] {
        &self.gui_spaces
    }

    /// Every image the GUI trees sample, in first-seen paint order and
    /// without repeats across the three container kinds — one download for
    /// an `ImageLabel` image that a screen and a surface both show.
    pub(crate) fn gui_assets(&self) -> Vec<AssetRef> {
        let mut references = Vec::new();
        for screen in &self.gui {
            screen.assets(&mut references);
        }
        for gui in &self.gui_spaces {
            gui.assets(&mut references);
        }
        // Wanted only once something has an image to fall back from: a place
        // with no `ImageLabel` at all never downloads it.
        if !references.is_empty() {
            references.push(gui_image_placeholder());
        }
        references
    }

    /// Every font face the GUI trees' text wants, in first-seen paint order
    /// and without repeats — see [`Scene::gui_assets`].
    pub(crate) fn gui_fonts(&self) -> Vec<Face> {
        let mut faces = Vec::new();
        for screen in &self.gui {
            screen.fonts(&mut faces);
        }
        for gui in &self.gui_spaces {
            gui.fonts(&mut faces);
        }
        faces
    }

    /// Every material map the scene needs before [`Scene::resolve_materials`].
    pub(crate) fn material_assets(&self) -> Vec<AssetRef> {
        self.materials.asset_refs()
    }

    /// Joins the material layers to whatever downloaded, then re-reads every
    /// part's slot: a layer whose pack never arrived has just become plastic.
    ///
    /// Runs after [`Scene::resolve_file_meshes`] and [`Scene::resolve_unions`],
    /// which are what create the resolved/recovered parts patched here
    /// alongside the rest.
    pub(crate) fn resolve_materials(&mut self, images: HashMap<AssetRef, Arc<Image>>) {
        self.materials.resolve(images);
        for part in &mut self.parts {
            part.material = self.materials.slot(part.material.layer);
        }
        for instance in &mut self.resolved_file_meshes.instances {
            instance.material = self.materials.slot(instance.material.layer);
        }
        // A `ViewportFrame`'s parts point at the same layers, so they go
        // plastic-then-textured on the same schedule.
        let materials = &self.materials;
        let mut reslot = |part: &mut Part| part.material = materials.slot(part.material.layer);
        for screen in &mut self.gui {
            screen.viewport_parts(&mut reslot);
        }
        for gui in &mut self.gui_spaces {
            gui.viewport_parts(&mut reslot);
        }
    }

    /// Every part's unit-mesh placement, keyed by DOM referent so the texture
    /// planner can project a face instance onto the geometry actually drawn.
    ///
    /// One entry per referent, always the instance's own box and never one of
    /// a union's recovered pieces: the pieces are what the union is *drawn*
    /// as, while the union is the one thing an outline, a decal, an emitter's
    /// spawn volume or a `SurfaceGui`'s adornee addresses — and the box those
    /// pieces stand inside is the very shape `pick` hit-tests it as.
    ///
    /// Suppressed parts are left out, with that one exception: a `MeshPart`
    /// whose real mesh resolved no longer draws the box its decal would be
    /// projected on, and a decal floating in the air where that box used to
    /// be is worse than none. A union drawn as its pieces keeps its box's
    /// placement precisely because those pieces fill it.
    pub(crate) fn placements(&self) -> HashMap<Ref, Placement> {
        let pieced: HashSet<Ref> = self
            .parts
            .iter()
            .filter(|part| !part.id.is_whole())
            .map(Part::referent)
            .collect();
        self.parts
            .iter()
            .filter(|part| {
                part.id.is_whole() && (!part.suppressed || pieced.contains(&part.referent()))
            })
            .map(|part| (part.referent(), part.placement()))
            .collect()
    }

    /// The same, suppressed parts included — what the selection outline and
    /// the transform gizmo stand on.
    ///
    /// The opposite call from [`Scene::placements`]' for the opposite reason:
    /// a `MeshPart` or a union whose real mesh replaced its box is still a
    /// `BasePart` with a `Size` and a `CFrame`, is still what a click selects
    /// and a drag moves, and Studio outlines exactly that box around it —
    /// while a *decal* projected onto a box nothing draws would float in the
    /// air. Leaving them out here left every mesh in a place unable to show a
    /// gizmo at all, and put the handles the editor hit-tests (which read the
    /// DOM, not this map) somewhere the renderer drew nothing.
    pub(crate) fn all_placements(&self) -> HashMap<Ref, Placement> {
        self.parts
            .iter()
            .map(|part| (part.referent(), part.placement()))
            .collect()
    }

    /// Every mesh and texture asset a caller needs to download before calling
    /// [`Scene::resolve_file_meshes`].
    pub(crate) fn file_mesh_assets(&self) -> (Vec<AssetRef>, Vec<AssetRef>) {
        (
            self.file_mesh_plan.mesh_refs(),
            self.file_mesh_plan.texture_refs(),
        )
    }

    pub(crate) fn resolved_file_meshes(&self) -> &Resolved {
        &self.resolved_file_meshes
    }

    /// Joins the file mesh plan to whatever actually downloaded, then hides
    /// the fallback box of every instance that got real geometry.
    ///
    /// Always safe to call with empty or partial maps: anything that fails to
    /// resolve simply leaves its box alone. Safe to call *again* over a larger
    /// map, which is what a streaming load does every time more meshes land:
    /// the resolved set is rebuilt from the plan, the unions already merged in
    /// are put back on top of it, and suppression only ever grows — a part
    /// whose mesh resolved once never goes back to drawing its box on its own.
    pub(crate) fn resolve_file_meshes(
        &mut self,
        meshes: HashMap<AssetRef, Arc<rbx_mesh::Mesh>>,
        images: HashMap<AssetRef, Arc<Image>>,
    ) {
        let (resolved, hidden) = filemesh::resolve(&self.file_mesh_plan, meshes, images);
        for part in &mut self.parts {
            if hidden.contains(&part.referent()) {
                part.suppressed = true;
            }
        }
        self.resolved_file_meshes = resolved;
        self.apply_resolved_unions();
    }

    /// Puts the unions' own meshes and instances back into the resolved set
    /// after [`Scene::resolve_file_meshes`] has rebuilt it from the file mesh
    /// plan, which knows nothing about them.
    ///
    /// Idempotent, because a streaming tick calls it twice over a growing
    /// `unions_resolved`: once when the file mesh pass rebuilds the resolved
    /// set, and again when the union pass absorbs whatever bytes landed since.
    /// The instances are a `Vec`, so the second call would otherwise append a
    /// union already put back by the first and draw it twice — for good, since
    /// nothing later prunes the set.
    fn apply_resolved_unions(&mut self) {
        for part in &mut self.parts {
            if self.unions_resolved.hidden.contains(&part.referent()) {
                part.suppressed = true;
            }
        }
        let resolved = &mut self.resolved_file_meshes;
        resolved.meshes.extend(
            self.unions_resolved
                .meshes
                .iter()
                .map(|(reference, mesh)| (reference.clone(), Arc::clone(mesh))),
        );
        // By referent, which is the union part the instance draws in place of
        // and is unique across the whole resolved set: whatever an earlier
        // call put there is dropped, and the accumulated set goes back on top
        // in the order a cold load would have produced it.
        let unions: HashSet<Ref> = self
            .unions_resolved
            .instances
            .iter()
            .map(|instance| instance.referent)
            .collect();
        resolved
            .instances
            .retain(|instance| !unions.contains(&instance.referent));
        resolved
            .instances
            .extend(self.unions_resolved.instances.iter().cloned());
    }

    /// One `(referent, asset)` pair per legacy union/negate found, to download
    /// before calling [`Scene::resolve_unions`].
    pub(crate) fn union_assets(&self) -> Vec<(Ref, AssetRef)> {
        self.union_plan.assets()
    }

    /// Joins the union plan to whatever actually downloaded: a union whose
    /// boolean geometry computed joins the resolved file meshes (same upload
    /// path as a `MeshPart`), one that did not gets a `Part` per recovered
    /// additive piece instead, and either way its fallback box is hidden.
    ///
    /// Must run after [`Scene::resolve_file_meshes`] (which replaces the
    /// resolved set this appends to) and before [`Scene::resolve_materials`],
    /// which is what re-reads the material slot of everything added here.
    /// Always safe to call with an empty or partial map: anything that fails
    /// to resolve simply leaves its box alone.
    ///
    /// Safe to call again every tick of a streaming load, which is what one
    /// does: `assets` need only carry the bytes of what has not been carved
    /// yet, and a union already merged in is ignored however many times
    /// `union::resolve` answers for it again — see `union::Merged::absorb`,
    /// without which a failed boolean's recovered pieces would be appended to
    /// the parts a second time.
    pub(crate) fn resolve_unions(
        &mut self,
        assets: HashMap<AssetRef, Vec<u8>>,
        evaluations: &mut UnionEvaluations,
    ) {
        let resolution = union::resolve(
            &self.union_plan,
            assets,
            &self.database,
            &mut self.materials,
            evaluations,
        );
        self.unions_resolved.absorb(resolution);
        // `push_part`, not a raw extend: `self.standing` has to know every
        // one of these by referent too, the same as any other part, or a
        // later edit of one (`Scene::resync_part`) finds nothing standing
        // for it.
        for piece in std::mem::take(&mut self.unions_resolved.fresh_parts) {
            self.push_part(piece);
        }
        self.apply_resolved_unions();
    }

    /// Appends `part`, keeping [`Scene::standing`] in step: a part that is
    /// not its referent's own box is one of a failed union's recovered
    /// pieces (see `union::tree`), which stand under the same referent as
    /// the box they fill.
    pub(super) fn push_part(&mut self, part: Part) {
        let index = self.parts.len();
        let standing = self.standing.entry(part.referent()).or_default();
        match part.id.is_whole() {
            true => standing.whole = Some(index),
            false => standing.pieces += 1,
        }
        self.parts.push(part);
    }
}

/// Builds the model matrix of a part: its CFrame, then its shape's offset (if
/// any) in the part's own frame, then the shape's size as a scale.
///
/// Returns `None` when either CFrame or size is missing: we cannot draw a part
/// without both its position and its extent.
fn build_part(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
    materials: &mut Catalog,
) -> Option<Part> {
    let instance = dom.get(referent)?;
    let properties = instance.properties();

    let Some(Variant::Vector3(size)) = properties.get("size") else {
        return None;
    };
    let Some(Variant::CFrame(cframe)) = properties.get("CFrame") else {
        return None;
    };
    let size = Vec3::new(size.x, size.y, size.z);
    let geometry = shape::resolve(dom, database, instance, size);

    Some(assemble_part(
        properties,
        database,
        materials,
        geometry,
        cframe_matrix(cframe),
        PartId::whole(referent),
    ))
}

/// The tail of [`build_part`] once placement and shape are known; also how a
/// union's recovered leaf becomes a part, whose `cframe` is composed from its
/// operation tree rather than read off any instance.
pub(super) fn assemble_part(
    properties: &std::collections::BTreeMap<String, Variant>,
    database: &ReflectionDatabase,
    materials: &mut Catalog,
    geometry: shape::Geometry,
    cframe: Mat4,
    id: PartId,
) -> Part {
    let color = match properties.get("Color3uint8") {
        Some(&Variant::Color3uint8 { r, g, b }) => [r, g, b],
        _ => FALLBACK_COLOR,
    };
    let material = materials.slot_for(properties, database);
    let transform = geometry.model(cframe);

    Part {
        kind: geometry.kind,
        material,
        transform,
        color: color.map(|channel| srgb_to_linear(f32::from(channel) / 255.0)),
        alpha: alpha(properties, material.kind),
        reflectance: number(properties.get("Reflectance")).clamp(0.0, 1.0),
        casts_shadow: casts_shadow(properties),
        size: geometry.size,
        id,
        suppressed: false,
    }
}

/// `1 - Transparency`, except on a ForceField: Roblox draws one as a shell
/// however solid the part claims to be, so it is capped at half-opaque — which
/// also routes it through the renderer's blended pass.
///
/// TODO: the shimmering hex pattern a real ForceField has is not drawn.
fn alpha(properties: &std::collections::BTreeMap<String, Variant>, kind: Kind) -> f32 {
    let alpha = 1.0 - number(properties.get("Transparency")).clamp(0.0, 1.0);
    match kind {
        Kind::ForceField => alpha.min(FORCE_FIELD_ALPHA),
        _ => alpha,
    }
}

/// `BasePart.CastShadow`, which Studio only serializes when a builder has turned
/// it off: its absence means the part casts, like everything else in a place.
pub(super) fn casts_shadow(properties: &std::collections::BTreeMap<String, Variant>) -> bool {
    !matches!(properties.get("CastShadow"), Some(Variant::Bool(false)))
}

/// A `BasePart` float property, or 0 when the file leaves it out — which is what
/// both `Transparency` and `Reflectance` mean by their absence.
fn number(value: Option<&Variant>) -> f32 {
    let value = match value {
        Some(Variant::Float32(value)) => *value,
        Some(Variant::Float64(value)) => *value as f32,
        _ => return 0.0,
    };
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

/// Converts a CFrame to a column-major model matrix for glam.
///
/// CFrameData stores rotation in row-major order (R[i][j] at `rotation[i*3+j]`).
/// Roblox applies `R * local_point + position`, so each glam column must hold one column of R,
/// not one row: this ensures the rotation encodes the correct basis vectors.
pub(crate) fn cframe_matrix(cframe: &CFrameData) -> Mat4 {
    let r = cframe.rotation;
    let position = cframe.position;

    Mat4::from_cols(
        Vec4::new(r[0], r[3], r[6], 0.0),
        Vec4::new(r[1], r[4], r[7], 0.0),
        Vec4::new(r[2], r[5], r[8], 0.0),
        Vec4::new(position.x, position.y, position.z, 1.0),
    )
}

// Colors come out of a place file in sRGB while the render targets are `*Srgb`
// formats, which re-encode on write: shading has to happen on linearized values.
pub(crate) fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Checks if an instance is renderable: a BasePart that is not Terrain.
pub(crate) fn is_drawable(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> bool {
    dom.get(referent).is_some_and(|instance| {
        instance.class() != EXCLUDED_CLASS
            && database.is_subclass_of(instance.class(), PART_ANCESTOR)
    })
}

pub(crate) fn descendants(dom: &WeakDom) -> impl Iterator<Item = Ref> + '_ {
    walk(dom, dom.root_refs().to_vec())
}

/// `root` and everything under it, in the same pre-order as [`descendants`].
pub(crate) fn descendants_of(dom: &WeakDom, root: Ref) -> impl Iterator<Item = Ref> + '_ {
    walk(dom, vec![root])
}

fn walk(dom: &WeakDom, mut pending: Vec<Ref>) -> impl Iterator<Item = Ref> + '_ {
    std::iter::from_fn(move || {
        let referent = pending.pop()?;
        if let Some(instance) = dom.get(referent) {
            pending.extend_from_slice(instance.children());
        }
        Some(referent)
    })
}

pub(super) const WORKSPACE_CLASS: &str = "Workspace";

/// Every descendant of the DOM's `Workspace` service, `Workspace` itself
/// included — real Studio only ever draws what is actually parented under it,
/// so a `Part` staged in `ServerStorage`/`ReplicatedStorage`/etc. (extremely
/// common in real places: spare parts, templates, tool prefabs) must never
/// reach the renderer. Locates `Workspace` the same way `Lighting::from_dom`
/// locates the `Lighting` service, rather than walking the whole DOM and
/// filtering.
///
/// A file missing its `Workspace` (a bare `.rbxm` model, say) yields an empty
/// iterator: there is nothing to draw.
pub(crate) fn workspace_descendants<'a>(
    dom: &'a WeakDom,
    database: &ReflectionDatabase,
) -> impl Iterator<Item = Ref> + 'a {
    let root = dom.root_refs().iter().copied().find(|&referent| {
        dom.get(referent)
            .is_some_and(|instance| database.is_subclass_of(instance.class(), WORKSPACE_CLASS))
    });

    walk(dom, root.into_iter().collect())
}

#[cfg(test)]
#[path = "scene/tests.rs"]
mod tests;
