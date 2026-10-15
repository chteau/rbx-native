//! Where a plugin's `Handles`, `ArcHandles` and `*HandleAdornment`s can be
//! grabbed, worked out with the same proportions `scene::adornment::handles`
//! draws them with, so what the cursor reaches is what the user sees.

use glam::{Mat4, Vec3};
use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::pick::Ray;

/// `HANDLE_UNIT`, `GAP`, `ARC_CLEARANCE` and `ARC_TUBE` in
/// `scene::adornment::handles`.
const HANDLE_UNIT: (f32, f32, f32) = (0.4, 1.0, 6.0);
const GAP: f32 = 0.15;
const ARC_CLEARANCE: f32 = 1.1;
const ARC_TUBE: f32 = 0.04;
/// `Enum.HandlesStyle.Movement`.
const STYLE_MOVEMENT: u32 = 1;
/// A ring's tube is thin on screen; the grab band around it is wider so it
/// can be hit without pixel hunting.
const RING_GRAB: f32 = 3.0;

/// What part of a plugin's handle the cursor is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// A `Handles` face: `Enum.NormalId` value.
    Face(u8),
    /// An `ArcHandles` ring: `Enum.Axis` value.
    Axis(u8),
    /// A `HandleAdornment`'s own body.
    Body,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Geometry {
    /// A segment with a radius round it (a lone sphere when `from == to`).
    Capsule { from: Vec3, to: Vec3, radius: f32 },
    /// A ring in the plane `normal` is perpendicular to.
    Ring {
        center: Vec3,
        normal: Vec3,
        radius: f32,
        tube: f32,
    },
    /// An oriented box: `frame` rigid, `half` its half extents.
    Box { frame: Mat4, half: Vec3 },
}

/// One grabbable piece of one plugin adornment, in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub referent: Ref,
    pub part: Part,
    pub geometry: Geometry,
    /// The adornee's rigid frame, which drags measure along.
    pub frame: Mat4,
}

/// `NormalId` order: Right, Top, Back, Left, Bottom, Front.
pub fn face_normal(face: u8) -> Vec3 {
    match face {
        0 => Vec3::X,
        1 => Vec3::Y,
        2 => Vec3::Z,
        3 => -Vec3::X,
        4 => -Vec3::Y,
        _ => -Vec3::Z,
    }
}

/// The grabbable shapes of every adornment in `adornments`, placed against
/// the `BasePart` adornee they name in `dom`. Adornments without a part
/// adornee have nothing to stand on and are skipped.
pub fn shapes(dom: &WeakDom, database: &ReflectionDatabase, adornments: &[Instance]) -> Vec<Shape> {
    adornments
        .iter()
        .flat_map(|adornment| {
            let Some(Variant::Ref(adornee)) = adornment.properties().get("Adornee") else {
                return Vec::new();
            };
            let Some((frame, size)) = dom.get(*adornee).and_then(part_frame) else {
                return Vec::new();
            };
            shapes_of(database, adornment, frame, size)
        })
        .collect()
}

/// A part's rigid frame and size, from its `CFrame` and `size`.
pub fn part_frame(part: &Instance) -> Option<(Mat4, Vec3)> {
    let properties = part.properties();
    let Some(Variant::CFrame(cframe)) = properties.get("CFrame") else {
        return None;
    };
    let size = match properties.get("size").or_else(|| properties.get("Size")) {
        Some(Variant::Vector3(v)) => Vec3::new(v.x, v.y, v.z),
        _ => return None,
    };
    let r = cframe.rotation;
    let p = &cframe.position;
    let frame = Mat4::from_cols(
        Vec3::new(r[0], r[3], r[6]).extend(0.0),
        Vec3::new(r[1], r[4], r[7]).extend(0.0),
        Vec3::new(r[2], r[5], r[8]).extend(0.0),
        Vec3::new(p.x, p.y, p.z).extend(1.0),
    );
    Some((frame, size))
}

