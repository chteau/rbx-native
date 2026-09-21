//! The triangles and screen-space line quads an adornment is drawn from.
//!
//! Everything here is plain CPU geometry with no GPU handle, built once per
//! plan rather than per frame (the one exception is [`ring`], which faces
//! the camera and so is rebuilt with it). Nothing is culled by winding — see
//! [`super::Adornments`] for why — so these builders only have to put the
//! corners in the right places, not agree on an order.

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
/// Segments in a camera-facing outline circle.
const RING_SEGMENTS: usize = 64;

/// Appends one [`AdornMesh`], placed by `frame`, as triangles.
pub(super) fn solid(mesh: AdornMesh, frame: Mat4, color: [f32; 4], out: &mut Vec<Vertex>) {
    match mesh {
        AdornMesh::Box { size } => cuboid(frame, size, color, out),
        AdornMesh::Sphere { radius } => sphere(frame, radius, color, out),
        AdornMesh::Cone { radius, height } => cone(frame, radius, height, color, out),
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

/// A textured quad in the frame's own XY plane — an `ImageHandleAdornment`.
pub(super) fn picture_quad(picture: &AdornPicture, alpha: f32, out: &mut Vec<ImageVertex>) {
    let half = picture.size * 0.5;
    let corner = |x: f32, y: f32| picture.frame.transform_point3(Vec3::new(x, y, 0.0));
    let (a, b, c, d) = (
        corner(-half.x, -half.y),
        corner(half.x, -half.y),
        corner(half.x, half.y),
        corner(-half.x, half.y),
    );
    // V runs down the image, as every other image this renderer draws does.
    let uv = |u: f32, v: f32| Vec2::new(u, v);
    for (position, texcoord) in [
        (a, uv(0.0, 1.0)),
        (b, uv(1.0, 1.0)),
        (c, uv(1.0, 0.0)),
        (a, uv(0.0, 1.0)),
        (c, uv(1.0, 0.0)),
        (d, uv(0.0, 0.0)),
    ] {
        out.push(ImageVertex {
            position: position.to_array(),
            uv: texcoord.to_array(),
            alpha,
        });
    }
}

/// One line as a screen-space quad, expanded to `pixels` across in the
/// vertex shader — the same trick `renderer::outline` uses for the
/// selection box, with the width and colour carried per vertex here because
/// an adornment picks both.
pub(super) fn line(from: Vec3, to: Vec3, pixels: f32, color: [f32; 4], out: &mut Vec<LineVertex>) {
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

/// A circle of `radius` around `centre`, drawn in the plane facing `eye` —
/// the silhouette a `SelectionSphere` outlines itself with.
pub(super) fn ring(
    centre: Vec3,
    radius: f32,
    pixels: f32,
    color: [f32; 4],
    eye: Vec3,
    out: &mut Vec<LineVertex>,
) {
    let normal = (eye - centre).normalize_or_zero();
    let normal = if normal.length_squared() < 0.5 {
        Vec3::Z
    } else {
        normal
    };
    let aside = if normal.y.abs() < 0.99 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let u = aside.cross(normal).normalize_or_zero() * radius;
    let v = normal.cross(u.normalize_or_zero()) * radius;

    let point = |segment: usize| {
        let angle = std::f32::consts::TAU * segment as f32 / RING_SEGMENTS as f32;
        centre + u * angle.cos() + v * angle.sin()
    };
    for segment in 0..RING_SEGMENTS {
        line(point(segment), point(segment + 1), pixels, color, out);
    }
}

fn cuboid(frame: Mat4, size: Vec3, color: [f32; 4], out: &mut Vec<Vertex>) {
    let half = size * 0.5;
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
        quad(
            out,
            color,
            corners[face[0]],
            corners[face[1]],
            corners[face[2]],
            corners[face[3]],
        );
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
    for band in 0..BANDS {
        for segment in 0..SEGMENTS {
            quad(
                out,
                color,
                point(band, segment),
                point(band, segment + 1),
                point(band + 1, segment + 1),
                point(band + 1, segment),
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
    for segment in 0..SEGMENTS {
        triangle(out, color, rim(segment), rim(segment + 1), apex);
        triangle(out, color, rim(segment + 1), rim(segment), base);
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
    let hollow = inner > 0.0;
    for segment in 0..segments {
        // Outer wall, then the inner one a hollow cylinder shows through.
        quad(
            out,
            color,
            at(segment, radius, 0.0),
            at(segment + 1, radius, 0.0),
            at(segment + 1, radius, -height),
            at(segment, radius, -height),
        );
        if hollow {
            quad(
                out,
                color,
                at(segment, inner, 0.0),
                at(segment + 1, inner, 0.0),
                at(segment + 1, inner, -height),
                at(segment, inner, -height),
            );
        }
        // The two ends. A hollow one is an annulus, a solid one a fan.
        for z in [0.0, -height] {
            if hollow {
                quad(
                    out,
                    color,
                    at(segment, inner, z),
                    at(segment + 1, inner, z),
                    at(segment + 1, radius, z),
                    at(segment, radius, z),
                );
            } else {
                triangle(
                    out,
                    color,
                    frame.transform_point3(Vec3::new(0.0, 0.0, z)),
                    at(segment, radius, z),
                    at(segment + 1, radius, z),
                );
            }
        }
    }
    // A sector is open at the two cuts; close them so it reads as a solid.
    if sweep < std::f32::consts::TAU - 1e-3 {
        for segment in [0, segments] {
            quad(
                out,
                color,
                at(segment, inner, 0.0),
                at(segment, radius, 0.0),
                at(segment, radius, -height),
                at(segment, inner, -height),
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
    for segment in 0..segments {
        for around in 0..TUBE_SEGMENTS {
            quad(
                out,
                color,
                point(segment, around),
                point(segment + 1, around),
                point(segment + 1, around + 1),
                point(segment, around + 1),
            );
        }
    }
}

fn quad(out: &mut Vec<Vertex>, color: [f32; 4], a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
    triangle(out, color, a, b, c);
    triangle(out, color, a, c, d);
}

fn triangle(out: &mut Vec<Vertex>, color: [f32; 4], a: Vec3, b: Vec3, c: Vec3) {
    for position in [a, b, c] {
        out.push(Vertex {
            position: position.to_array(),
            color,
        });
    }
}

#[cfg(test)]
#[path = "geometry/tests.rs"]
mod tests;
