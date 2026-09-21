//! The selection adornments — `SelectionBox`, `SelectionSphere` and
//! `SurfaceSelection` — each of which wraps its adornee rather than being
//! placed by a `CFrame` of its own.

use glam::{Mat4, Vec3};
use rbx_reflection::ReflectionDatabase;

use super::{Adornee, Context, Mesh, Piece, Ring, Solid};
use crate::scene::props::{float_or, linear_color_or};
use crate::textures::NormalId;

/// Studio's own default `SelectionBox.LineThickness`. Documented only as
/// "measured in studs", with `0` drawing no outline at all; the figure is
/// this renderer's stand-in for a file that carries none, which a file
/// Studio wrote never is.
const DEFAULT_LINE_THICKNESS: f32 = 0.05;
/// How thick a `SurfaceSelection`'s highlighted face is drawn, in studs. The
/// docs say only that it "highlights a face ... in a configurable color" and
/// give it no thickness, so this is a slab thin enough to read as the
/// surface itself.
const FACE_THICKNESS: f32 = 0.05;
/// How far a `SelectionSphere`'s outline ring sits, on screen, across.
const RING_PIXELS: f32 = 2.0;

pub(super) fn pieces(
    database: &ReflectionDatabase,
    class: &str,
    context: &Context<'_>,
) -> Option<Vec<Piece>> {
    let is = |ancestor: &str| database.is_subclass_of(class, ancestor);
    if is("SelectionBox") {
        return Some(selection_box(context));
    }
    if is("SelectionSphere") {
        return Some(selection_sphere(context));
    }
    if is("SurfaceSelection") {
        return Some(surface_selection(context));
    }
    if is("SelectionLasso") {
        // A lasso draws a line from a `Humanoid`'s torso to a part or a
        // point, and both classes that carry that other end —
        // `SelectionPartLasso` and `SelectionPointLasso` — are deprecated.
        // The base class itself has nothing but the `Humanoid`, so there is
        // no line for one to draw. Recognized rather than left unhandled.
        return Some(Vec::new());
    }
    if is("ParabolaAdornment") {
        // Every property a parabola has (`A`, `B`, `C`, `Range`,
        // `Thickness`) is tagged `Hidden` in the API dump, and Roblox
        // publishes no page for the class at all — there is no documented
        // curve to draw. Recognized rather than guessed at.
        return Some(Vec::new());
    }
    None
}

/// The documented box: an outline of `LineThickness` studs in `Color3`, and
/// surfaces in `SurfaceColor3` that are invisible by default
/// (`SurfaceTransparency` 1).
fn selection_box(context: &Context<'_>) -> Vec<Piece> {
    let Some(adornee) = context.adornee else {
        return Vec::new();
    };
    let mut pieces = Vec::new();

    let surface_alpha =
        1.0 - float_or(context.properties, "SurfaceTransparency", 1.0).clamp(0.0, 1.0);
    if surface_alpha > 0.0 {
        pieces.push(Piece::Solid(Solid {
            mesh: Mesh::Box { size: adornee.size },
            frame: adornee.frame,
            color: linear_color_or(context.properties, "SurfaceColor3", [1.0, 1.0, 1.0]),
            alpha: surface_alpha,
        }));
    }

    let thickness = float_or(context.properties, "LineThickness", DEFAULT_LINE_THICKNESS).max(0.0);
    if thickness > 0.0 && context.alpha > 0.0 {
        pieces.extend(edges(&adornee, thickness).map(|(mesh, frame)| context.solid(mesh, frame)));
    }
    pieces
}

/// The twelve edges of the adornee's box, each a bar `thickness` studs
/// square. Every bar is that much longer than the span it covers, so the
/// three meeting at a corner close it instead of leaving a notch.
fn edges(adornee: &Adornee, thickness: f32) -> impl Iterator<Item = (Mesh, Mat4)> + '_ {
    let half = adornee.size * 0.5;
    (0..3).flat_map(move |axis| {
        [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)]
            .into_iter()
            .map(move |(a, b)| {
                let mut size = Vec3::splat(thickness);
                let mut offset = Vec3::ZERO;
                size[axis] = adornee.size[axis] + thickness;
                offset[(axis + 1) % 3] = half[(axis + 1) % 3] * a;
                offset[(axis + 2) % 3] = half[(axis + 2) % 3] * b;
                (
                    Mesh::Box { size },
                    adornee.frame * Mat4::from_translation(offset),
                )
            })
    })
}

/// The documented sphere: "the sphere's geometry consists of a ring/outline
/// in addition to a surface". The surface is `SurfaceColor3` at
/// `SurfaceTransparency` (invisible by default, so the ring is usually the
/// whole of what shows); the ring is the sphere's own silhouette, which only
/// the renderer can face at the camera.
///
/// How large the sphere is around its adornee is not published: this one is
/// the adornee box's own bounding sphere, so nothing inside it pokes out.
fn selection_sphere(context: &Context<'_>) -> Vec<Piece> {
    let Some(adornee) = context.adornee else {
        return Vec::new();
    };
    let centre = adornee.frame.w_axis.truncate();
    let radius = adornee.size.length() * 0.5;
    let mut pieces = Vec::new();

    let surface_alpha =
        1.0 - float_or(context.properties, "SurfaceTransparency", 1.0).clamp(0.0, 1.0);
    if surface_alpha > 0.0 {
        pieces.push(Piece::Solid(Solid {
            mesh: Mesh::Sphere { radius },
            frame: Mat4::from_translation(centre),
            color: linear_color_or(context.properties, "SurfaceColor3", [1.0, 1.0, 1.0]),
            alpha: surface_alpha,
        }));
    }
    if context.alpha > 0.0 {
        pieces.push(Piece::Ring(Ring {
            centre,
            radius,
            pixels: RING_PIXELS,
            color: context.color,
            alpha: context.alpha,
        }));
    }
    pieces
}

/// One face of the adornee, highlighted in `Color3` — a slab laid on the
/// face `TargetSurface` names.
fn surface_selection(context: &Context<'_>) -> Vec<Piece> {
    let Some(adornee) = context.adornee else {
        return Vec::new();
    };
    let face = match context.properties.get("TargetSurface") {
        Some(&rbx_dom::Variant::Enum(raw)) => NormalId::from_ordinal(raw),
        _ => Some(NormalId::Top),
    };
    let Some(face) = face else {
        return Vec::new();
    };
    let normal = face.axis();
    // The slab's own thickness along the face normal, everything else the
    // face's own extent.
    let mut size = adornee.size;
    for axis in 0..3 {
        if normal[axis].abs() > 0.5 {
            size[axis] = FACE_THICKNESS;
        }
    }
    let offset = normal * (adornee.size * normal.abs()).length() * 0.5;
    vec![context.solid(
        Mesh::Box { size },
        adornee.frame * Mat4::from_translation(offset),
    )]
}