/// One adornment's shapes against an adornee at `frame` of `size`.
pub fn shapes_of(
    database: &ReflectionDatabase,
    adornment: &Instance,
    frame: Mat4,
    size: Vec3,
) -> Vec<Shape> {
    let class = adornment.class();
    let properties = adornment.properties();
    let referent = adornment.referent();
    let shape = |part, geometry| Shape {
        referent,
        part,
        geometry,
        frame,
    };
    if database.is_subclass_of(class, "Handles") {
        let Some(Variant::Faces(faces)) = properties.get("Faces") else {
            return Vec::new();
        };
        let movement = matches!(properties.get("Style"), Some(&Variant::Enum(STYLE_MOVEMENT)));
        let enabled = [
            faces.right,
            faces.top,
            faces.back,
            faces.left,
            faces.bottom,
            faces.front,
        ];
        return face_handles(enabled, movement, frame, size)
            .into_iter()
            .map(|(face, geometry)| shape(Part::Face(face), geometry))
            .collect();
    }
    if database.is_subclass_of(class, "ArcHandles") {
        let Some(Variant::Axes(axes)) = properties.get("Axes") else {
            return Vec::new();
        };
        return arc_handles([axes.x, axes.y, axes.z], frame, size)
            .into_iter()
            .map(|(axis, geometry)| shape(Part::Axis(axis), geometry))
            .collect();
    }
    if database.is_subclass_of(class, "HandleAdornment") {
        return vec![shape(
            Part::Body,
            Geometry::Box {
                frame,
                half: size * 0.5,
            },
        )];
    }
    Vec::new()
}

fn unit(size: Vec3) -> f32 {
    let (fraction, min, max) = HANDLE_UNIT;
    (size.max_element() * fraction).clamp(min, max)
}

/// `Handles`: a sphere (Resize) or a shaft-and-cone arrow (Movement) off
/// each enabled face, as `handles::handles` builds them.
pub fn face_handles(enabled: [bool; 6], movement: bool, frame: Mat4, size: Vec3) -> Vec<(u8, Geometry)> {
    let unit = unit(size);
    (0u8..6)
        .filter(|&face| enabled[face as usize])
        .map(|face| {
            let normal = face_normal(face);
            let base = normal * ((size * normal.abs()).length() * 0.5 + unit * GAP);
            let from = frame.transform_point3(base);
            let geometry = if movement {
                Geometry::Capsule {
                    from,
                    to: frame.transform_point3(base + normal * unit * 1.2),
                    radius: unit * 0.28,
                }
            } else {
                Geometry::Capsule {
                    from,
                    to: from,
                    radius: unit * 0.3,
                }
            };
            (face, geometry)
        })
        .collect()
}

/// `ArcHandles`: one ring per enabled axis, as `handles::arc_handles`.
pub fn arc_handles(enabled: [bool; 3], frame: Mat4, size: Vec3) -> Vec<(u8, Geometry)> {
    let half = size * 0.5;
    (0u8..3)
        .filter(|&axis| enabled[axis as usize])
        .map(|axis| {
            let i = axis as usize;
            let (a, b) = ((i + 1) % 3, (i + 2) % 3);
            let radius = (half[a] * half[a] + half[b] * half[b]).sqrt() * ARC_CLEARANCE;
            let mut normal = Vec3::ZERO;
            normal[i] = 1.0;
            (
                axis,
                Geometry::Ring {
                    center: frame.transform_point3(Vec3::ZERO),
                    normal: frame.transform_vector3(normal).normalize_or_zero(),
                    radius,
                    tube: radius * ARC_TUBE,
                },
            )
        })
        .collect()
}

/// How far along `ray` it meets `geometry`, if it does.
pub fn hit(geometry: &Geometry, ray: Ray) -> Option<f32> {
    match *geometry {
        Geometry::Capsule { from, to, radius } => capsule(from, to, radius, ray),
        Geometry::Ring {
            center,
            normal,
            radius,
            tube,
        } => {
            let slope = ray.direction.dot(normal);
            if slope.abs() < 1e-4 {
                return None;
            }
            let t = (center - ray.origin).dot(normal) / slope;
            if t <= 0.0 {
                return None;
            }
            let from_center = (ray.at(t) - center).length();
            ((from_center - radius).abs() <= tube * RING_GRAB).then_some(t)
        }
        Geometry::Box { frame, half } => {
            let inverse = frame.inverse();
            let origin = inverse.transform_point3(ray.origin);
            let direction = inverse.transform_vector3(ray.direction);
            let (mut near, mut far) = (f32::NEG_INFINITY, f32::INFINITY);
            for i in 0..3 {
                if direction[i].abs() < 1e-8 {
                    if origin[i].abs() > half[i] {
                        return None;
                    }
                    continue;
                }
                let a = (-half[i] - origin[i]) / direction[i];
                let b = (half[i] - origin[i]) / direction[i];
                near = near.max(a.min(b));
                far = far.min(a.max(b));
            }
            (near <= far && far >= 0.0).then_some(near.max(0.0))
        }
    }
}

