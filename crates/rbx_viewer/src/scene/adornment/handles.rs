//! `Handles` and `ArcHandles`: the two `HandlesBase` classes, which place
//! grab handles around an adornee rather than drawing one shape of their
//! own.
//!
//! Neither is interactive here — the docs are explicit that handles listen
//! for input only under a player's `PlayerGui` or the `CoreGui`, and this
//! viewer runs no scripts to listen with — so what is drawn is their
//! appearance: "the shape of the handles can be set to either arrows or
//! spheres" for `Handles`, and one arc per enabled axis for `ArcHandles`.
//!
//! How big a handle is beside its adornee is published nowhere, so the
//! proportions below are this renderer's own, chosen to sit clear of the
//! part without swamping a small one.

use glam::{Mat4, Vec3};
use rbx_dom::Variant;
use rbx_reflection::ReflectionDatabase;

use super::{Adornee, Context, Mesh, Piece};
use crate::textures::NormalId;

/// `Enum.HandlesStyle`, from the API dump: `Resize` draws spheres,
/// `Movement` arrows.
const STYLE_MOVEMENT: u32 = 1;

/// The size one handle is built from, as a fraction of the adornee's
/// largest extent, kept inside a range so a 200-stud baseplate does not grow
/// a 50-stud arrow and a half-stud part still gets a grabbable one.
const HANDLE_UNIT: (f32, f32, f32) = (0.25, 0.5, 4.0);
/// How far a handle's own base stands off the face it belongs to, in units.
const GAP: f32 = 0.15;
/// An arc's ring sits just outside the adornee, and its tube is a fraction
/// of the ring.
const ARC_CLEARANCE: f32 = 1.1;
const ARC_TUBE: f32 = 0.04;

pub(super) fn pieces(
    database: &ReflectionDatabase,
    class: &str,
    context: &Context<'_>,
) -> Option<Vec<Piece>> {
    let is = |ancestor: &str| database.is_subclass_of(class, ancestor);
    if is("Handles") {
        return Some(handles(context));
    }
    if is("ArcHandles") {
        return Some(arc_handles(context));
    }
    None
}

fn unit(adornee: &Adornee) -> f32 {
    let (fraction, min, max) = HANDLE_UNIT;
    (adornee.size.max_element() * fraction).clamp(min, max)
}

fn handles(context: &Context<'_>) -> Vec<Piece> {
    let Some(adornee) = context.adornee else {
        return Vec::new();
    };
    let Some(Variant::Faces(faces)) = context.properties.get("Faces") else {
        return Vec::new();
    };
    let movement = matches!(
        context.properties.get("Style"),
        Some(&Variant::Enum(STYLE_MOVEMENT))
    );
    let unit = unit(&adornee);

    enabled_faces(faces)
        .flat_map(|face| {
            let normal = face.axis();
            // Out of the face itself, not the centre: a long part's handles
            // stand off each of its own sides.
            let base = normal * ((adornee.size * normal.abs()).length() * 0.5 + unit * GAP);
            let frame = adornee.frame * aim(normal, base);
            if movement {
                arrow(context, frame, unit)
            } else {
                vec![context.solid(
                    Mesh::Sphere {
                        radius: unit * 0.25,
                    },
                    frame,
                )]
            }
        })
        .collect()
}

/// A movement handle: a shaft with a head on the end, both along the
/// frame's own -Z.
fn arrow(context: &Context<'_>, frame: Mat4, unit: f32) -> Vec<Piece> {
    let shaft = unit * 0.6;
    let head = unit * 0.4;
    vec![
        context.solid(
            Mesh::Cylinder {
                radius: unit * 0.06,
                inner: 0.0,
                height: shaft,
                sweep: 360.0,
            },
            frame,
        ),
        context.solid(
            Mesh::Cone {
                radius: unit * 0.18,
                height: head,
            },
            frame * Mat4::from_translation(Vec3::new(0.0, 0.0, -shaft)),
        ),
    ]
}

fn arc_handles(context: &Context<'_>) -> Vec<Piece> {
    let Some(adornee) = context.adornee else {
        return Vec::new();
    };
    let Some(Variant::Axes(axes)) = context.properties.get("Axes") else {
        return Vec::new();
    };
    let half = adornee.size * 0.5;

    [(0, axes.x), (1, axes.y), (2, axes.z)]
        .into_iter()
        .filter(|&(_, enabled)| enabled)
        .map(|(axis, _)| {
            // The ring lies in the plane the axis is normal to, so it is
            // sized by the two extents it encircles.
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            let radius = (half[a] * half[a] + half[b] * half[b]).sqrt() * ARC_CLEARANCE;
            let mut normal = Vec3::ZERO;
            normal[axis] = 1.0;
            context.solid(
                Mesh::Arc {
                    radius,
                    tube: radius * ARC_TUBE,
                    sweep: 360.0,
                },
                adornee.frame * ring_plane(normal),
            )
        })
        .collect()
}

/// A frame whose own -Z points along `direction`, placed at `at` — the
/// orientation every length-bearing shape in this family is built in (see
/// [`Mesh`]).
fn aim(direction: Vec3, at: Vec3) -> Mat4 {
    let forward = direction.normalize_or_zero();
    let aside = if forward.y.abs() < 0.99 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let right = aside.cross(forward).normalize_or_zero();
    let up = forward.cross(right);
    Mat4::from_cols(
        right.extend(0.0),
        up.extend(0.0),
        (-forward).extend(0.0),
        at.extend(1.0),
    )
}

/// A frame whose XY plane is the plane `normal` is perpendicular to — where
/// an [`Mesh::Arc`] is drawn.
fn ring_plane(normal: Vec3) -> Mat4 {
    let z = normal.normalize_or_zero();
    let aside = if z.y.abs() < 0.99 { Vec3::Y } else { Vec3::X };
    let x = aside.cross(z).normalize_or_zero();
    let y = z.cross(x);
    Mat4::from_cols(
        x.extend(0.0),
        y.extend(0.0),
        z.extend(0.0),
        Vec3::ZERO.extend(1.0),
    )
}

fn enabled_faces(faces: &rbx_dom::Faces) -> impl Iterator<Item = NormalId> + '_ {
    [
        (NormalId::Right, faces.right),
        (NormalId::Top, faces.top),
        (NormalId::Back, faces.back),
        (NormalId::Left, faces.left),
        (NormalId::Bottom, faces.bottom),
        (NormalId::Front, faces.front),
    ]
    .into_iter()
    .filter(|&(_, enabled)| enabled)
    .map(|(face, _)| face)
}
