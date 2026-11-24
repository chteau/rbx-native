//! The triangles and screen-space line quads an adornment is drawn from.
//!
//! Everything here is plain CPU geometry with no GPU handle, built once per
//! plan rather than per frame (the one exception is [`ring`], which faces
//! the camera and so is rebuilt with it). Every triangle is wound
//! counter-clockwise seen from outside the shape, so a single-sided
//! `HandleAdornment` (`DoubleSided = false`, see [`super::Adornments`]) can
//! drop the faces turned away from the eye. Each builder says which way is
//! out, and [`triangle`] turns the corners round to agree.

use glam::{Mat4, Vec2, Vec3};

use super::{ImageVertex, LineVertex, Vertex};
use crate::scene::{AdornMesh, AdornPicture};

/// How many segments go round a cylinder, a cone or a sphere's equator.
/// Matches `shapes::cylinder`'s own count, which already reads as round for
/// a part-sized shape.
const SEGMENTS: usize = 24;
/// How many bands a sphere is sliced into from pole to pole.
const BANDS: usize = 12;
/// Segments per full turn of an arc's ring, and round its tube.
const ARC_SEGMENTS: usize = 64;
const TUBE_SEGMENTS: usize = 8;
/// Segments round a camera-facing outline band.
const RING_SEGMENTS: usize = 64;

/// Appends one [`AdornMesh`], placed by `frame`, as triangles.
pub(super) fn solid(mesh: AdornMesh, frame: Mat4, color: [f32; 4], out: &mut Vec<Vertex>) {
    match mesh {
        AdornMesh::Box { size } => cuboid(frame, size, color, out),
        AdornMesh::Sphere { radius } => sphere(frame, radius, color, out),
        AdornMesh::Cone { radius, height } => cone(frame, radius, height, color, out),
        AdornMesh::Pyramid {
            sides,
            size,
            height,
        } => pyramid(frame, sides, size, height, color, out),
        AdornMesh::Cylinder {
            radius,
            inner,
            height,
            sweep,
        } => cylinder(frame, radius, inner, height, sweep, color, out),
        AdornMesh::Arc {
            radius,
            tube,
            sweep,
        } => arc(frame, radius, tube, sweep, color, out),
    }
}

/// A textured quad in the frame's own XY plane, an `ImageHandleAdornment`.
pub(super) fn picture_quad(picture: &AdornPicture, color: [f32; 4], out: &mut Vec<ImageVertex>) {
    let half = picture.size * 0.5;
    let corner = |x: f32, y: f32| picture.frame.transform_point3(Vec3::new(x, y, 0.0));
    let (a, b, c, d) = (
        corner(-half.x, -half.y),
        corner(half.x, -half.y),
        corner(half.x, half.y),
        corner(-half.x, half.y),
    );
    // V runs down the image, as every other image this renderer draws does.
    // Wound counter-clockwise seen from the frame's own -Z: Studio shows a
    // single-sided image only to an eye its `LookVector` points at (checked
    // 2026-11-20).
    let uv = |u: f32, v: f32| Vec2::new(u, v);
    for (position, texcoord) in [
        (a, uv(0.0, 1.0)),
        (c, uv(1.0, 0.0)),
        (b, uv(1.0, 1.0)),
        (a, uv(0.0, 1.0)),
        (d, uv(0.0, 0.0)),
        (c, uv(1.0, 0.0)),
    ] {
        out.push(ImageVertex {
            position: position.to_array(),
            uv: texcoord.to_array(),
            color,
            single_sided: if picture.double_sided { 0.0 } else { 1.0 },
        });
    }
}

/// One line as a screen-space quad, expanded to `pixels` across in the
/// vertex shader, the same trick `renderer::outline` uses for the
/// selection box, with the width and colour carried per vertex here because
/// an adornment picks both.
pub(in crate::renderer) fn line(
    from: Vec3,
    to: Vec3,
    pixels: f32,
    color: [f32; 4],
    out: &mut Vec<LineVertex>,
) {
    let half = (pixels * 0.5).max(0.5);
    let corner = |position: Vec3, other: Vec3, side: f32| LineVertex {
        position: position.to_array(),
        other: other.to_array(),
        side,
        half_width: half,
        color,
    };
    // A corner names the far end as its `other` and the far end names this
    // one, so both read the same on-screen direction; the sides mirror with
    // them, exactly as `renderer::outline`'s own edge quad does.
    out.extend([
        corner(from, to, 1.0),
        corner(from, to, -1.0),
        corner(to, from, -1.0),
        corner(from, to, -1.0),
        corner(to, from, 1.0),
        corner(to, from, -1.0),
    ]);
}

