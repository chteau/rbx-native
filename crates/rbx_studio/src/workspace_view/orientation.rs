//! The viewport's orientation indicator: pure math for the up-to-three cube
//! faces the camera's current pose puts on the near side, and where each
//! one's four corners and label land inside the widget.
//!
//! An rbx-native addition, not a Roblox Studio one — see the `ROADMAP.md`
//! bullet this implements. Still a flat 2D overlay, not a literal 3D cube
//! mesh in the scene: each face is the plain orthographic projection of a
//! world-axis-aligned unit cube's square face onto the camera's own basis,
//! which is exactly what an isometric-style flat drawing of a cube already
//! looks like — no perspective divide, no WGPU render pass. `workspace_view
//! .rs`'s `Render` impl turns [`visible_faces`]'s answer into the actual
//! GPUI elements (`PathBuilder`-filled polygons plus a label per face), the
//! same split `label.rs` keeps between formatting and layout.

use glam::Vec3;
use rbx_viewer::Pose;

// Studio's own red/green/blue X/Y/Z convention — the same sRGB source values
// `rbx_viewer::gizmo::Axis::color`'s doc comment documents (that copy is
// linearized for the HDR render pipeline; this one is plain sRGB, since GPUI
// paints this overlay directly rather than through the scene's tonemap).
const COLOR_X: u32 = 0xe62929;
const COLOR_Y: u32 = 0x29c738;
const COLOR_Z: u32 = 0x335cf2;

/// One world axis's pair of opposite faces, and the square they each span.
///
/// `a`/`b` are the *other* two world axes — the ones a face spans across,
/// found the same way `rbx_viewer::gizmo::Axis::next` cycles X→Y→Z→X, just
/// inlined as data instead of a method, since this table only ever needs
/// the two axes *other than* its own.
struct AxisFaces {
    axis: Vec3,
    a: Vec3,
    b: Vec3,
    /// Which world axis each label names — an rbx-native convention, not a
    /// Roblox one: positionally paired the way the `ROADMAP.md` bullet
    /// itself lists them (`+X`/Right, `-X`/Left, `+Y`/Top, `-Y`/Bottom,
    /// `+Z`/Front, `-Z`/Back), not Roblox's own `CFrame.LookVector` (`-Z`).
    positive: &'static str,
    negative: &'static str,
    color: u32,
}

const AXIS_FACES: [AxisFaces; 3] = [
    AxisFaces {
        axis: Vec3::X,
        a: Vec3::Y,
        b: Vec3::Z,
        positive: "Right",
        negative: "Left",
        color: COLOR_X,
    },
    AxisFaces {
        axis: Vec3::Y,
        a: Vec3::Z,
        b: Vec3::X,
        positive: "Top",
        negative: "Bottom",
        color: COLOR_Y,
    },
    AxisFaces {
        axis: Vec3::Z,
        a: Vec3::X,
        b: Vec3::Y,
        positive: "Front",
        negative: "Back",
        color: COLOR_Z,
    },
];

/// One cube face: which one, where its four corners land, and where its
/// label sits.
#[derive(Debug, Clone, Copy)]
pub(super) struct Face {
    pub(super) label: &'static str,
    pub(super) color: u32,
    /// The face's four corners, in cyclic (non-self-intersecting) order, on
    /// a unit-radius screen scale — the caller scales by the widget's
    /// actual on-screen radius. `x` is screen-right, `y` is screen-down
    /// (GPUI's own convention, not world space).
    pub(super) corners: [(f32, f32); 4],
    /// The face's centre — exactly the mean of `corners`, since a linear
    /// projection of a square's centre is the mean of its projected
    /// corners; kept as its own field so a caller placing a label never has
    /// to average `corners` itself.
    pub(super) center: (f32, f32),
    /// How square-on this face is to the camera, from `0.0` (perfectly
    /// edge-on — see `visible_faces`'s doc on the degenerate case) to `1.0`
    /// (dead centre, filling the indicator). A face's own fill already reads
    /// this from its `corners`' shrinking area for free; a caller's fixed-
    /// size *label* does not; without dimming it a near-edge-on face still
    /// shows its full-size, full-opacity name floating over a sliver too
    /// thin to justify it.
    pub(super) prominence: f32,
}

