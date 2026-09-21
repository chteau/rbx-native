//! Studio's `targetMatrix`: the frame every ruler, and a free drag's grid and
//! soft snaps, are measured in (`DragHelper.getSurfaceMatrix`).
//!
//! Its origin is the corner of the face under the cursor nearest the cursor,
//! so a grid measured in it starts from that corner and runs along the face's
//! own edges — not from the world origin, and not along world axes. Every part
//! is taken as its box: Studio's own box, wedge and truss targets are, and
//! the ball, cylinder and mesh variants it has are left out.

use glam::{Mat4, Vec2, Vec3};

/// A face's nearest-corner frame: `y` is the face normal, `z` runs along the
/// face edge nearest the cursor and `x` across it. `x` and `z` may point out of
/// the face; whoever reads the frame finds the inside from the sign of the
/// cursor's own coordinates, as Studio does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SurfaceFrame {
    pub(crate) corner: Vec3,
    pub(crate) x: Vec3,
    pub(crate) y: Vec3,
    pub(crate) z: Vec3,
    /// The whole face's extent along `x` and along `z`.
    pub(crate) size: Vec2,
}

impl SurfaceFrame {
    /// `point` in this frame's own coordinates, Studio's `PointToObjectSpace`.
    pub(crate) fn local(&self, point: Vec3) -> Vec3 {
        let offset = point - self.corner;
        Vec3::new(offset.dot(self.x), offset.dot(self.y), offset.dot(self.z))
    }

    /// The inverse of [`SurfaceFrame::local`].
    pub(crate) fn world(&self, local: Vec3) -> Vec3 {
        self.corner + self.x * local.x + self.y * local.y + self.z * local.z
    }

    /// Where `point` lands on the face's plane, straight along its normal.
    pub(crate) fn onto_plane(&self, point: Vec3) -> Vec3 {
        point - self.y * (point - self.corner).dot(self.y)
    }
}

/// The frame of the face of `model`'s box nearest `hit` (Studio's
/// `getClosestFace`), cornered on the end nearest `hit` of the face edge
/// nearest `hit`. `None` for a box with no size to build axes from.
pub(crate) fn surface_frame(model: Mat4, hit: Vec3) -> Option<SurfaceFrame> {
    let centre = model.w_axis.truncate();
    let mut axes = [Vec3::ZERO; 3];
    let mut half = [0.0f32; 3];
    for (index, column) in [model.x_axis, model.y_axis, model.z_axis]
        .into_iter()
        .enumerate()
    {
        let column = column.truncate();
        axes[index] = column.try_normalize()?;
        half[index] = column.length() * 0.5;
    }
    let local = [0, 1, 2].map(|axis| (hit - centre).dot(axes[axis]));

    // The face whose plane is nearest the hit.
    let (face, side) = (0..3)
        .flat_map(|axis| [(axis, 1.0f32), (axis, -1.0)])
        .min_by(|&(a, sa), &(b, sb)| {
            (local[a] - sa * half[a])
                .abs()
                .total_cmp(&(local[b] - sb * half[b]).abs())
        })?;
    let normal = axes[face] * side;
    let (u, v) = ((face + 1) % 3, (face + 2) % 3);

    // The face's four edges each run along `u` or `v` at the other one's
    // extreme; the nearest is the one the hit is closest to across.
    let (across, along, edge_side) = [(u, v, 1.0f32), (u, v, -1.0), (v, u, 1.0), (v, u, -1.0)]
        .into_iter()
        .min_by(|&(a, _, sa), &(b, _, sb)| {
            (local[a] - sa * half[a])
                .abs()
                .total_cmp(&(local[b] - sb * half[b]).abs())
        })?;
    // Of that edge's two ends, the one nearer the hit.
    let end = if local[along] < 0.0 { -1.0 } else { 1.0 };
    let corner = centre
        + normal * half[face]
        + axes[across] * edge_side * half[across]
        + axes[along] * end * half[along];

    // Studio's `fromMatrix(corner, -(edge × n), n)`: z along the edge (here
    // pointing from the corner along it), x = n × z across it.
    let z = axes[along] * -end;
    let x = normal.cross(z);
    Some(SurfaceFrame {
        corner,
        x,
        y: normal,
        z,
        size: Vec2::new(half[across] * 2.0, half[along] * 2.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    /// An 8 × 1 × 4 slab with its top face at y = 0.5, centred on the origin.
    fn slab() -> Mat4 {
        Mat4::from_scale(Vec3::new(8.0, 1.0, 4.0))
    }

    #[test]
    fn the_corner_is_the_one_nearest_the_cursor_on_the_face_it_is_over() {
        let frame = surface_frame(slab(), Vec3::new(3.0, 0.5, 1.5)).unwrap();
        assert!(close(frame.y, Vec3::Y));
        assert!(close(frame.corner, Vec3::new(4.0, 0.5, 2.0)));

        let frame = surface_frame(slab(), Vec3::new(-3.0, 0.5, -1.2)).unwrap();
        assert!(close(frame.corner, Vec3::new(-4.0, 0.5, -2.0)));
    }

    #[test]
    fn z_runs_along_the_nearest_edge_and_the_size_is_the_whole_face() {
        // Half a stud from the +Z edge, three from the +X one: the edge along
        // X is nearer, so z runs along world X.
        let frame = surface_frame(slab(), Vec3::new(1.0, 0.5, 1.5)).unwrap();
        assert!(frame.z.dot(Vec3::X).abs() > 0.999, "z = {}", frame.z);
        assert!(frame.x.dot(Vec3::Z).abs() > 0.999, "x = {}", frame.x);
        assert!((frame.size.x - 4.0).abs() < 1e-5 && (frame.size.y - 8.0).abs() < 1e-5);
    }

    #[test]
    fn a_hit_is_on_the_face_plane_in_its_own_frame() {
        let hit = Vec3::new(1.0, 0.5, 1.5);
        let frame = surface_frame(slab(), hit).unwrap();
        let local = frame.local(hit);
        assert!(local.y.abs() < 1e-5);
        assert!(close(frame.world(local), hit));
        // The corner is at (4, 0.5, 2): the hit is 3 in along the edge and
        // half a stud in across it.
        assert!((local.z.abs() - 3.0).abs() < 1e-5 && (local.x.abs() - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_side_face_gets_its_own_normal() {
        let frame = surface_frame(slab(), Vec3::new(4.0, 0.2, 0.3)).unwrap();
        assert!(close(frame.y, Vec3::X));
        assert!((frame.corner.x - 4.0).abs() < 1e-5);
    }

    #[test]
    fn a_turned_part_is_measured_along_its_own_edges() {
        let turned = Mat4::from_rotation_y(0.3) * slab();
        let hit = turned.transform_point3(Vec3::new(0.3, 0.5, 0.2));
        let frame = surface_frame(turned, hit).unwrap();
        let x_axis = turned.x_axis.truncate().normalize();
        assert!(frame.z.dot(x_axis).abs() > 0.999 || frame.x.dot(x_axis).abs() > 0.999);
        assert!(frame.local(hit).y.abs() < 1e-4);
    }
}