/// A `SelectionSphere`'s outline facing `eye`: the band between the
/// silhouettes of the spheres `inner` and `outer` studs round `centre`, laid
/// on the plane touching the outer sphere's near side, so the adornment's
/// own surface never covers it, with every radius scaled to project where
/// that sphere's silhouette does. From inside the outer sphere Studio draws
/// none of it.
pub(super) fn ring(
    centre: Vec3,
    inner: f32,
    outer: f32,
    color: [f32; 4],
    eye: Vec3,
    out: &mut Vec<Vertex>,
) {
    let to_eye = eye - centre;
    let distance = to_eye.length();
    if distance <= outer {
        return;
    }
    let normal = to_eye / distance;
    let aside = if normal.y.abs() < 0.99 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let u = aside.cross(normal).normalize();
    let v = normal.cross(u);
    let plane = centre + normal * outer;
    // A sphere `radius` across fills the cone of half-angle
    // asin(radius / distance) round the line to its centre.
    let reach =
        |radius: f32| (distance - outer) * radius / (distance * distance - radius * radius).sqrt();
    let (near, far) = (reach(inner), reach(outer));
    let point = |segment: usize, radius: f32| {
        let angle = std::f32::consts::TAU * segment as f32 / RING_SEGMENTS as f32;
        plane + (u * angle.cos() + v * angle.sin()) * radius
    };
    for segment in 0..RING_SEGMENTS {
        quad(
            out,
            color,
            point(segment, near),
            point(segment, far),
            point(segment + 1, far),
            point(segment + 1, near),
            normal,
        );
    }
}

fn cuboid(frame: Mat4, size: Vec3, color: [f32; 4], out: &mut Vec<Vertex>) {
    let half = size * 0.5;
    let centre = frame.w_axis.truncate();
    let corner = |x: f32, y: f32, z: f32| frame.transform_point3(Vec3::new(x, y, z) * half);
    let corners = [
        corner(-1.0, -1.0, -1.0),
        corner(1.0, -1.0, -1.0),
        corner(1.0, 1.0, -1.0),
        corner(-1.0, 1.0, -1.0),
        corner(-1.0, -1.0, 1.0),
        corner(1.0, -1.0, 1.0),
        corner(1.0, 1.0, 1.0),
        corner(-1.0, 1.0, 1.0),
    ];
    for face in [
        [0, 1, 2, 3],
        [5, 4, 7, 6],
        [4, 0, 3, 7],
        [1, 5, 6, 2],
        [3, 2, 6, 7],
        [4, 5, 1, 0],
    ] {
        let [a, b, c, d] = face.map(|index| corners[index]);
        quad(out, color, a, b, c, d, (a + c) * 0.5 - centre);
    }
}

fn sphere(frame: Mat4, radius: f32, color: [f32; 4], out: &mut Vec<Vertex>) {
    let point = |band: usize, segment: usize| {
        let phi = std::f32::consts::PI * band as f32 / BANDS as f32;
        let theta = std::f32::consts::TAU * segment as f32 / SEGMENTS as f32;
        frame.transform_point3(
            Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin()) * radius,
        )
    };
    let centre = frame.w_axis.truncate();
    for band in 0..BANDS {
        for segment in 0..SEGMENTS {
            let (a, c) = (point(band, segment), point(band + 1, segment + 1));
            quad(
                out,
                color,
                a,
                point(band, segment + 1),
                c,
                point(band + 1, segment),
                (a + c) * 0.5 - centre,
            );
        }
    }
}

