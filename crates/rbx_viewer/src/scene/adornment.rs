//! The 3D adornment family — `Handles`, `ArcHandles`, the `*HandleAdornment`
//! shapes, `SelectionBox`, `SelectionSphere` and `SurfaceSelection` — read
//! off a DOM and resolved into the handful of primitives the renderer draws
//! them from.
//!
//! All of them descend from `GuiBase3d`, documented as "3D GUI elements that
//! are rendered in the world": a colour, a transparency and a `Visible`
//! flag, adorned to something through an `Adornee`. Roblox renders one when
//! it "is a descendant of the `Workspace` or anywhere where GUI objects are
//! rendered" (`SelectionSphere`'s own page) — here that means `Workspace` or
//! `StarterGui`, which is already this viewer's stand-in for the player's
//! own GUI (see `scene::gui::plan::starter`).
//!
//! Pure data, like every other module beside [`Scene`](super::Scene):
//! everything below resolves to world-space primitives with no GPU handle
//! and no camera, which is what lets the whole family be tested without a
//! device. The one camera-dependent piece — the circle a `SelectionSphere`
//! outlines itself with — is carried as a [`Ring`] for the renderer to face
//! at the eye.
//!
//! Two things creator-docs describes are deliberately not read, because the
//! API dump this project syncs daily does not carry them: the newer
//! `AdornShading` enum (a `Shading` property on the shape adornments, which
//! would let one be lit or drawn through geometry) and
//! `ConeHandleAdornment.Hollow`. Both are documented but absent from the
//! dump, so no file can carry one yet, and their enum ordinals are not
//! published anywhere this project can read. The legacy
//! `HandleAdornment.AlwaysOnTop`/`ZIndex` pair, which the dump does carry,
//! is read in full.

mod handles;
mod selection;
mod shape;

use std::collections::HashMap;

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::props::{bool_or, cframe_or, float_or, int_or, linear_color_or, vector3_or, Properties};
use super::{descendants, Placement};

/// `GuiBase3d`, the base every adornment shares.
const BASE_CLASS: &str = "GuiBase3d";
/// The two places this viewer draws an adornment parented into.
const RENDERED_UNDER: [&str; 2] = ["Workspace", "StarterGui"];
/// `ZIndex`'s documented range. `-1` is the special value that gives up
/// `AlwaysOnTop` and puts the adornment back in the depth-tested scene.
const Z_INDEX_RANGE: (i32, i32) = (-1, 10);

/// One adornment instance, resolved to what draws it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Adornment {
    pub(crate) referent: Ref,
    pub(crate) pieces: Vec<Piece>,
    /// `AlwaysOnTop`: drawn over everything rather than depth-tested against
    /// the scene. Already folded with the documented `ZIndex == -1`
    /// exception, which overrides it.
    pub(crate) always_on_top: bool,
    /// `ZIndex`, clamped to its documented -1..10: the draw order among
    /// adornments, which only applies while [`Adornment::always_on_top`].
    pub(crate) order: i32,
    /// Every drawn part whose placement this adornment was built from, so a
    /// part that moves can re-plan exactly the adornments that followed it
    /// (see `Scene::adorns`).
    pub(crate) covers: Vec<Ref>,
}

/// One primitive of an adornment. World space throughout.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Piece {
    Solid(Solid),
    /// A line whose width is in *pixels* — `LineHandleAdornment.Thickness`
    /// is documented in pixels, unlike `SelectionBox.LineThickness`, which
    /// is in studs and is drawn as [`Mesh::Box`] edges instead.
    Line(Line),
    /// The camera-facing circle a `SelectionSphere` outlines itself with.
    Ring(Ring),
    Picture(Picture),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Solid {
    pub(crate) mesh: Mesh,
    /// Rigid placement: where the shape's own origin sits and how it is
    /// turned. Sizes live in [`Mesh`] rather than in this matrix's scale, so
    /// a cylinder sector or a cone can be built at the right proportions
    /// without unpicking them again.
    pub(crate) frame: Mat4,
    pub(crate) color: [f32; 3],
    pub(crate) alpha: f32,
}

