//! Studio's light guides (`Studio.Show Light Guides`): the lines drawn
//! around a selected light "that indicate the color and field of effect"
//! (`effects/light-sources.md`), in the light's own `Color`.
//!
//! Roblox documents what a guide is for, not how it is drawn, so the shapes
//! here are read off Studio's own guides:
//!
//! - `PointLight`: three great circles of radius `Range` about the light,
//!   one in each plane of its parent's axes.
//! - `SpotLight`: "a cone with a spherical base" (the class docs) from the
//!   light's position along `Face`: the axis out to `Range`, the rim circle
//!   where the cone meets that sphere — `Range·cos(Angle/2)` along the axis,
//!   `Range·sin(Angle/2)` across — and four slant lines out to the rim. On a
//!   part the apex is the part's centre, not the face: Studio's slant lines,
//!   carried back, meet there.
//! - `SurfaceLight`: the same reach stretched over the whole face ("light
//!   emits from the entire surface"): a line from each corner of the face to
//!   a far rectangle `Range·cos(Angle/2)` out and `Range·sin(Angle/2)` wider
//!   on every side, that rectangle's edges, and the centre line out to
//!   `Range` — which is why the centre line overshoots the far rectangle in
//!   Studio too. On an `Attachment`, which has no face, it is the spot's
//!   cone: the class docs call it "equivalent to a SpotLight" there.
//!
//! Drawn only for a light that is itself selected and `Enabled`: Studio's
//! announcement says to "select any light(s) you want to visualize and check
//! the Enabled box", and that guides "will not show when just selecting a
//! light's parent".

use std::f32::consts::TAU;

use glam::{Mat4, Vec2, Vec3};
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{
    boolean, color, face, frame_of, half_angle, number, range, size_of, ATTACHMENT_CLASS,
    LIGHT_CLASS, SPOT_CLASS, SURFACE_CLASS,
};
use crate::renderer::Segment;
use crate::scene::is_drawable;
use crate::textures::NormalId;

/// Straight pieces per guide circle — Studio's read as a polygon of about
/// this many sides.
const CIRCLE_SEGMENTS: usize = 32;
/// How a guide is shaded from its light's linear `Color`: Studio draws a
/// white light's guide as a light grey (about 213 of 255) letting some 30%
/// of the sky behind it through. Both matched by eye against Studio's own
/// screenshots, through this renderer's blending and tone map.
const SHADE: f32 = 0.75;
const ALPHA: f32 = 0.5;
/// A rim smaller than this is an `Angle` of 0: its slant lines would all lie
/// on the axis line.
const MIN_RIM: f32 = 1e-4;

/// The guides of every light in `selected`, in world space. Anything else in
/// the selection — a part, even one with lights of its own — adds nothing.
pub fn light_guides(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    selected: &[Ref],
) -> Vec<Segment> {
    selected
        .iter()
        .filter_map(|&referent| guide(dom, database, referent))
        .flatten()
        .collect()
}

