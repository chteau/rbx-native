//! What the editor draws over the viewport: the brush's wireframe where it
//! would land, and the selection region with its handles.

use crate::tokens;
use glam::{Mat4, Vec3};
use rbx_terrain::edit::brush::Shape;
use rbx_terrain::edit::region::StudBox;
use rbx_viewer::gizmo::{Axis, Faces, Handles};
use rbx_viewer::Segment;

use super::settings::Settings;

const RING: usize = 48;
const LINE: f32 = 2.0;
/// Studio's terrain brush is a light blue wire. The active theme's add
/// colour, as linear RGBA, read on each call so a theme can change it.
fn brush_add_color() -> [f32; 4] {
    tokens::linear(tokens::viewport_brush_add())
}

/// Subtracting turns the brush red so a held `Ctrl` reads at a glance. The
/// active theme's subtract colour, read on each call.
fn brush_subtract_color() -> [f32; 4] {
    tokens::linear(tokens::viewport_brush_subtract())
}

/// The selection region's box. The active theme's region colour, read on
/// each call.
fn brush_region_color() -> [f32; 4] {
    tokens::linear(tokens::viewport_brush_region())
}
const HANDLE_DOT: f32 = 14.0;

fn segment(from: Vec3, to: Vec3, color: [f32; 4]) -> Segment {
    Segment {
        from,
        to,
        color,
        on_top: true,
        width: LINE,
    }
}

fn ring(center: Vec3, u: Vec3, v: Vec3, radius: f32, color: [f32; 4], out: &mut Vec<Segment>) {
    let point = |i: usize| {
        let a = i as f32 / RING as f32 * std::f32::consts::TAU;
        center + (u * a.cos() + v * a.sin()) * radius
    };
    out.extend((0..RING).map(|i| segment(point(i), point(i + 1), color)));
}

fn box_edges(corners: [Vec3; 8], color: [f32; 4], out: &mut Vec<Segment>) {
    // Corner i has bit 0 = +x, bit 1 = +y, bit 2 = +z.
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                out.push(segment(corners[i], corners[i | bit], color));
            }
        }
    }
}

/// The brush's outline centred on `center`: three great circles for a
/// sphere, the twelve edges of a box, two rims and four struts for a
/// cylinder.
pub(crate) fn brush_outline(settings: &Settings, center: Vec3, subtract: bool) -> Vec<Segment> {
    let color = if subtract {
        brush_subtract_color()
    } else {
        brush_add_color()
    };
    let radius = settings.size * 0.5;
    let half_height = settings.brush_height() * 0.5;
    let mut out = Vec::new();
    match settings.shape {
        Shape::Sphere => {
            ring(center, Vec3::X, Vec3::Z, radius, color, &mut out);
            ring(center, Vec3::X, Vec3::Y, radius, color, &mut out);
            ring(center, Vec3::Y, Vec3::Z, radius, color, &mut out);
        }
        Shape::Box => {
            let half = Vec3::new(radius, half_height, radius);
            let corners = std::array::from_fn(|i| {
                center + half * Vec3::new(sign(i, 1), sign(i, 2), sign(i, 4))
            });
            box_edges(corners, color, &mut out);
        }
        Shape::Cylinder => {
            for y in [-half_height, half_height] {
                ring(
                    center + Vec3::Y * y,
                    Vec3::X,
                    Vec3::Z,
                    radius,
                    color,
                    &mut out,
                );
            }
            for direction in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
                let foot = center + direction * radius;
                out.push(segment(
                    foot - Vec3::Y * half_height,
                    foot + Vec3::Y * half_height,
                    color,
                ));
            }
        }
    }
    out
}

fn sign(index: usize, bit: usize) -> f32 {
    if index & bit == 0 {
        -1.0
    } else {
        1.0
    }
}

/// The region's placement as a model matrix: its size folded into the
/// columns, turned by `rotation`, the way a part's box is.
pub(crate) fn region_model(region: &StudBox, rotation: glam::Mat3) -> Mat4 {
    let size = Vec3::from(region.size());
    Mat4::from_cols(
        (rotation.x_axis * size.x).extend(0.0),
        (rotation.y_axis * size.y).extend(0.0),
        (rotation.z_axis * size.z).extend(0.0),
        Vec3::from(region.center()).extend(1.0),
    )
}

/// The region's box, and the handles the tool grabs it by: a ball on each
/// face to scale it, and — for Transform — the three axis arrows to move it.
pub(crate) fn region_outline(
    model: Mat4,
    faces: Option<&Faces>,
    handles: Option<&Handles>,
) -> Vec<Segment> {
    let corners = std::array::from_fn(|i| {
        model.transform_point3(Vec3::new(sign(i, 1), sign(i, 2), sign(i, 4)) * 0.5)
    });
    let mut out = Vec::new();
    box_edges(corners, brush_region_color(), &mut out);
    if let Some(faces) = faces {
        for (axis, side) in faces.all() {
            let at = faces.handle(axis, side);
            out.push(Segment {
                from: at,
                to: at,
                color: axis_color(axis),
                on_top: true,
                width: HANDLE_DOT,
            });
        }
    }
    if let Some(handles) = handles {
        // Transform's rotate rings, where `Handles::grab_ring` looks for
        // them: one arm out from the centre, round each axis.
        for axis in Axis::ALL {
            let (_, zero, quarter) = handles.ring_frame(axis);
            ring(
                handles.origin(),
                zero,
                quarter,
                handles.arm(),
                axis_color(axis),
                &mut out,
            );
        }
        for axis in Axis::ALL {
            let tip = handles.origin() + handles.direction(axis) * handles.arm();
            out.push(Segment {
                from: handles.origin(),
                to: tip,
                color: axis_color(axis),
                on_top: true,
                width: 4.0,
            });
            out.push(Segment {
                from: tip,
                to: tip,
                color: axis_color(axis),
                on_top: true,
                width: HANDLE_DOT * 0.8,
            });
        }
    }
    out
}

fn axis_color(axis: Axis) -> [f32; 4] {
    let [r, g, b] = axis.color();
    [r, g, b, 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_shape_draws_its_own_wire() {
        let mut settings = Settings::default();
        let sphere = brush_outline(&settings, Vec3::ZERO, false);
        assert_eq!(sphere.len(), 3 * RING);
        assert!(sphere.iter().all(|s| (s.from.length() - 4.0).abs() < 1e-4));
        settings.shape = Shape::Box;
        assert_eq!(brush_outline(&settings, Vec3::ZERO, false).len(), 12);
        settings.shape = Shape::Cylinder;
        let cylinder = brush_outline(&settings, Vec3::ZERO, true);
        assert_eq!(cylinder.len(), 2 * RING + 4);
        assert_eq!(cylinder[0].color, brush_subtract_color());
    }

    #[test]
    fn the_region_box_has_its_corners_where_the_region_is() {
        let region = StudBox::from_center_size([10.0, 0.0, 0.0], [4.0, 8.0, 12.0]);
        let model = region_model(&region, glam::Mat3::IDENTITY);
        let lines = region_outline(model, None, None);
        assert_eq!(lines.len(), 12);
        let max_x = lines
            .iter()
            .map(|s| s.from.x.max(s.to.x))
            .fold(f32::MIN, f32::max);
        assert!((max_x - 12.0).abs() < 1e-5);
    }
}
