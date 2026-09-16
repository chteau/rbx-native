//! Turns a parsed DOM into the shaped instances the renderer draws.

mod beam;
mod bounds;
mod effects;
mod filemesh;
mod gui;
mod material;
mod particles;
mod patch;
mod resync;
mod shape;
mod trail;
mod union;

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec3, Vec4};
use rbx_assets::AssetRef;
use rbx_dom::{CFrameData, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

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
pub(crate) use gui::{
    resolve as gui_layout, resolve_canvas as gui_canvas_layout, Anchor as GuiAnchor,
    Element as GuiElement, Rect as GuiRect, Screen as GuiScreen, SpaceGui,
};
// Only a test (`renderer::gui::quads`'s) names an element's image directly;
// everything else reaches one through `GuiElement::image`.
#[cfg(test)]
pub(crate) use gui::Painted;
pub(crate) use material::{Catalog, Kind, Maps, Slot};
pub(crate) use particles::sequence::{eval_color, eval_number};
pub(crate) use particles::{Emitter, Simulation};
pub(crate) use resync::PartSync;
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
/// UnionOperation is ignored; those keep rendering as their bounding box until
/// real CSG geometry lands. `MeshPart` and a `SpecialMesh` FileMesh child also
/// start out classified this way (see `shape::resolve`), which doubles as their
/// fallback if [`filemesh`] fails to resolve real geometry for them — see
/// `referent` and `suppressed`.
///
/// `Clone`/`Copy`: `Scene::resync_part` hands a value copy back to the
/// renderer rather than a borrow, so the caller is free of `Scene`'s own
/// borrow by the time it reaches into `Renderer`/`Offscreen`, both behind
/// other fields.
#[derive(Debug, Clone, Copy)]
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
    /// The DOM instance this box stands in for, so a later-resolved file mesh
    /// can find and hide it instead of drawing both on top of each other —
    /// and so the renderer's own per-instance patch maps (`renderer::shaped`,
    /// `renderer::translucent`, `renderer::shadow::casters`) can key
    /// themselves off the same id [`Scene::resync_part`] looks it up by.
    pub(crate) referent: Ref,
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
    bounds: Bounds,
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
        let mut scene = Scene {
            parts,
            bounds,
            materials,
            file_mesh_plan,
            resolved_file_meshes: filemesh::Resolved::default(),
            emitters: Vec::new(),
            beams: Vec::new(),
            trails: Vec::new(),
            gui: gui::plan(dom, database),
            gui_spaces: Vec::new(),
            union_plan,
            database: database.clone(),
        };
        // Needs `scene.placements()`, which only exists once `parts` is set —
        // an emitter's spawn volume is its parent's own placement, and a
        // `SurfaceGui`'s canvas covers one face of the part as drawn.
        let placements = scene.placements();
        scene.emitters = particles::plan(dom, database, &placements);
        scene.gui_spaces = gui::plan_space(dom, database, &placements);
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
    }

    /// Every part's unit-mesh placement, keyed by DOM referent so the texture
    /// planner can project a face instance onto the geometry actually drawn.
    ///
    /// Suppressed parts are left out: a `MeshPart` whose real mesh resolved no
    /// longer draws the box its decal would be projected on, and a decal
    /// floating in the air where that box used to be is worse than none.
    pub(crate) fn placements(&self) -> HashMap<Ref, Placement> {
        self.parts
            .iter()
            .filter(|part| !part.suppressed)
            .map(|part| (part.referent, part.placement()))
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
    /// resolve simply leaves its box alone.
    pub(crate) fn resolve_file_meshes(
        &mut self,
        meshes: HashMap<AssetRef, Arc<rbx_mesh::Mesh>>,
        images: HashMap<AssetRef, Arc<Image>>,
    ) {
        let (resolved, hidden) = filemesh::resolve(&self.file_mesh_plan, meshes, images);
        for part in &mut self.parts {
            if hidden.contains(&part.referent) {
                part.suppressed = true;
            }
        }
        self.resolved_file_meshes = resolved;
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
        for part in &mut self.parts {
            if resolution.hidden.contains(&part.referent) {
                part.suppressed = true;
            }
        }
        self.parts.extend(resolution.parts);
        let resolved = &mut self.resolved_file_meshes;
        resolved.meshes.extend(resolution.meshes);
        resolved.instances.extend(resolution.instances);
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
        referent,
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
    referent: Ref,
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
        referent,
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
