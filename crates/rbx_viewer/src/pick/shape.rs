//! A ray against the unit solids every procedurally shaped part is drawn as,
//! in the solid's own space: `[-0.5, 0.5]^3` for a box, radius 0.5 for a ball
//! or a cylinder — the convention [`crate::shapes`] builds each mesh to, so a
//! part's model matrix carries the same solid the GPU draws.
//!
//! Every solid here is convex, so a ray meets it along one contiguous span —
//! where it enters and where it leaves — and each solid is the intersection
//! of a few simpler spans: three slabs make a box, a slab and a quadratic make
//! a cylinder, a box and a half-space make a wedge. One clipping loop serves
//! them all instead of one bespoke test per shape.

use glam::{Mat4, Vec3};

use super::Ray;
use crate::scene::ShapeKind;

const HALF: f32 = 0.5;
const RADIUS: f32 = 0.5;

// A part scaled to nothing on some axis has a singular model matrix, which
// cannot be inverted into shape space at all. Rather than let the tests below
// produce NaNs and non-deterministic hits, such a part simply isn't pickable —
// it has no visible surface to click on either.
const SINGULAR: f32 = 1e-12;

/// How far along `ray` it first meets the unit solid of `kind` carried through
/// `model`. `None` when the ray misses, or when the solid is entirely behind
/// the ray's origin.
///
/// A ray starting *inside* the solid hits at distance 0 rather than missing,
/// so clicking while the camera sits inside a part still selects it.
pub(super) fn hit(kind: ShapeKind, model: Mat4, ray: Ray) -> Option<f32> {
    let local = Local::of(model, ray)?;
    let span = span_of(kind, local.origin, local.direction)?;
    Some(span.first_ahead()? / local.per_stud)
}

/// Where `ray` enters the unit solid of `kind` carried through `model`, and
/// the outward unit normal of the face it enters through, both in world
/// space. `None` when [`hit`] misses, and also for a ray starting inside,
/// which enters through no face at all.
pub(super) fn surface(kind: ShapeKind, model: Mat4, ray: Ray) -> Option<(Vec3, Vec3)> {
    let local = Local::of(model, ray)?;
    let span = span_of(kind, local.origin, local.direction)?;
    if span.entry < 0.0 {
        return None;
    }
    // A normal is a covector: a part stretched along one axis tilts its
    // faces' normals the other way, so it goes through the inverse-transpose.
    let normal = model.inverse().transpose().transform_vector3(span.normal);
    Some((ray.at(span.entry / local.per_stud), normal.try_normalize()?))
}

/// The distance a click or hover orders this shape by, which is [`hit`]'s own
/// answer except when the ray *starts inside* the shape: a part the camera
/// sits inside is around the camera, not in front of it, so it is ordered by
/// where the ray *leaves* it rather than by `0`. That puts it after whatever
/// stands in front, letting `Alt`-cycling — and a plain click — reach a child
/// the camera is looking at directly, instead of always landing on the part
/// the camera happens to be inside first.
pub(super) fn hit_key(kind: ShapeKind, model: Mat4, ray: Ray) -> Option<f32> {
    let local = Local::of(model, ray)?;
    let span = span_of(kind, local.origin, local.direction)?;
    let key = if span.entry >= 0.0 {
        span.entry
    } else {
        span.exit
    };
    (key >= 0.0).then_some(key / local.per_stud)
}

/// The span of `ray` (in the shape's own space) inside the shape, or `None`
/// when the ray misses it.
fn span_of(kind: ShapeKind, origin: Vec3, direction: Vec3) -> Option<Span> {
    match kind {
        // A truss is drawn as a lattice, but Studio selects it by its whole
        // extent too: clicking through a gap between its bars still picks it.
        ShapeKind::Box | ShapeKind::Truss { .. } => cube(origin, direction),
        ShapeKind::Ball => quadratic(origin, direction),
        ShapeKind::CylinderX => cylinder(origin, direction, 0),
        ShapeKind::CylinderY => cylinder(origin, direction, 1),
        // Half the cube under the plane `y = z`: the slope climbs from the
        // front-bottom edge to the back-top one, and the vertical face stands
        // at +Z — see `shapes::wedge`.
        ShapeKind::Wedge => cube(origin, direction)
            .and_then(|span| span.clip(half_space(origin, direction, Vec3::new(0.0, 1.0, -1.0)))),
        // The pyramid over the cube's floor whose apex is the (+X, +Y, -Z)
        // corner: its two sloped faces are `y = -z` and `y = x` — see
        // `shapes::corner_wedge`.
        ShapeKind::CornerWedge => cube(origin, direction)
            .and_then(|span| span.clip(half_space(origin, direction, Vec3::new(0.0, 1.0, 1.0))))
            .and_then(|span| span.clip(half_space(origin, direction, Vec3::new(-1.0, 1.0, 0.0)))),
    }
}

/// `ray` moved into the space a unit solid is defined in.
///
/// An affine map carries the ray parameter through unchanged, so a distance
/// found here is a world one — except that the direction is renormalized, so
/// every test below works on a unit vector however the part is scaled (a
/// 2048-stud baseplate would otherwise shrink it to where parallel-axis
/// checks lose their meaning). `per_stud` undoes that on the way out.
pub(super) struct Local {
    pub(super) origin: Vec3,
    pub(super) direction: Vec3,
    /// Local units per world stud along the ray: what a local distance is
    /// divided by to become a world one.
    pub(super) per_stud: f32,
}

