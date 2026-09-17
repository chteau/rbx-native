//! `BillboardGui` and `SurfaceGui`: the same `GuiObject` tree a `ScreenGui`
//! carries, but painted into an offscreen canvas that is then placed in the
//! world rather than over the frame.
//!
//! Only the placement differs, so everything below stops at the canvas: how
//! many pixels it is ([`SpaceGui::canvas`]) and where its rectangle lives
//! ([`Anchor`]). The tree itself is read by [`super::plan`] and resolved by
//! [`super::layout`], unchanged.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use glam::Vec3;
use rbx_assets::AssetRef;
use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::plan::{collect_assets, elements, flag, list_layout, span, vector2, List, Node};
use super::style::Styled;
use crate::scene::beam::{world_cframe, ParentMap};
use crate::scene::Placement;
use crate::textures::NormalId;

const BILLBOARD_CLASS: &str = "BillboardGui";
const SURFACE_CLASS: &str = "SurfaceGui";
const ATTACHMENT_CLASS: &str = "Attachment";

/// `SurfaceGui.CanvasSize`'s own default, which only a tree built in code ever
/// falls back to: a place file always serializes the property.
pub(super) const DEFAULT_CANVAS: [f32; 2] = [200.0, 50.0];

/// How finely a canvas with no pixel count of its own is rasterised, matching
/// `SurfaceGui.PixelsPerStud`'s default. A `BillboardGui` has no such property
/// at all, so this is the only density it can be drawn at.
const PIXELS_PER_STUD: f32 = 50.0;

/// `Enum.SurfaceGuiSizingMode.FixedSize`. The other value, `PixelsPerStud`
/// (1), is Roblox's default: a `SurfaceGui` only honours `CanvasSize` when
/// this one is set.
const FIXED_SIZE: u32 = 0;

/// Ceiling on either canvas axis: a huge `Size` would otherwise ask for a
/// texture no adapter will allocate, and nothing is legible past this anyway.
const MAX_CANVAS: f32 = 2048.0;

/// Where a canvas' rectangle sits in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Anchor {
    /// A camera-facing quad centred on `origin`. The renderer finishes it:
    /// only it knows the eye the quad has to turn towards, and `view_offset`
    /// is expressed in that same basis.
    Billboard {
        origin: Vec3,
        /// World width and height in studs — see [`studs`].
        size: [f32; 2],
        /// `StudsOffset`, in the camera's own right/up/forward basis.
        view_offset: Vec3,
        /// `StudsOffsetWorldSpace`, along the global axes.
        world_offset: Vec3,
    },
    /// A fixed quad on one face of a part: exactly the rectangle a stretched
    /// `Decal` on that face covers (see [`crate::textures`]), pushed out along
    /// the face normal by `ZOffset`.
    ///
    /// Corners in image order: top-left, top-right, bottom-right, bottom-left.
    Surface { corners: [Vec3; 4] },
}

/// One `BillboardGui`/`SurfaceGui` reduced to a canvas and a placement.
#[derive(Clone)]
pub(crate) struct SpaceGui {
    /// What the canvas hangs off (see [`adornee`]) — so an edit that moves
    /// that part knows to re-place the canvas, wherever in the tree the
    /// container itself sits.
    pub(crate) adornee: Ref,
    /// The offscreen texture's size in pixels, which is also the viewport the
    /// tree's top-level `UDim2`s resolve against.
    pub(crate) canvas: [f32; 2],
    /// `AlwaysOnTop`: drawn without a depth test, over the whole scene.
    pub(crate) always_on_top: bool,
    pub(crate) anchor: Anchor,
    pub(super) list: Option<List>,
    pub(super) roots: Vec<Node>,
}

impl SpaceGui {
    /// Every image the canvas wants, in first-seen paint order.
    pub(crate) fn assets(&self, into: &mut Vec<AssetRef>) {
        for root in &self.roots {
            collect_assets(root, into);
        }
    }
}

/// Every enabled `BillboardGui`/`SurfaceGui` that could be placed and has
/// something to draw.
///
/// `placements` is what both are measured against: a canvas hangs off the part
/// as the scene actually drew it, so a part that never made it in (a
/// `MeshPart` replaced by real geometry, say) carries no canvas.
///
/// TODO: `Brightness`, `LightInfluence`, `MaxDistance` and the container's own
/// `ClipsDescendants`.
pub(crate) fn plan(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    placements: &HashMap<Ref, Placement>,
) -> Vec<SpaceGui> {
    let parents = ParentMap::build(dom);
    let kinds = RefCell::new(HashMap::new());
    let styles = Styled::new(dom);
    let context = Context {
        dom,
        database,
        styles: &styles,
        parents: &parents,
        placements,
        kinds: &kinds,
    };
    let mut found = Vec::new();
    for &root in dom.root_refs() {
        gather(context, root, None, &mut found);
    }
    found
}