/// The shapes an adornment draws as.
///
/// Where a shape has a length, it runs along the frame's own **-Z** — the
/// axis Roblox calls a `CFrame`'s `LookVector` — from the frame's origin, so
/// a handle aimed with `CFrame.lookAt(origin, target)`, the ordinary way
/// plugin code aims one, points at the target. Roblox publishes no axis
/// convention for the handle adornments at all; this is the one this
/// renderer picks, not a documented fact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Mesh {
    /// Centred on the frame, `size` studs across — `BoxHandleAdornment.Size`
    /// and every box-shaped piece built here.
    Box { size: Vec3 },
    /// Centred on the frame.
    Sphere { radius: f32 },
    /// Base on the frame's origin, apex `height` studs along -Z.
    Cone { radius: f32, height: f32 },
    /// From the frame's origin, `height` studs along -Z. `inner` hollows it
    /// out and `sweep` (degrees) cuts it to a pie slice, both documented on
    /// `CylinderHandleAdornment`.
    Cylinder {
        radius: f32,
        inner: f32,
        height: f32,
        sweep: f32,
    },
    /// A torus segment in the frame's XY plane, starting along +X and
    /// sweeping `sweep` degrees — what an `ArcHandles` arc is drawn from.
    Arc { radius: f32, tube: f32, sweep: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Line {
    pub(crate) from: Vec3,
    pub(crate) to: Vec3,
    pub(crate) pixels: f32,
    pub(crate) color: [f32; 3],
    pub(crate) alpha: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ring {
    pub(crate) centre: Vec3,
    pub(crate) radius: f32,
    pub(crate) pixels: f32,
    pub(crate) color: [f32; 3],
    pub(crate) alpha: f32,
}

/// `ImageHandleAdornment`: a flat, textured quad in the frame's own XY
/// plane, `size` studs across. Not camera-facing — nothing documents it as a
/// billboard, and its `CFrame` would have nothing to orient if it were.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Picture {
    pub(crate) frame: Mat4,
    pub(crate) size: glam::Vec2,
    pub(crate) texture: AssetRef,
    pub(crate) alpha: f32,
}

/// What every builder below starts from: the adornment's own instance, the
/// colour and transparency `GuiBase3d` gives it, and where its adornee
/// stands.
pub(super) struct Context<'a> {
    pub(super) properties: &'a Properties,
    /// The adornee's own rigid frame — its `CFrame` with the size divided
    /// back out — and its extent in studs. `None` when the adornment names
    /// no adornee this scene draws.
    pub(super) adornee: Option<Adornee>,
    pub(super) color: [f32; 3],
    pub(super) alpha: f32,
}

/// Where an adornment's adornee stands: a rigid frame and the extent the
/// adornment is sized and offset against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Adornee {
    pub(super) frame: Mat4,
    pub(super) size: Vec3,
}

/// Every adornment this scene draws, in DOM order.
pub(crate) fn plan(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    placements: &HashMap<Ref, Placement>,
) -> Vec<Adornment> {
    // Memoised per class, not asked per instance: this walks every instance
    // in the place, a superclass chain is several hash lookups deep, and a
    // place of tens of thousands of instances holds a few dozen distinct
    // classes — the same trick `scene::gui::space` already plays on the
    // same walk.
    let mut adornments: HashMap<String, bool> = HashMap::new();
    let mut found = Vec::new();
    for referent in descendants(dom) {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        let is_adornment = *adornments
            .entry(instance.class().to_string())
            .or_insert_with(|| database.is_subclass_of(instance.class(), BASE_CLASS));
        if !is_adornment || !rendered(dom, database, referent) {
            continue;
        }
        if let Some(adornment) = build(dom, database, placements, referent) {
            found.push(adornment);
        }
    }
    found
}