/// Base on the frame's origin, apex `height` along its -Z.
fn cone(frame: Mat4, radius: f32, height: f32, color: [f32; 4], out: &mut Vec<Vertex>) {
    let apex = frame.transform_point3(Vec3::new(0.0, 0.0, -height));
    let base = frame.w_axis.truncate();
    let rim = |segment: usize| {
        let angle = std::f32::consts::TAU * segment as f32 / SEGMENTS as f32;
        frame.transform_point3(Vec3::new(angle.cos() * radius, angle.sin() * radius, 0.0))
    };
    let inside = frame.transform_point3(Vec3::new(0.0, 0.0, -height * 0.25));
    for segment in 0..SEGMENTS {
        let (a, b) = (rim(segment), rim(segment + 1));
        triangle(out, color, a, b, apex, (a + b + apex) / 3.0 - inside);
        triangle(out, color, b, a, base, base - inside);
    }
}

/// A `PyramidHandleAdornment`, laid out as Studio 0.748 draws one: the base
/// is a regular polygon centred on the frame's origin with one edge flat
/// along the bottom (-Y), sized so its height up the frame's Y is `size`
/// (an even polygon's inradius is half of it; an odd one's circumradius
/// and inradius add up to it), and the apex sits `height` along -Z.
fn pyramid(
    frame: Mat4,
    sides: u32,
    size: f32,
    height: f32,
    color: [f32; 4],
    out: &mut Vec<Vertex>,
) {
    let n = sides.max(3) as f32;
    let half_step = std::f32::consts::PI / n;
    let circumradius = if sides.is_multiple_of(2) {
        size * 0.5 / half_step.cos()
    } else {
        size / (1.0 + half_step.cos())
    };
    let corner = |k: u32| {
        let angle = -std::f32::consts::FRAC_PI_2 + half_step + 2.0 * half_step * k as f32;
        Vec3::new(angle.cos(), angle.sin(), 0.0) * circumradius
    };
    let apex = Vec3::new(0.0, 0.0, -height);
    // The base faces away from the apex. With no height at all, base and
    // sides lie in one plane facing opposite ways, so Studio's flat
    // pyramid shows from both sides even single-sided.
    let away = if height < 0.0 { -1.0 } else { 1.0 };
    let world = |local: Vec3| frame.transform_point3(local);
    let outward = |local: Vec3| frame.transform_vector3(local);
    for k in 0..sides {
        let (a, b) = (corner(k), corner(k + 1));
        let middle = (a + b) * 0.5;
        let inradius = middle.length();
        let side_out =
            middle / inradius.max(f32::EPSILON) * height.abs() - Vec3::Z * inradius * away;
        triangle(
            out,
            color,
            world(a),
            world(b),
            world(apex),
            outward(side_out),
        );
        triangle(
            out,
            color,
            world(b),
            world(a),
            world(Vec3::ZERO),
            outward(Vec3::Z * away),
        );
    }
}