fn guide(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> Option<Vec<Segment>> {
    let light = dom.get(referent)?;
    let class = light.class();
    let properties = light.properties();
    if !database.is_subclass_of(class, LIGHT_CLASS)
        || !boolean(properties.get("Enabled")).unwrap_or(true)
    {
        return None;
    }
    let spot = database.is_subclass_of(class, SPOT_CLASS);
    let surface = database.is_subclass_of(class, SURFACE_CLASS);
    let range = range(properties.get("Range"), spot);
    if range <= 0.0 {
        return None;
    }
    let (frame, size) = placement(dom, database, dom.parent(referent)?)?;

    let lines = if spot || surface {
        let face = face(properties.get("Face"))?;
        let half = half_angle(number(properties.get("Angle")));
        let basis = Basis::of(frame, face);
        match size {
            Some(size) if surface => {
                let (normal, u, v) = face.axes();
                let extent = |axis: Vec3| 0.5 * size.dot(axis.abs());
                let face = Basis {
                    origin: basis.origin + basis.normal * extent(normal),
                    ..basis
                };
                frustum(&face, Vec2::new(extent(u), extent(v)), range, half)
            }
            _ => cone(&basis, range, half),
        }
    } else {
        sphere(frame, range)
    };

    let color = (color(properties.get("Color")).unwrap_or(Vec3::ONE) * SHADE)
        .extend(ALPHA)
        .to_array();
    Some(
        lines
            .into_iter()
            .map(|(from, to)| Segment {
                from,
                to,
                color,
                on_top: false,
            })
            .collect(),
    )
}

/// Where a light on `parent` stands, and the extent of the part it lights
/// from — `None` on an `Attachment`, which has none. The same two parents
/// `super::local_lights` reads a light off.
fn placement(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    parent: Ref,
) -> Option<(Mat4, Option<Vec3>)> {
    let instance = dom.get(parent)?;
    if database.is_subclass_of(instance.class(), ATTACHMENT_CLASS) {
        let part = dom.parent(parent)?;
        if !is_drawable(dom, database, part) {
            return None;
        }
        return Some((frame_of(dom.get(part)?)? * frame_of(instance)?, None));
    }
    if !is_drawable(dom, database, parent) {
        return None;
    }
    Some((
        frame_of(instance)?,
        Some(size_of(instance).unwrap_or(Vec3::ZERO)),
    ))
}

/// A point and three unit axes in world space: the way a light shines, and
/// two across it.
#[derive(Debug, Clone, Copy)]
struct Basis {
    origin: Vec3,
    normal: Vec3,
    u: Vec3,
    v: Vec3,
}

impl Basis {
    /// `face`'s axes carried into `frame`, standing at its origin.
    fn of(frame: Mat4, face: NormalId) -> Basis {
        let (normal, u, v) = face.axes();
        let world = |axis: Vec3| frame.transform_vector3(axis).normalize_or_zero();
        Basis {
            origin: frame.w_axis.truncate(),
            normal: world(normal),
            u: world(u),
            v: world(v),
        }
    }
}

/// A `PointLight`'s reach: a great circle in each plane of `frame`'s axes.
fn sphere(frame: Mat4, range: f32) -> Vec<(Vec3, Vec3)> {
    let centre = frame.w_axis.truncate();
    let [x, y, z] = [frame.x_axis, frame.y_axis, frame.z_axis]
        .map(|axis| axis.truncate().normalize_or_zero() * range);
    let mut lines = Vec::with_capacity(3 * CIRCLE_SEGMENTS);
    for (a, b) in [(x, y), (y, z), (z, x)] {
        circle(centre, a, b, &mut lines);
    }
    lines
}

/// A cone from `basis.origin` along `basis.normal`, `half` either side of
/// it, capped by the sphere of radius `range`.
fn cone(basis: &Basis, range: f32, half: f32) -> Vec<(Vec3, Vec3)> {
    let apex = basis.origin;
    let mut lines = vec![(apex, apex + basis.normal * range)];
    let rim = apex + basis.normal * (range * half.cos());
    let radius = range * half.sin();
    if radius > MIN_RIM {
        let (u, v) = (basis.u * radius, basis.v * radius);
        lines.extend([u, -u, v, -v].map(|side| (apex, rim + side)));
        circle(rim, u, v, &mut lines);
    }
    lines
}

/// A face's reach: `basis` stands at the face's centre, and `extent` is
/// half its size along `basis.u` and `basis.v`.
fn frustum(basis: &Basis, extent: Vec2, range: f32, half: f32) -> Vec<(Vec3, Vec3)> {
    let far = basis.origin + basis.normal * (range * half.cos());
    let grown = extent + Vec2::splat(range * half.sin());
    let corner = |centre: Vec3, extent: Vec2, (a, b): (f32, f32)| {
        centre + basis.u * (a * extent.x) + basis.v * (b * extent.y)
    };
    // Round the rectangle, so each corner's successor is the next one along
    // an edge rather than across a diagonal.
    let signs = [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)];
    let near = signs.map(|sign| corner(basis.origin, extent, sign));
    let far_corners = signs.map(|sign| corner(far, grown, sign));

    let mut lines = vec![(basis.origin, basis.origin + basis.normal * range)];
    for index in 0..4 {
        lines.push((near[index], far_corners[index]));
        lines.push((far_corners[index], far_corners[(index + 1) % 4]));
    }
    lines
}

/// A circle about `centre` through `centre + u` and `centre + v`, as
/// straight pieces.
fn circle(centre: Vec3, u: Vec3, v: Vec3, lines: &mut Vec<(Vec3, Vec3)>) {
    let point = |index: usize| {
        let angle = TAU * index as f32 / CIRCLE_SEGMENTS as f32;
        centre + u * angle.cos() + v * angle.sin()
    };
    lines.extend((0..CIRCLE_SEGMENTS).map(|index| (point(index), point(index + 1))));
}

#[cfg(test)]
mod tests;
