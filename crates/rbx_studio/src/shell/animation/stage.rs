//! The Animation Editor's stage, Blender's animation workspace: the bound
//! rig on its own, standing on a floor grid with the X (red) and Z (blue)
//! axes drawn through it, instead of the whole place around it. The grid is
//! a line layer in the 3D view; hiding the rest of the place is the
//! viewer's isolation (`rbx_viewer::Headless::set_isolation`).

use crate::tokens;
use glam::Vec3;
use rbx_viewer::Segment;

/// How far the grid reaches from its centre, in lines, and its spacing in
/// studs. The reach is the viewer's own: an isolated view fits its shadow
/// map to the rig and a floor this wide, so the grid is exactly the ground
/// the rig's shadow can land on.
const STEP: f32 = 1.0;
const REACH: i32 = (rbx_viewer::STAGE_REACH / STEP) as i32;
/// Every this many lines is drawn a little brighter, as Blender's are.
const MAJOR: i32 = 5;

/// Blender's grid greys, and its axis reds and blues, as linear RGBA. A
/// theme can change each one, so the active palette is read on each call.
fn grid_minor_color() -> [f32; 4] {
    tokens::linear(tokens::viewport_grid_minor())
}

fn grid_major_color() -> [f32; 4] {
    tokens::linear(tokens::viewport_grid_major())
}

fn grid_axis_x_color() -> [f32; 4] {
    tokens::linear(tokens::viewport_grid_axis_x())
}

fn grid_axis_z_color() -> [f32; 4] {
    tokens::linear(tokens::viewport_grid_axis_z())
}

/// The floor grid under a rig whose root stands over `centre` and whose
/// lowest point is at height `floor`. The two axis lines cross at the
/// rig's own feet, so a clip's root motion reads against them.
pub(super) fn grid(centre: Vec3, floor: f32) -> Vec<Segment> {
    let origin = Vec3::new(centre.x, floor, centre.z);
    let reach = REACH as f32 * STEP;
    let mut out = Vec::new();
    for i in -REACH..=REACH {
        let offset = i as f32 * STEP;
        let (colour, width) = match i {
            0 => continue,
            _ if i % MAJOR == 0 => (grid_major_color(), 1.25),
            _ => (grid_minor_color(), 1.0),
        };
        for (from, to) in [
            (
                Vec3::new(offset, 0.0, -reach),
                Vec3::new(offset, 0.0, reach),
            ),
            (
                Vec3::new(-reach, 0.0, offset),
                Vec3::new(reach, 0.0, offset),
            ),
        ] {
            out.push(segment(origin + from, origin + to, colour, width));
        }
    }
    // The axes last, so they draw over the grid lines they cross.
    out.push(segment(
        origin + Vec3::new(-reach, 0.0, 0.0),
        origin + Vec3::new(reach, 0.0, 0.0),
        grid_axis_x_color(),
        1.75,
    ));
    out.push(segment(
        origin + Vec3::new(0.0, 0.0, -reach),
        origin + Vec3::new(0.0, 0.0, reach),
        grid_axis_z_color(),
        1.75,
    ));
    out
}

fn segment(from: Vec3, to: Vec3, color: [f32; 4], width: f32) -> Segment {
    Segment {
        from,
        to,
        color,
        on_top: false,
        width,
    }
}

/// The lowest point of a set of upright boxes, `(centre, size)`: where the
/// rig's feet meet the floor.
pub(super) fn floor_of(boxes: impl IntoIterator<Item = (Vec3, Vec3)>) -> Option<f32> {
    boxes
        .into_iter()
        .map(|(centre, size)| centre.y - size.y * 0.5)
        .reduce(f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_lies_flat_at_the_feet_with_both_axes_through_the_root() {
        let lines = grid(Vec3::new(10.0, 3.0, -4.0), 0.5);
        assert!(lines.iter().all(|s| s.from.y == 0.5 && s.to.y == 0.5));
        // 40 lines each way besides the axes, and the two axes.
        assert_eq!(lines.len(), 2 * 40 + 2);
        let x = &lines[lines.len() - 2];
        assert_eq!((x.from.z, x.to.z), (-4.0, -4.0));
        assert_eq!(x.color, grid_axis_x_color());
        let z = &lines[lines.len() - 1];
        assert_eq!((z.from.x, z.to.x), (10.0, 10.0));
        assert_eq!(z.color, grid_axis_z_color());
    }

    #[test]
    fn the_floor_is_the_lowest_box_bottom() {
        let floor = floor_of([
            (Vec3::new(0.0, 3.0, 0.0), Vec3::new(2.0, 2.0, 1.0)),
            (Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 2.0, 1.0)),
        ]);
        assert_eq!(floor, Some(0.0));
        assert_eq!(floor_of([]), None);
    }
}