/// Everything the walk carries unchanged, bundled so [`gather`] keeps to the
/// arguments that actually move.
#[derive(Clone, Copy)]
struct Context<'a> {
    dom: &'a WeakDom,
    database: &'a ReflectionDatabase,
    styles: &'a Styled,
    parents: &'a ParentMap<'a>,
    placements: &'a HashMap<Ref, Placement>,
    /// Each class met so far, as `(billboard, surface)`: the walk asks the
    /// question of every instance in the place, and a superclass chain is
    /// several hash lookups per answer — memoised, a place of tens of
    /// thousands of instances asks it a few dozen times.
    kinds: &'a RefCell<HashMap<String, (bool, bool)>>,
}

/// Its own recursion rather than [`crate::scene::descendants`] for the same
/// reason [`super::plan::plan`] walks itself — paint order is tree order —
/// and because a container's *parent* is what it hangs off when `Adornee` is
/// unset, which a flat iterator cannot hand back.
fn gather(context: Context<'_>, referent: Ref, parent: Option<Ref>, into: &mut Vec<SpaceGui>) {
    let Some(instance) = context.dom.get(referent) else {
        return;
    };
    let (billboard, surface) = kind_of(context, instance.class());
    if billboard || surface {
        if let Some(gui) = read(context, instance, parent, billboard) {
            into.push(gui);
        }
        // Neither nests inside the other, and the children are the GUI tree.
        return;
    }
    for &child in instance.children() {
        gather(context, child, Some(referent), into);
    }
}

/// Whether `class` is a `BillboardGui`, a `SurfaceGui`, or neither — see
/// `Context::kinds`.
fn kind_of(context: Context<'_>, class: &str) -> (bool, bool) {
    if let Some(&known) = context.kinds.borrow().get(class) {
        return known;
    }
    let kind = (
        context.database.is_subclass_of(class, BILLBOARD_CLASS),
        context.database.is_subclass_of(class, SURFACE_CLASS),
    );
    context.kinds.borrow_mut().insert(class.to_string(), kind);
    kind
}

fn read(
    context: Context<'_>,
    instance: &Instance,
    parent: Option<Ref>,
    billboard: bool,
) -> Option<SpaceGui> {
    let properties = context.styles.properties_of(instance);
    if !flag(properties, "Enabled", true) {
        return None;
    }
    let adornee = adornee(context.dom, properties, parent)?;
    let roots = elements(
        context.dom,
        context.database,
        context.styles,
        instance.children(),
    );
    // A tree that paints nothing is every `SurfaceGui` holding only
    // transparent text in practice; allocating it a canvas is pure waste.
    if !roots.iter().any(Node::paints) {
        return None;
    }

    let (canvas, anchor) = match billboard {
        true => {
            let size = studs(span(properties, "Size"));
            let origin = origin(context, adornee)?;
            (
                billboard_canvas(size),
                Anchor::Billboard {
                    origin,
                    size,
                    view_offset: vector3(properties, "StudsOffset"),
                    world_offset: vector3(properties, "StudsOffsetWorldSpace"),
                },
            )
        }
        false => {
            let placement = context.placements.get(&adornee)?;
            let face = face(properties);
            let corners = face_corners(face, placement, number(properties, "ZOffset", 0.0));
            (
                surface_canvas(properties, face_studs(&corners)),
                Anchor::Surface { corners },
            )
        }
    };
    if canvas.iter().any(|axis| *axis < 1.0) {
        return None;
    }

    Some(SpaceGui {
        adornee,
        canvas,
        always_on_top: flag(properties, "AlwaysOnTop", false),
        anchor,
        list: list_layout(
            context.dom,
            context.database,
            context.styles,
            instance.children(),
        ),
        roots,
    })
}

/// What the canvas hangs off: `Adornee` where it points at a live instance,
/// the parent otherwise — Roblox's own rule is that the property *overrides*
/// the parent rather than complementing it.
pub(super) fn adornee(
    dom: &WeakDom,
    properties: &BTreeMap<String, Variant>,
    parent: Option<Ref>,
) -> Option<Ref> {
    match properties.get("Adornee") {
        Some(&Variant::Ref(referent)) if dom.get(referent).is_some() => Some(referent),
        _ => parent,
    }
}

/// `Face`, Front by default like Roblox itself.
fn face(properties: &BTreeMap<String, Variant>) -> NormalId {
    match properties.get("Face") {
        Some(&Variant::Enum(raw)) => NormalId::from_ordinal(raw).unwrap_or(NormalId::Front),
        _ => NormalId::Front,
    }
}

