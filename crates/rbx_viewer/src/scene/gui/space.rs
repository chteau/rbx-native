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

use super::plan::{
    collect_assets_of, collect_fonts_of, elements, flag, float, global_z_index, hides_contents,
    layout_of, span, vector2, Group, Layout, Node,
};
use super::style::Styled;
use crate::fonts::Face;
use crate::scene::beam::{world_cframe, ParentMap};
use crate::scene::Placement;
use crate::scene::{Catalog, Part};

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

/// `Brightness`'s own documented ceiling: "can be set to any number between 0
/// and 1000".
const MAX_BRIGHTNESS: f32 = 1000.0;

/// `SurfaceGui.MaxDistance`'s default, which a tree built in code falls back
/// to; a `BillboardGui` has no limit by default.
const SURFACE_MAX_DISTANCE: f32 = 1000.0;

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
        /// `SizeOffset`: a shift in units of the billboard's own size, along
        /// the camera's right and up axes — "a 2D offset in size-relative
        /// units that acts like an anchor point" (`BillboardGui.SizeOffset`).
        size_offset: [f32; 2],
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
    /// The `BillboardGui`/`SurfaceGui` itself.
    pub(crate) referent: Ref,
    /// What the canvas hangs off (see [`adornee`]) — so an edit that moves
    /// that part knows to re-place the canvas, wherever in the tree the
    /// container itself sits.
    pub(crate) adornee: Ref,
    /// The offscreen texture's size in pixels, which is also the viewport the
    /// tree's top-level `UDim2`s resolve against.
    pub(crate) canvas: [f32; 2],
    /// `AlwaysOnTop`: drawn without a depth test, over the whole scene.
    pub(crate) always_on_top: bool,
    /// What the canvas' colour is scaled by before it is composited — see
    /// [`brightness`].
    pub(crate) brightness: f32,
    /// `MaxDistance`: how far the eye may be before the canvas stops being
    /// drawn at all. Infinite where the property means "no limit".
    pub(crate) max_distance: f32,
    pub(crate) anchor: Anchor,
    /// `ZIndexBehavior.Global`, which a `BillboardGui`/`SurfaceGui` carries
    /// like any other `LayerCollector`.
    pub(super) global_z_index: bool,
    pub(super) list: Option<Layout>,
    pub(super) roots: Vec<Node>,
    pub(super) groups: Vec<Group>,
}

impl SpaceGui {
    /// Every image the canvas wants, in first-seen paint order.
    pub(crate) fn assets(&self, into: &mut Vec<AssetRef>) {
        collect_assets_of(&self.roots, &self.groups, into);
    }

    /// Every font face the canvas' text wants, in first-seen paint order.
    pub(crate) fn fonts(&self, into: &mut Vec<Face>) {
        collect_fonts_of(&self.roots, &self.groups, into);
    }

    /// Every `ViewportFrame` part on the canvas — see `Screen::viewport_parts`.
    pub(crate) fn viewport_parts(&mut self, apply: &mut impl FnMut(&mut Part)) {
        for root in &mut self.roots {
            super::plan::each_viewport_part(root, apply);
        }
    }
}