/// From the frame's origin along its -Z. `inner` hollows it and `sweep`
/// (degrees) cuts it to a sector, both of which a `CylinderHandleAdornment`
/// can ask for.
fn cylinder(
    frame: Mat4,
    radius: f32,
    inner: f32,
    height: f32,
    sweep: f32,
    color: [f32; 4],
    out: &mut Vec<Vertex>,
) {
    let sweep = sweep.clamp(0.0, 360.0).to_radians();
    let segments = ((SEGMENTS as f32 * sweep / std::f32::consts::TAU).ceil() as usize).max(3);
    let at = |segment: usize, r: f32, z: f32| {
        let angle = sweep * segment as f32 / segments as f32;
        frame.transform_point3(Vec3::new(angle.cos() * r, angle.sin() * r, z))
    };
    // Which way is out, in the frame's own space: away from the axis on the
    // outer wall, towards it on the inner one, along the axis at the ends.
    let radial = |segment: f32| {
        let angle = sweep * segment / segments as f32;
        frame.transform_vector3(Vec3::new(angle.cos(), angle.sin(), 0.0))
    };
    let along = frame.transform_vector3(Vec3::Z);
    let hollow = inner > 0.0;
    for segment in 0..segments {
        let middle = radial(segment as f32 + 0.5);
        // Outer wall, then the inner one a hollow cylinder shows through.
        quad(
            out,
            color,
            at(segment, radius, 0.0),
            at(segment + 1, radius, 0.0),
            at(segment + 1, radius, -height),
            at(segment, radius, -height),
            middle,
        );
        if hollow {
            quad(
                out,
                color,
                at(segment, inner, 0.0),
                at(segment + 1, inner, 0.0),
                at(segment + 1, inner, -height),
                at(segment, inner, -height),
                -middle,
            );
        }
        // The two ends. A hollow one is an annulus, a solid one a fan.
        for z in [0.0, -height] {
            let outward = if z == 0.0 { along } else { -along };
            if hollow {
                quad(
                    out,
                    color,
                    at(segment, inner, z),
                    at(segment + 1, inner, z),
                    at(segment + 1, radius, z),
                    at(segment, radius, z),
                    outward,
                );
            } else {
                triangle(
                    out,
                    color,
                    frame.transform_point3(Vec3::new(0.0, 0.0, z)),
                    at(segment, radius, z),
                    at(segment + 1, radius, z),
                    outward,
                );
            }
        }
    }
    // A sector is open at the two cuts; close them so it reads as a solid.
    if sweep < std::f32::consts::TAU - 1e-3 {
        for segment in [0, segments] {
            // Out of the slice: back against the sweep at its start,
            // onwards with it at its end.
            let tangent = along.cross(radial(segment as f32));
            let outward = if segment == 0 { -tangent } else { tangent };
            quad(
                out,
                color,
                at(segment, inner, 0.0),
                at(segment, radius, 0.0),
                at(segment, radius, -height),
                at(segment, inner, -height),
                outward,
            );
        }
    }
}

/// A torus segment in the frame's XY plane, from +X sweeping `sweep`
/// degrees.
fn arc(frame: Mat4, radius: f32, tube: f32, sweep: f32, color: [f32; 4], out: &mut Vec<Vertex>) {
    let sweep = sweep.clamp(0.0, 360.0).to_radians();
    let segments = ((ARC_SEGMENTS as f32 * sweep / std::f32::consts::TAU).ceil() as usize).max(2);
    let point = |segment: usize, around: usize| {
        let angle = sweep * segment as f32 / segments as f32;
        let phi = std::f32::consts::TAU * around as f32 / TUBE_SEGMENTS as f32;
        let centre = Vec3::new(angle.cos(), angle.sin(), 0.0);
        let offset = centre * (phi.cos() * tube) + Vec3::Z * (phi.sin() * tube);
        frame.transform_point3(centre * radius + offset)
    };
    let spine = |segment: usize| {
        let angle = sweep * (segment as f32 + 0.5) / segments as f32;
        frame.transform_point3(Vec3::new(angle.cos(), angle.sin(), 0.0) * radius)
    };
    for segment in 0..segments {
        for around in 0..TUBE_SEGMENTS {
            let (a, c) = (point(segment, around), point(segment + 1, around + 1));
            quad(
                out,
                color,
                a,
                point(segment + 1, around),
                c,
                point(segment, around + 1),
                (a + c) * 0.5 - spine(segment),
            );
        }
    }
}

/// `a`, `b`, `c`, `d` in order round a flat quad's border, `outward` any
/// vector pointing out of the shape through it.
fn quad(out: &mut Vec<Vertex>, color: [f32; 4], a: Vec3, b: Vec3, c: Vec3, d: Vec3, outward: Vec3) {
    triangle(out, color, a, b, c, outward);
    triangle(out, color, a, c, d, outward);
}

/// One triangle, wound counter-clockwise seen from the side `outward`
/// points to.
fn triangle(out: &mut Vec<Vertex>, color: [f32; 4], a: Vec3, b: Vec3, c: Vec3, outward: Vec3) {
    let corners = if (b - a).cross(c - a).dot(outward) < 0.0 {
        [a, c, b]
    } else {
        [a, b, c]
    };
    for position in corners {
        out.push(Vertex {
            position: position.to_array(),
            color,
            single_sided: 0.0,
        });
    }
}

#[cfg(test)]
#[path = "geometry/tests.rs"]
mod tests;
