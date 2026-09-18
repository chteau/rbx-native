//! The viewport's orientation indicator: pure math for where each world
//! axis's dot lands inside the widget, given the camera's current pose.
//!
//! An rbx-native addition, not a Roblox Studio one — see the `ROADMAP.md`
//! bullet this implements. Modelled on Blender's own gizmo, which is drawn
//! the same way: a flat 2D arrangement of coloured dots at each axis's
//! projected screen direction, not a literal 3D cube mesh in the scene.
//! `workspace_view.rs`'s `Render` impl turns [`marks`]'s answer into the
//! actual GPUI elements, the same split `label.rs` keeps between formatting
//! and layout.

use glam::Vec3;
use rbx_viewer::Pose;

// Studio's own red/green/blue X/Y/Z convention — the same sRGB source values
// `rbx_viewer::gizmo::Axis::color`'s doc comment documents (that copy is
// linearized for the HDR render pipeline; this one is plain sRGB, since GPUI
// paints this overlay directly rather than through the scene's tonemap).
const COLOR_X: u32 = 0xe62929;
const COLOR_Y: u32 = 0x29c738;
const COLOR_Z: u32 = 0x335cf2;

/// Which world axis each label names — an rbx-native convention, not a
/// Roblox one: positionally paired the way the `ROADMAP.md` bullet itself
/// lists them (`+X`/Right, `-X`/Left, `+Y`/Top, `-Y`/Bottom, `+Z`/Front,
/// `-Z`/Back), not Roblox's own `CFrame.LookVector` (`-Z`).
const AXES: [(Vec3, &str, u32); 6] = [
    (Vec3::X, "Right", COLOR_X),
    (Vec3::NEG_X, "Left", COLOR_X),
    (Vec3::Y, "Top", COLOR_Y),
    (Vec3::NEG_Y, "Bottom", COLOR_Y),
    (Vec3::Z, "Front", COLOR_Z),
    (Vec3::NEG_Z, "Back", COLOR_Z),
];

/// One axis's dot: where it sits and how it reads.
pub(super) struct AxisMark {
    pub(super) label: &'static str,
    pub(super) color: u32,
    /// Offset from the indicator's centre, on a unit circle — the caller
    /// scales by the widget's actual on-screen radius. `x` is screen-right,
    /// `y` is screen-down (GPUI's own convention, not world space).
    pub(super) x: f32,
    pub(super) y: f32,
    /// Roughly aligned with the way the camera currently looks, so drawn
    /// prominent; the opposite three axes (roughly behind the camera) are
    /// not, and draw dim. Not a literal near/far cube-face test — see
    /// [`marks`]'s own doc for why that reads unintuitively for whichever
    /// label happens to be looked "away from".
    pub(super) front: bool,
}

/// The six axis marks for a camera at this pose, ready to lay out around the
/// indicator's own centre.
///
/// Depends only on `pose`'s rotation (`Pose::basis`), never its position or
/// `ortho_scale` — so, deliberately unlike most of this camera's own state,
/// this projection needs no special-casing for orthographic mode: an
/// orthographic camera's basis is exactly as meaningful as a perspective
/// one's, since both share the same notion of "which way is it looking".
///
/// "Prominent" (`AxisMark::front`) means roughly aligned with the camera's
/// own forward vector, not "closer to the viewer" in the literal ViewCube
/// sense — looking straight down `-Z` puts `-Z` ("Back") dead ahead and `+Z`
/// ("Front") behind it, so the "Back" dot draws prominently and "Front"
/// dims. That reads oddly next to the label's English meaning, but it is
/// what "which axis am I currently facing" has to mean for a fixed
/// label-to-axis pairing with no free camera-relative renaming — and this
/// widget is an original rbx-native convention to begin with, not a claim
/// about how Roblox or any other tool labels its own gizmo.
pub(super) fn marks(pose: Pose) -> [AxisMark; 6] {
    let (forward, right, up) = pose.basis();
    AXES.map(|(axis, label, color)| AxisMark {
        label,
        color,
        x: axis.dot(right),
        y: -axis.dot(up),
        front: axis.dot(forward) > 0.0,
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

    fn mark<'a>(marks: &'a [AxisMark; 6], label: &str) -> &'a AxisMark {
        marks
            .iter()
            .find(|mark| mark.label == label)
            .unwrap_or_else(|| panic!("no {label} mark"))
    }

    // Roblox's own spawn convention: `Camera` looking flat along `-Z`, no
    // roll — see `crate::camera::Camera::spawn_pose` in `rbx_viewer`.
    #[test]
    fn right_projects_to_the_right_when_looking_down_minus_z() {
        let marks = marks(pose(0.0, 0.0));

        let right = mark(&marks, "Right");
        assert!((right.x - 1.0).abs() < 1e-5, "x = {}", right.x);
        assert!(right.y.abs() < 1e-5, "y = {}", right.y);
    }

    // The worked example this pure function exists to make checkable: looking
    // straight down `-Z`, the `Front` label (paired with `+Z`, the opposite
    // way this project names it from Roblox's own `-Z` `LookVector`) sits
    // dead ahead in world space but *behind* the camera's own forward
    // vector, so it draws as the dim, non-prominent mark.
    #[test]
    fn front_is_the_non_prominent_mark_when_looking_down_minus_z() {
        let marks = marks(pose(0.0, 0.0));

        let front = mark(&marks, "Front");
        assert!(!front.front);
        assert!(front.x.abs() < 1e-5);
        assert!(front.y.abs() < 1e-5);

        // Its opposite, dead ahead of the camera, is the prominent one.
        let back = mark(&marks, "Back");
        assert!(back.front);
    }

    #[test]
    fn top_projects_upward_pitched_level() {
        let marks = marks(pose(0.0, 0.0));

        let top = mark(&marks, "Top");
        assert!(top.x.abs() < 1e-5);
        // Screen `y` grows downward, so "up" on screen is negative `y`.
        assert!((top.y + 1.0).abs() < 1e-5, "y = {}", top.y);
    }

    #[test]
    fn yawing_a_quarter_turn_swaps_which_label_reads_as_right() {
        let quarter_turn = std::f32::consts::FRAC_PI_2;
        let marks = marks(pose(quarter_turn, 0.0));

        // A 90-degree yaw turns the camera to look along world `-X`: the axis
        // that used to sit dead ahead (`Back`, world `-Z`) now reads as
        // screen-right instead of `Right` (world `+X`).
        let back = mark(&marks, "Back");
        assert!((back.x - 1.0).abs() < 1e-4, "x = {}", back.x);
        assert!(back.y.abs() < 1e-4, "y = {}", back.y);

        // `Right` itself, no longer beside the camera but directly behind
        // it, now projects to the centre and reads as non-prominent.
        let right = mark(&marks, "Right");
        assert!(right.x.hypot(right.y) < 1e-4);
        assert!(!right.front);
    }

    #[test]
    fn every_mark_stays_within_the_unit_circle() {
        for mark in marks(pose(0.73, -0.31)) {
            assert!(mark.x.hypot(mark.y) <= 1.0 + 1e-5);
        }
    }
}