/// Every enabled `BillboardGui`/`SurfaceGui` that could be placed and has
/// something to draw.
///
/// `placements` is what both are measured against: a canvas hangs off the part
/// as the scene actually drew it, so a part that never made it in (a
/// `MeshPart` replaced by real geometry, say) carries no canvas.
pub(crate) fn plan(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    placements: &HashMap<Ref, Placement>,
    materials: &mut Catalog,
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
        gather(context, materials, root, None, &mut found);
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
///
/// `materials` rides beside [`Context`] rather than inside it: the context
/// is copied down the walk, and a catalog a `ViewportFrame` adds layers to
/// cannot be.
fn gather(
    context: Context<'_>,
    materials: &mut Catalog,
    referent: Ref,
    parent: Option<Ref>,
    into: &mut Vec<SpaceGui>,
) {
    let Some(instance) = context.dom.get(referent) else {
        return;
    };
    // "The contents of `StarterGui`" a hidden development GUI covers is the
    // whole subtree, canvases included — see `plan::starter`.
    if hides_contents(context.database, instance) {
        return;
    }
    let (billboard, surface) = kind_of(context, instance.class());
    if billboard || surface {
        if let Some(gui) = read(context, materials, instance, parent, billboard) {
            into.push(gui);
        }
        // Neither nests inside the other, and the children are the GUI tree.
        return;
    }
    for &child in instance.children() {
        gather(context, materials, child, Some(referent), into);
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
    materials: &mut Catalog,
    instance: &Instance,
    parent: Option<Ref>,
    billboard: bool,
) -> Option<SpaceGui> {
    let properties = context.styles.properties_of(instance);
    if !flag(properties, "Enabled", true) {
        return None;
    }
    let adornee = adornee(context.dom, properties, parent)?;
    let (roots, groups) = elements(
        context.dom,
        context.database,
        context.styles,
        materials,
        instance.children(),
    );
    // A tree that paints nothing is every `SurfaceGui` holding only
    // transparent text in practice; allocating it a canvas is pure waste.
    if !roots.iter().any(Node::paints) && !groups.iter().any(Group::paints) {
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
                    size_offset: vector2(properties, "SizeOffset"),
                },
            )
        }
        false => {
            let placement = context.placements.get(&adornee)?;
            let face = face(properties);
            let corners = face_corners(face, placement, float(properties, "ZOffset", 0.0));
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
        referent: instance.referent(),
        adornee,
        canvas,
        always_on_top: always_on_top(properties),
        brightness: brightness(properties),
        max_distance: max_distance(properties, billboard),
        anchor,
        global_z_index: global_z_index(properties),
        list: layout_of(
            context.dom,
            context.database,
            context.styles,
            instance.children(),
        ),
        roots,
        groups,
    })
}

fn always_on_top(properties: &BTreeMap<String, Variant>) -> bool {
    flag(properties, "AlwaysOnTop", false)
}

/// `Brightness` under `LightInfluence`, as the factor the canvas' colour is
/// multiplied by.
///
/// "Determines the factor by which the container's light is scaled when
/// `LightInfluence` is 0 ... `Brightness` ... has no effect when either
/// `LightInfluence` is 1 or `AlwaysOnTop` is true"
/// (`BillboardGui.Brightness`, `SurfaceGui.Brightness`), and `LightInfluence`
/// itself runs "from 0 to 1 ... 1 means that surrounding lighting has complete
/// control over the appearance". This viewer has no per-canvas light probe, so
/// full influence is taken as the canvas' own colours unscaled and the two are
/// mixed across the range.
fn brightness(properties: &BTreeMap<String, Variant>) -> f32 {
    if always_on_top(properties) {
        return 1.0;
    }
    let influence = float(properties, "LightInfluence", 0.0).clamp(0.0, 1.0);
    let brightness = float(properties, "Brightness", 1.0).clamp(0.0, MAX_BRIGHTNESS);
    brightness + (1.0 - brightness) * influence
}

/// `MaxDistance` in studs, `f32::INFINITY` where there is no limit.
///
/// "A value of 0 ... means there is no limit and it will render infinitely far
/// away" (`BillboardGui.MaxDistance`); a billboard's own default is `inf` and
/// a `SurfaceGui`'s is 1000 ("the default value of 1000 works fine for most
/// cases").
fn max_distance(properties: &BTreeMap<String, Variant>, billboard: bool) -> f32 {
    let default = match billboard {
        true => f32::INFINITY,
        false => SURFACE_MAX_DISTANCE,
    };
    match float(properties, "MaxDistance", default) {
        limit if limit <= 0.0 => f32::INFINITY,
        limit => limit,
    }
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

mod geometry;

pub(in crate::scene::gui) use geometry::{
    billboard_canvas, face, face_corners, face_studs, studs, surface_canvas,
};

#[cfg(test)]
mod tests;