/// The three cube faces — one per world axis — facing the camera at this
/// pose, ready to lay out around the indicator's own centre.
///
/// Always exactly three, one per axis, never fewer: at an axis-aligned view
/// (looking exactly down one world axis) the other two axes' faces are
/// edge-on and project to a degenerate zero-area sliver (their four corners
/// collapse to two — see this module's tests) rather than vanishing outright,
/// which reads the same as "not there" once painted but needs no special
/// case to produce.
///
/// Depends only on `pose`'s rotation (`Pose::basis`), never its position or
/// `ortho_scale` — so, deliberately unlike most of this camera's own state,
/// this projection needs no special-casing for orthographic mode: an
/// orthographic camera's basis is exactly as meaningful as a perspective
/// one's, since both share the same notion of "which way is it looking".
///
/// "Facing the camera" means roughly aligned with the camera's own forward
/// vector, not "closer to the viewer" in the literal ViewCube sense —
/// looking straight down `-Z` puts `-Z` ("Back") dead ahead and `+Z`
/// ("Front") behind it, so the `Back` face fills the indicator square-on and
/// `Front` doesn't appear. That reads oddly next to the label's English
/// meaning, but it is what "which face am I currently looking toward" has
/// to mean for a fixed label-to-axis pairing with no free camera-relative
/// renaming — and this widget is an original rbx-native convention to begin
/// with, not a claim about how Roblox or any other tool labels its own cube.
pub(super) fn visible_faces(pose: Pose) -> [Face; 3] {
    let (forward, right, up) = pose.basis();
    let project = |world: Vec3| (world.dot(right), -world.dot(up));

    AXIS_FACES.map(|group| {
        let alignment = group.axis.dot(forward);
        let front = alignment > 0.0;
        let normal = if front { group.axis } else { -group.axis };
        let label = if front {
            group.positive
        } else {
            group.negative
        };

        Face {
            label,
            color: group.color,
            corners: [
                project(normal + group.a + group.b),
                project(normal + group.a - group.b),
                project(normal - group.a - group.b),
                project(normal - group.a + group.b),
            ],
            prominence: alignment.abs(),
            center: project(normal),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(yaw: f32, pitch: f32) -> Pose {
        Pose {
            position: Vec3::ZERO,
            yaw,
            pitch,
            fov_degrees: 70.0,
            ortho_scale: 1.0,
        }
    }

    fn face<'a>(faces: &'a [Face; 3], label: &str) -> &'a Face {
        faces
            .iter()
            .find(|face| face.label == label)
            .unwrap_or_else(|| panic!("no {label} face"))
    }

    fn assert_point(actual: (f32, f32), expected: (f32, f32)) {
        assert!(
            (actual.0 - expected.0).abs() < 1e-4 && (actual.1 - expected.1).abs() < 1e-4,
            "{actual:?} != {expected:?}"
        );
    }

    // The worked example this pure function exists to make checkable: looking
    // straight down `-Z` (Roblox's own spawn convention — see
    // `rbx_viewer::camera::Camera::spawn_pose`), the `Back` face (paired
    // with `-Z`) sits dead ahead of the camera and fills the indicator
    // square-on, corner-for-corner.
    #[test]
    fn looking_down_minus_z_shows_the_back_face_square_on() {
        let faces = visible_faces(pose(0.0, 0.0));

        let back = face(&faces, "Back");
        assert_point(back.center, (0.0, 0.0));
        assert!((back.prominence - 1.0).abs() < 1e-4, "{}", back.prominence);
        for corner in back.corners {
            assert!(
                (corner.0.abs() - 1.0).abs() < 1e-4 && (corner.1.abs() - 1.0).abs() < 1e-4,
                "corner {corner:?} is not a unit-square corner"
            );
        }

        // Its edge-on neighbours have nothing to show — see
        // `a_face_exactly_edge_on_collapses_to_a_line_not_a_square` below for
        // their actual (degenerate) shape.
        assert!(face(&faces, "Left").prominence < 1e-4);
        assert!(face(&faces, "Bottom").prominence < 1e-4);
    }

    // Its neighbour, the axis this pose looks exactly along the edge of,
    // degenerates: both `+X`/`-X` faces are edge-on from here, and whichever
    // one this ties to (`Left`, per the strict `> 0.0` front test) collapses
    // to a two-point line rather than a real quadrilateral.
    #[test]
    fn a_face_exactly_edge_on_collapses_to_a_line_not_a_square() {
        let faces = visible_faces(pose(0.0, 0.0));

        let left = face(&faces, "Left");
        assert_point(left.corners[0], left.corners[1]);
        assert_point(left.corners[2], left.corners[3]);
        assert!(
            (left.corners[0].1 - left.corners[2].1).abs() > 1.0,
            "the two collapsed points should still be a stud apart: {:?} vs {:?}",
            left.corners[0],
            left.corners[2]
        );
    }

    // A quarter turn brings the adjacent axis square-on instead — the same
    // rotation `orientation`'s previous dot-based version checked.
    #[test]
    fn a_quarter_turn_brings_the_left_face_square_on() {
        let quarter_turn = std::f32::consts::FRAC_PI_2;
        let faces = visible_faces(pose(quarter_turn, 0.0));

        let left = face(&faces, "Left");
        assert_point(left.center, (0.0, 0.0));
        assert!((left.prominence - 1.0).abs() < 1e-4, "{}", left.prominence);
        for corner in left.corners {
            assert!(
                (corner.0.abs() - 1.0).abs() < 1e-4 && (corner.1.abs() - 1.0).abs() < 1e-4,
                "corner {corner:?} is not a unit-square corner"
            );
        }
    }

    #[test]
    fn every_faces_centre_is_the_mean_of_its_own_corners() {
        for face in visible_faces(pose(0.73, -0.31)) {
            let sum = face
                .corners
                .iter()
                .fold((0.0, 0.0), |acc, c| (acc.0 + c.0, acc.1 + c.1));
            let mean = (sum.0 / 4.0, sum.1 / 4.0);
            assert_point(mean, face.center);
        }
    }

    #[test]
    fn the_three_visible_faces_are_one_per_axis_colour() {
        let mut colors: Vec<u32> = visible_faces(pose(0.73, -0.31))
            .iter()
            .map(|face| face.color)
            .collect();
        colors.sort_unstable();

        let mut expected = [COLOR_X, COLOR_Y, COLOR_Z];
        expected.sort_unstable();
        assert_eq!(colors, expected);
    }
}