/// Whether an adornment parented here is drawn at all — see this module's
/// own note on where Roblox renders one.
fn rendered(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> bool {
    let mut current = dom.parent(referent);
    while let Some(ancestor) = current {
        let Some(instance) = dom.get(ancestor) else {
            return false;
        };
        if RENDERED_UNDER
            .iter()
            .any(|class| database.is_subclass_of(instance.class(), class))
        {
            return true;
        }
        current = dom.parent(ancestor);
    }
    false
}

fn build(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    placements: &HashMap<Ref, Placement>,
    referent: Ref,
) -> Option<Adornment> {
    let instance = dom.get(referent)?;
    let properties = instance.properties();
    // `Visible` is documented as hiding the object and its descendants; an
    // adornment has no drawn descendants of its own, so hiding it is all
    // there is to do.
    if !bool_or(properties, "Visible", true) {
        return None;
    }
    let alpha = 1.0 - float_or(properties, "Transparency", 0.0).clamp(0.0, 1.0);

    let (adornee, covers) = adornee_of(dom, database, placements, properties);
    let context = Context {
        properties,
        adornee,
        color: linear_color_or(properties, "Color3", [1.0, 1.0, 1.0]),
        alpha,
    };

    let class = instance.class();
    let pieces = shape::pieces(database, class, &context)
        .or_else(|| selection::pieces(database, class, &context))
        .or_else(|| handles::pieces(database, class, &context))?;
    if pieces.is_empty() {
        return None;
    }

    // `ZIndex == -1` is documented as overriding `AlwaysOnTop`, putting the
    // adornment back among ordinary depth-tested geometry.
    let order = int_or(properties, "ZIndex", 0).clamp(Z_INDEX_RANGE.0, Z_INDEX_RANGE.1);
    let always_on_top = bool_or(properties, "AlwaysOnTop", false) && order > Z_INDEX_RANGE.0;

    Some(Adornment {
        referent,
        pieces,
        always_on_top,
        order,
        covers,
    })
}

/// Resolves `Adornee` into the frame and extent the adornment is placed
/// against, plus every drawn part that answer was derived from.
///
/// A `BasePart` adornee keeps its own oriented box, which turns with it. Any
/// other adornee — a `Model`, a `Folder` — has no orientation to turn with,
/// so it gets the world-axis-aligned box around every part beneath it, the
/// same answer this renderer's own selection outline gives a container (see
/// `renderer::outline::box_of`).
fn adornee_of(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    placements: &HashMap<Ref, Placement>,
    properties: &Properties,
) -> (Option<Adornee>, Vec<Ref>) {
    let Some(Variant::Ref(adornee)) = properties.get("Adornee") else {
        return (None, Vec::new());
    };
    let adornee = *adornee;
    if let Some(placement) = placements.get(&adornee) {
        return (Some(Adornee::of(placement)), vec![adornee]);
    }
    let covered: Vec<Ref> = super::descendants_of(dom, adornee)
        .filter(|referent| placements.contains_key(referent))
        .collect();
    let models = covered
        .iter()
        .filter_map(|referent| placements.get(referent));
    let Some((min, max)) = crate::gizmo::bounds_of(models.map(|placement| placement.model)) else {
        return (None, covered);
    };
    let _ = database;
    (
        Some(Adornee {
            frame: Mat4::from_translation((min + max) * 0.5),
            size: max - min,
        }),
        covered,
    )
}

impl Adornee {
    fn of(placement: &Placement) -> Self {
        // `Placement::model` folds the part's size into its basis columns;
        // an adornment is placed against the part's own frame and offset by
        // its size separately, so the two come apart again here.
        let axis = |column: glam::Vec4, extent: f32| {
            let vector = column.truncate();
            if extent.abs() > f32::EPSILON {
                vector / extent
            } else {
                vector
            }
        };
        let model = placement.model;
        let size = placement.size;
        Adornee {
            frame: Mat4::from_cols(
                axis(model.x_axis, size.x).extend(0.0),
                axis(model.y_axis, size.y).extend(0.0),
                axis(model.z_axis, size.z).extend(0.0),
                model.w_axis,
            ),
            size,
        }
    }
}

impl Context<'_> {
    /// The frame a `HandleAdornment` draws in: the adornee's own, shifted by
    /// `SizeRelativeOffset` (documented as a scale of the adornee's `Size`,
    /// where 1 reaches the corresponding edge) and then composed with the
    /// adornment's own `CFrame`, which the docs say is "applied after any
    /// translations due to SizeRelativeOffset".
    ///
    /// With no adornee, the adornment's `CFrame` is a world frame outright:
    /// `WireframeHandleAdornment` documents an adornment as drawing "onto a
    /// `BasePart` ... or into the `Workspace`".
    pub(super) fn handle_frame(&self) -> Mat4 {
        let local = cframe_or(self.properties, "CFrame");
        let Some(adornee) = self.adornee else {
            return local;
        };
        let offset = vector3_or(self.properties, "SizeRelativeOffset", Vec3::ZERO);
        adornee.frame * Mat4::from_translation(offset * adornee.size * 0.5) * local
    }

    pub(super) fn solid(&self, mesh: Mesh, frame: Mat4) -> Piece {
        Piece::Solid(Solid {
            mesh,
            frame,
            color: self.color,
            alpha: self.alpha,
        })
    }
}

#[cfg(test)]
#[path = "adornment/tests.rs"]
mod tests;