fn capsule(from: Vec3, to: Vec3, radius: f32, ray: Ray) -> Option<f32> {
    let segment = to - from;
    let length = segment.length_squared();
    let w = ray.origin - from;
    let d = ray.direction;
    let (t, s) = if length < 1e-8 {
        (-w.dot(d), 0.0)
    } else {
        let b = d.dot(segment);
        let denominator = length - b * b;
        let s = if denominator.abs() < 1e-8 {
            0.0
        } else {
            ((segment.dot(w) - b * d.dot(w)) / denominator).clamp(0.0, 1.0)
        };
        ((from + segment * s - ray.origin).dot(d), s)
    };
    if t < 0.0 {
        return None;
    }
    let closest = from + segment * s;
    let gap = (ray.at(t) - closest).length();
    (gap <= radius).then_some((t - (radius * radius - gap * gap).sqrt()).max(0.0))
}

/// The nearest shape `ray` hits: which adornment and which part of it.
pub fn pick(shapes: &[Shape], ray: Ray) -> Option<(Ref, Part)> {
    pick_shape(shapes, ray).map(|shape| (shape.referent, shape.part))
}

/// [`pick`], keeping the whole shape for the drag that follows.
pub fn pick_shape(shapes: &[Shape], ray: Ray) -> Option<&Shape> {
    shapes
        .iter()
        .filter_map(|shape| hit(&shape.geometry, ray).map(|t| (shape, t)))
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(shape, _)| shape)
}

#[cfg(test)]
mod tests {
    use super::{arc_handles, face_handles, hit, pick, Geometry, Part, Shape};
    use crate::pick::Ray;
    use glam::{Mat4, Vec3};
    use rbx_dom::Ref;

    fn ray(origin: Vec3, toward: Vec3) -> Ray {
        Ray {
            origin,
            direction: (toward - origin).normalize(),
        }
    }

    fn shapes(list: Vec<(u8, Geometry)>, referent: Ref, part: fn(u8) -> Part) -> Vec<Shape> {
        list.into_iter()
            .map(|(i, geometry)| Shape {
                referent,
                part: part(i),
                geometry,
                frame: Mat4::IDENTITY,
            })
            .collect()
    }

    #[test]
    fn picks_the_top_sphere() {
        let size = Vec3::splat(4.0);
        let list = face_handles([true; 6], false, Mat4::IDENTITY, size);
        assert_eq!(list.len(), 6);
        let all = shapes(list, Ref::new(7), Part::Face);
        // unit = 1.6, top sphere centre at y = 2 + 0.24.
        let r = ray(Vec3::new(0.0, 20.0, 0.0), Vec3::new(0.0, 2.24, 0.0));
        assert_eq!(pick(&all, r), Some((Ref::new(7), Part::Face(1))));
        let miss = ray(Vec3::new(10.0, 20.0, 10.0), Vec3::new(10.0, 0.0, 10.0));
        assert_eq!(pick(&all, miss), None);
    }

    #[test]
    fn arrow_reaches_out_along_the_face() {
        let list = face_handles([true, false, false, false, false, false], true, Mat4::IDENTITY, Vec3::splat(4.0));
        let Geometry::Capsule { to, .. } = list[0].1 else {
            panic!("arrow is a capsule");
        };
        assert!(to.x > 2.24);
        let r = ray(Vec3::new(3.0, 10.0, 0.0), Vec3::new(3.0, 0.0, 0.0));
        assert!(hit(&list[0].1, r).is_some());
    }

    #[test]
    fn ring_is_hit_on_its_band_only() {
        let list = arc_handles([false, true, false], Mat4::IDENTITY, Vec3::new(2.0, 2.0, 2.0));
        let Geometry::Ring { radius, .. } = list[0].1 else {
            panic!("arc is a ring");
        };
        let on = ray(Vec3::new(radius, 10.0, 0.0), Vec3::new(radius, 0.0, 0.0));
        let all = shapes(list, Ref::new(3), Part::Axis);
        assert_eq!(pick(&all, on), Some((Ref::new(3), Part::Axis(1))));
        let centre = ray(Vec3::new(0.0, 10.0, 0.0), Vec3::ZERO);
        assert_eq!(pick(&all, centre), None);
    }

    #[test]
    fn nearest_shape_wins() {
        let near = Shape {
            referent: Ref::new(1),
            part: Part::Body,
            geometry: Geometry::Box {
                frame: Mat4::from_translation(Vec3::new(0.0, 0.0, 5.0)),
                half: Vec3::ONE,
            },
            frame: Mat4::IDENTITY,
        };
        let far = Shape {
            referent: Ref::new(2),
            ..near
        };
        let far = Shape {
            geometry: Geometry::Box {
                frame: Mat4::IDENTITY,
                half: Vec3::ONE,
            },
            ..far
        };
        let r = ray(Vec3::new(0.0, 0.0, 20.0), Vec3::ZERO);
        assert_eq!(pick(&[far, near], r), Some((Ref::new(1), Part::Body)));
    }
}
