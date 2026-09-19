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
    /// The axis's own one-letter name, for the orbital indicator's labels.
    name: &'static str,
    color: u32,
}

const AXIS_FACES: [AxisFaces; 3] = [
    AxisFaces {
        axis: Vec3::X,
        a: Vec3::Y,
        b: Vec3::Z,
        positive: "Right",
        negative: "Left",
        name: "X",
        color: COLOR_X,
    },
    AxisFaces {
        axis: Vec3::Y,
        a: Vec3::Z,
        b: Vec3::X,
        positive: "Top",
        negative: "Bottom",
        name: "Y",
        color: COLOR_Y,
    },
    AxisFaces {
        axis: Vec3::Z,
        a: Vec3::X,
        b: Vec3::Y,
        positive: "Front",
        negative: "Back",
        name: "Z",
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
    ///
    /// A face's projected *centre* is simply the mean of these, since the
    /// projection is linear; it used to be carried as its own field, and
    /// is not any more because nothing needed it.
    pub(super) corners: [(f32, f32); 4],
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
        }
    })
}

/// One world axis's orbital ring, split at the horizon into the half in
/// front of the sphere's centre and the half behind it.
///
/// The split is the whole illusion: two arcs of the same colour, the far
/// one dimmed, read as a circle passing *through* a sphere rather than as a
/// flat ellipse drawn on top of one. Nothing here is a 3D render — it is
/// the same orthographic projection the cube faces already use.
///
/// Each half is walked from the same start angle, so a ring that crosses
/// the horizon twice yields its two runs in order; the caller strokes each
/// as an open polyline, which is why neither is closed.
#[derive(Debug, Clone)]
pub(super) struct Ring {
    pub(super) color: u32,
    /// The arc on the camera's side of centre, in screen units.
    pub(super) near: Vec<(f32, f32)>,
    /// The arc behind it.
    pub(super) far: Vec<(f32, f32)>,
}

/// Where one axis's name sits, and how much of it the camera can see.
#[derive(Debug, Clone, Copy)]
pub(super) struct AxisLabel {
    pub(super) text: &'static str,
    pub(super) color: u32,
    pub(super) at: (f32, f32),
    /// `1.0` when the axis points straight at the camera, `0.0` when it
    /// lies in the screen plane, and negative when it points away — a
    /// caller fades the far ones rather than letting three labels pile up
    /// in the middle.
    pub(super) depth: f32,
}

/// How many segments each ring is drawn with. Enough that a 40px circle has
/// no visible corners, few enough that three of them cost nothing.
const RING_SEGMENTS: usize = 64;

/// The three orbital rings at this pose — one per world axis, each the unit
/// circle in the plane *perpendicular* to that axis, so the red ring is the
/// one the X axis passes through.
pub(super) fn axis_rings(pose: Pose) -> [Ring; 3] {
    let (forward, right, up) = pose.basis();
    let project = |world: Vec3| (world.dot(right), -world.dot(up));

    AXIS_FACES.map(|group| {
        let mut near = Vec::with_capacity(RING_SEGMENTS + 1);
        let mut far = Vec::with_capacity(RING_SEGMENTS + 1);

        for step in 0..=RING_SEGMENTS {
            let angle = step as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            let world = group.a * angle.cos() + group.b * angle.sin();
            let point = project(world);
            // `forward` points away from the camera, so a *negative* dot is
            // the half nearer the viewer.
            if world.dot(forward) <= 0.0 {
                near.push(point);
            } else {
                far.push(point);
            }
        }

        Ring {
            color: group.color,
            near,
            far,
        }
    })
}

/// The three axis names, placed where each axis's positive end meets the
/// sphere.
pub(super) fn axis_labels(pose: Pose) -> [AxisLabel; 3] {
    let (forward, right, up) = pose.basis();
    let project = |world: Vec3| (world.dot(right), -world.dot(up));

    AXIS_FACES.map(|group| AxisLabel {
        text: group.name,
        color: group.color,
        at: project(group.axis),
        depth: -group.axis.dot(forward),
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
        assert_point(centre_of(back), (0.0, 0.0));
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
        assert_point(centre_of(left), (0.0, 0.0));
        assert!((left.prominence - 1.0).abs() < 1e-4, "{}", left.prominence);
        for corner in left.corners {
            assert!(
                (corner.0.abs() - 1.0).abs() < 1e-4 && (corner.1.abs() - 1.0).abs() < 1e-4,
                "corner {corner:?} is not a unit-square corner"
            );
        }
    }

    /// A square's projected centre is the mean of its projected corners,
    /// because the projection is linear — which is what lets the renderer
    /// place anything relative to a face without carrying a separate point.
    #[test]
    fn a_faces_centre_is_the_mean_of_its_own_corners() {
        for face in visible_faces(pose(0.0, 0.0)) {
            let centre = centre_of(&face);
            assert!(
                centre.0.is_finite() && centre.1.is_finite(),
                "{centre:?} is not a point"
            );
        }
        // Looking straight down -Z puts Back dead centre, and its four
        // corners are symmetric about it.
        let faces = visible_faces(pose(0.0, 0.0));
        assert_point(centre_of(face(&faces, "Back")), (0.0, 0.0));
    }

    /// The three rings are one per axis colour, and each is split into the
    /// half in front of the sphere and the half behind it.
    #[test]
    fn the_three_rings_are_one_per_axis_colour_and_split_at_the_horizon() {
        let rings = axis_rings(pose(0.73, -0.31));

        let mut colors: Vec<u32> = rings.iter().map(|ring| ring.color).collect();
        colors.sort_unstable();
        let mut expected = [COLOR_X, COLOR_Y, COLOR_Z];
        expected.sort_unstable();
        assert_eq!(colors, expected);

        for ring in &rings {
            assert_eq!(
                ring.near.len() + ring.far.len(),
                RING_SEGMENTS + 1,
                "every sampled point belongs to exactly one half"
            );
            assert!(
                !ring.near.is_empty(),
                "a ring with no near half would read as painted behind the sphere entirely"
            );
        }
    }

    /// Each axis's name sits where that axis meets the sphere, and the one
    /// pointing at the camera is the least foreshortened.
    #[test]
    fn an_axis_label_follows_its_own_axis() {
        let labels = axis_labels(pose(0.0, 0.0));
        let z = labels
            .iter()
            .find(|label| label.text == "Z")
            .expect("a Z label");

        // Looking down -Z, +Z points straight back at the camera: its label
        // lands at the centre and is at full depth.
        assert_point(z.at, (0.0, 0.0));
        assert!((z.depth - 1.0).abs() < 1e-4, "{}", z.depth);
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

    fn centre_of(face: &Face) -> (f32, f32) {
        let sum = face
            .corners
            .iter()
            .fold((0.0, 0.0), |acc, c| (acc.0 + c.0, acc.1 + c.1));
        (sum.0 / 4.0, sum.1 / 4.0)
    }
}
