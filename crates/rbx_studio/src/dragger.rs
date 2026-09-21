//! Roblox Studio's dragger guides, and the snapping they illustrate: the ruler
//! that measures a hovered point or a free drag's landing from the nearest
//! corner of the face under it, the lines a free drag aligns along, and the
//! dots, axis line and label a handle drag shows.
//!
//! Everything here is Studio's own behaviour, read out of the
//! `DraggerFramework` and `DraggerSchemaCore` packages Studio ships (client
//! 0.739), with the constants those packages use. It follows the legacy
//! visuals — Studio's default, with the `NextGenDraggers` beta off. Where a
//! detail had to be inferred rather than read, the comment at that spot says
//! so. Pure geometry, no GPU and no DOM: the view and `Shell` feed in what
//! each of them knows, and the renderer draws the [`Guides`] that come out.

pub(crate) mod free;
pub(crate) mod label;
pub(crate) mod ruler;
pub(crate) mod surface;
pub(crate) mod sweep;

use glam::Vec3;
use rbx_viewer::Pose;

/// `Studio.DraggerMajorGridIncrement`'s default: every fifth ruler tick,
/// counted from the edge, is a long one.
pub(crate) const MAJOR_GRID_INCREMENT: u32 = 5;
/// `Studio.DraggerMaxSoftSnaps`' default. Studio's own test fixture uses 100;
/// what ships is 32.
pub(crate) const MAX_SOFT_SNAPS: usize = 32;
/// `Studio.DraggerSoftSnapMarginFactor`'s default, the multiplier on every
/// soft-snap reach.
const SOFT_SNAP_MARGIN: f32 = 1.0;

/// `Studio.DraggerPassiveColor`: the hover ruler, a handle drag's axis line
/// and its soft-snap dots.
pub(crate) const PASSIVE: [f32; 3] = [1.0, 1.0, 1.0];
/// `Studio.DraggerActiveColor`: whatever a drag is actually snapped to.
pub(crate) const ACTIVE: [f32; 3] = [1.0, 1.0, 0.0];

/// The camera term of Studio's `getHandleScale`: the point's depth along the
/// view direction times the sine of the vertical field of view.
///
/// Studio multiplies it by 0.05 for a handle's size ([`handle_scale`]) and by
/// 0.02 for a free drag's soft-snap reach. A world length `c *
/// depth_scale` covers `c·cos²(fov/2)` of the viewport's height wherever the
/// point sits on screen, which is what keeps every guide the same size on
/// screen at any distance.
pub(crate) fn depth_scale(point: Vec3, pose: Pose, orthographic: bool) -> f32 {
    let fov = pose.fov_degrees.to_radians();
    if orthographic {
        // Studio has no parallel projection. The same share of the view's
        // height it would cover in perspective, measured against the
        // orthographic view's own height instead.
        return 2.0 * pose.ortho_scale * (fov * 0.5).cos().powi(2);
    }
    let (forward, ..) = pose.basis();
    (point - pose.position).dot(forward).max(0.0) * fov.sin()
}

/// Studio's `getHandleScale` for its legacy draggers: every screen-constant
/// size a guide has is a multiple of this.
pub(crate) fn handle_scale(point: Vec3, pose: Pose, orthographic: bool) -> f32 {
    0.05 * depth_scale(point, pose, orthographic)
}

/// One guide segment, drawn the way Studio draws its guides: once
/// depth-tested against the scene and once over everything, each copy with
/// its own opacity (Studio's `MainTransparency`/`DimTransparency` pairs). The
/// stretch in plain view reads strong and the stretch behind geometry faint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Line {
    pub(crate) from: Vec3,
    pub(crate) to: Vec3,
    pub(crate) color: [f32; 3],
    /// Opacity of the depth-tested copy.
    pub(crate) under: f32,
    /// Opacity of the copy drawn over everything.
    pub(crate) over: f32,
    /// World-space thickness, or `0.0` for the engine's one-pixel line
    /// (Studio never sets `Thickness` on a guide). Only the dragged point's
    /// bar, which Studio draws as a thin box, has one.
    pub(crate) width: f32,
}

impl Line {
    fn hairline(from: Vec3, to: Vec3, color: [f32; 3], under: f32, over: f32) -> Line {
        Line {
            from,
            to,
            color,
            under,
            over,
            width: 0.0,
        }
    }
}

/// A marker Studio draws with a handle adornment, always over everything.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Dot {
    pub(crate) centre: Vec3,
    /// The sphere's radius, or half the cube's side.
    pub(crate) radius: f32,
    pub(crate) color: [f32; 3],
    /// A cube (`BoxHandleAdornment`) rather than a sphere.
    pub(crate) cube: bool,
}

/// Everything one frame's guides draw.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Guides {
    pub(crate) lines: Vec<Line>,
    pub(crate) dots: Vec<Dot>,
}

/// Luau's `math.sign`, which answers 0 for 0 where `f32::signum` answers 1:
/// Studio's guides lean on that to leave an axis the cursor sits exactly on
/// undecided.
fn sign(value: f32) -> f32 {
    if value == 0.0 {
        0.0
    } else {
        value.signum()
    }
}

/// `floor(v/g + 0.5)*g`, the rounding every Studio snap uses — halves round
/// up, not away from zero. No grid (`g <= 0`) passes the value through.
fn snap_to(value: f32, grid: f32) -> f32 {
    if grid > 0.0 {
        (value / grid + 0.5).floor() * grid
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn looking_down_z() -> Pose {
        Pose {
            position: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            fov_degrees: 70.0,
            ortho_scale: 10.0,
        }
    }

    #[test]
    fn a_handle_covers_the_same_share_of_the_screen_at_any_depth() {
        let pose = looking_down_z();
        let (forward, ..) = pose.basis();
        let near = handle_scale(forward * 10.0, pose, false);
        let far = handle_scale(forward * 40.0, pose, false);
        assert!((far / near - 4.0).abs() < 1e-4);
        // 0.05·cos²(35°) of the view's height per unit of scale: 0.15 of a
        // scale is about 5.4 pixels at 1080p, the radius of Studio's dots.
        let visible = 2.0 * 10.0 * 35f32.to_radians().tan();
        let share = 0.15 * near / visible * 1080.0;
        assert!((share - 5.435).abs() < 0.01, "{share} px");
    }

    #[test]
    fn a_point_off_centre_scales_by_its_depth_not_its_distance() {
        let pose = looking_down_z();
        let (forward, right, _) = pose.basis();
        let centre = handle_scale(forward * 10.0, pose, false);
        let aside = handle_scale(forward * 10.0 + right * 5.0, pose, false);
        assert!((centre - aside).abs() < 1e-5);
    }

    #[test]
    fn orthographic_keeps_the_perspective_share_of_the_screen() {
        let pose = looking_down_z();
        let scale = handle_scale(Vec3::ZERO, pose, true);
        let share = scale / (2.0 * pose.ortho_scale);
        assert!((share - 0.05 * 35f32.to_radians().cos().powi(2)).abs() < 1e-6);
    }

    #[test]
    fn studio_rounds_halves_up() {
        assert_eq!(snap_to(2.5, 1.0), 3.0);
        assert_eq!(snap_to(-2.5, 1.0), -2.0);
        assert_eq!(snap_to(-2.6, 1.0), -3.0);
        assert_eq!(snap_to(1.3, 0.0), 1.3);
    }
}