/// The canvas' pixel size for a billboard of `size` studs.
pub(super) fn billboard_canvas(size: [f32; 2]) -> [f32; 2] {
    size.map(|studs| (studs * PIXELS_PER_STUD).clamp(0.0, MAX_CANVAS).round())
}

/// The pixel size of a `SurfaceGui`'s canvas: the face's stud size at
/// `PixelsPerStud` by default, `CanvasSize` under `SizingMode.FixedSize`.
/// Either way the canvas is stretched over the whole face, so the two only
/// differ in how many pixels a `UDim2` offset comes to.
///
/// Falls back to [`DEFAULT_CANVAS`] per axis where the result is degenerate —
/// a zero axis would ask for a texture no adapter will allocate.
pub(super) fn surface_canvas(
    properties: &BTreeMap<String, Variant>,
    face_studs: [f32; 2],
) -> [f32; 2] {
    let raw = match properties.get("SizingMode") {
        Some(&Variant::Enum(FIXED_SIZE)) => vector2(properties, "CanvasSize"),
        _ => {
            let density = number(properties, "PixelsPerStud", PIXELS_PER_STUD);
            face_studs.map(|studs| studs * density)
        }
    };
    let axis = |value: f32, default: f32| match value.is_finite() && value >= 1.0 {
        true => value.min(MAX_CANVAS).round(),
        false => default,
    };
    [
        axis(raw[0], DEFAULT_CANVAS[0]),
        axis(raw[1], DEFAULT_CANVAS[1]),
    ]
}

/// World width and height of a `BillboardGui.Size`, in studs.
///
/// Simplification: Roblox gives the two halves of that `UDim2` different
/// units — the scale half is the billboard's stud size in 3D, the offset half
/// a constant screen-pixel size that does not shrink with distance. Only the
/// first is reproduced; an offset-only `Size` is read as studs at
/// [`PIXELS_PER_STUD`], so such a billboard keeps a fixed *world* size instead
/// of a fixed *screen* one.
///
/// TODO: true scale-with-distance for the offset half.
pub(super) fn studs(size: super::plan::Span) -> [f32; 2] {
    let axis = |scale: f32, offset: f32| match scale > 0.0 {
        true => scale,
        false => (offset / PIXELS_PER_STUD).max(0.0),
    };
    [
        axis(size.scale[0], size.offset[0]),
        axis(size.scale[1], size.offset[1]),
    ]
}

/// The four world corners of `face` on a part, in image order.
///
/// The same rectangle a stretched `Decal` covers: [`NormalId::axes`] is what
/// `crate::textures::face` builds its own projection from, so the canvas and a
/// decal on the very same face land on exactly the same quad.
pub(super) fn face_corners(face: NormalId, placement: &Placement, z_offset: f32) -> [Vec3; 4] {
    let (normal, u, v) = face.axes();
    // The unit mesh spans [-0.5, 0.5]³, so the face plane sits half a unit
    // along its own normal and the image axes span the other two.
    let centre = normal * 0.5;
    let model = &placement.model;
    let push = model.transform_vector3(normal).normalize_or_zero() * z_offset;
    let corner = |right: f32, down: f32| {
        model.transform_point3(centre + u * (right * 0.5) + v * (down * 0.5)) + push
    };
    [
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
    ]
}

/// Width and height in studs of a face quad in image order, as the part is
/// actually placed — so a scaled `Placement` sizes the canvas like Roblox
/// sizes it off the part's own `Size`.
pub(super) fn face_studs(corners: &[Vec3; 4]) -> [f32; 2] {
    [
        corners[1].distance(corners[0]),
        corners[3].distance(corners[0]),
    ]
}

/// The world position a billboard hangs off: an `Attachment`'s resolved CFrame
/// or a part's own centre.
fn origin(context: Context<'_>, adornee: Ref) -> Option<Vec3> {
    let instance = context.dom.get(adornee)?;
    if context
        .database
        .is_subclass_of(instance.class(), ATTACHMENT_CLASS)
    {
        return world_cframe(context.dom, context.parents, adornee)
            .map(|frame| frame.w_axis.truncate());
    }
    Some(context.placements.get(&adornee)?.model.w_axis.truncate())
}

fn vector3(properties: &BTreeMap<String, Variant>, name: &str) -> Vec3 {
    match properties.get(name) {
        Some(Variant::Vector3(value)) => Vec3::new(value.x, value.y, value.z),
        _ => Vec3::ZERO,
    }
}

fn number(properties: &BTreeMap<String, Variant>, name: &str, default: f32) -> f32 {
    let raw = match properties.get(name) {
        Some(&Variant::Float32(value)) => value,
        Some(&Variant::Float64(value)) => value as f32,
        _ => return default,
    };
    match raw.is_finite() {
        true => raw,
        false => default,
    }
}

#[cfg(test)]
mod tests;