impl Local {
    pub(super) fn of(model: Mat4, ray: Ray) -> Option<Self> {
        if model.determinant().abs() < SINGULAR {
            return None;
        }
        let inverse = model.inverse();
        let origin = inverse.transform_point3(ray.origin);
        let direction = inverse.transform_vector3(ray.direction);
        let per_stud = direction.length();
        if per_stud <= 0.0 || !per_stud.is_finite() {
            return None;
        }
        Some(Local {
            origin,
            direction: direction / per_stud,
            per_stud,
        })
    }
}

/// The stretch of a ray that lies inside one convex solid, as distances along
/// it. Either end may be infinite: a ray inside a half-space never leaves it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    entry: f32,
    exit: f32,
    /// Outward normal of the surface `entry` crosses, in the solid's own
    /// space and not necessarily unit length; zero where `entry` is infinite.
    normal: Vec3,
}

impl Span {
    const EVERYWHERE: Span = Span {
        entry: f32::NEG_INFINITY,
        exit: f32::INFINITY,
        normal: Vec3::ZERO,
    };

    /// The part of this span also inside `other`; `None` once nothing is left
    /// (or `other` never held the ray at all).
    fn clip(self, other: Option<Span>) -> Option<Span> {
        let other = other?;
        // Whichever surface the ray crosses last on the way in is the one
        // the intersection's own entry lies on.
        let (entry, normal) = if other.entry > self.entry {
            (other.entry, other.normal)
        } else {
            (self.entry, self.normal)
        };
        let span = Span {
            entry,
            exit: self.exit.min(other.exit),
            normal,
        };
        (span.entry <= span.exit).then_some(span)
    }

    /// The first point of the span at or ahead of the ray's origin — zero
    /// for an origin already inside — or `None` when the whole span lies
    /// behind it.
    fn first_ahead(self) -> Option<f32> {
        let entry = self.entry.max(0.0);
        (entry <= self.exit).then_some(entry)
    }
}

/// `|p[axis]| <= 0.5`, or `None` for a ray running parallel to the slab
/// outside it.
fn slab(origin: Vec3, direction: Vec3, axis: usize) -> Option<Span> {
    let (origin, direction) = (origin[axis], direction[axis]);
    if direction.abs() < f32::EPSILON {
        return (-HALF..=HALF).contains(&origin).then_some(Span::EVERYWHERE);
    }
    let first = (-HALF - origin) / direction;
    let second = (HALF - origin) / direction;
    // A ray heading up the axis enters through the face at -0.5.
    let mut normal = Vec3::ZERO;
    normal[axis] = -direction.signum();
    Some(Span {
        entry: first.min(second),
        exit: first.max(second),
        normal,
    })
}

fn cube(origin: Vec3, direction: Vec3) -> Option<Span> {
    slab(origin, direction, 0)?
        .clip(slab(origin, direction, 1))?
        .clip(slab(origin, direction, 2))
}

/// `normal . p <= 0`: the side of a plane through the origin the normal
/// points away from.
fn half_space(origin: Vec3, direction: Vec3, normal: Vec3) -> Option<Span> {
    let slope = normal.dot(direction);
    let height = normal.dot(origin);
    if slope.abs() < f32::EPSILON {
        return (height <= 0.0).then_some(Span::EVERYWHERE);
    }
    let crossing = -height / slope;
    Some(if slope > 0.0 {
        // Climbing out of the half-space: inside up to the plane.
        Span {
            entry: f32::NEG_INFINITY,
            exit: crossing,
            normal: Vec3::ZERO,
        }
    } else {
        Span {
            entry: crossing,
            exit: f32::INFINITY,
            normal,
        }
    })
}

/// `|p| <= 0.5` on whichever components `origin` and `direction` still carry:
/// the unit ball for all three, the infinite unit cylinder once the axis
/// component is zeroed.
///
/// Solved from the ray's closest approach to the axis rather than by the
/// textbook quadratic formula, whose `b^2 - 4ac` cancels catastrophically for
/// a small part seen from far away.
fn quadratic(origin: Vec3, direction: Vec3) -> Option<Span> {
    let along = direction.length_squared();
    if along < f32::EPSILON {
        return (origin.length_squared() <= RADIUS * RADIUS).then_some(Span::EVERYWHERE);
    }
    let middle = -origin.dot(direction) / along;
    let closest = origin + direction * middle;
    let gap = RADIUS * RADIUS - closest.length_squared();
    if gap < 0.0 {
        return None;
    }
    let half_chord = (gap / along).sqrt();
    let entry = middle - half_chord;
    Some(Span {
        entry,
        exit: middle + half_chord,
        // The point it enters at, seen from the centre (or, for a cylinder,
        // from the axis, whose component the caller has already zeroed).
        normal: origin + direction * entry,
    })
}

/// The capped unit cylinder lying along `axis`.
fn cylinder(origin: Vec3, direction: Vec3, axis: usize) -> Option<Span> {
    let radial = |mut vector: Vec3| {
        vector[axis] = 0.0;
        vector
    };
    quadratic(radial(origin), radial(direction))?.clip(slab(origin, direction, axis))
}

#[cfg(test)]
#[path = "shape/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "shape/surface_tests.rs"]
mod surface_tests;
