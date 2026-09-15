//! Orbiting perspective camera framed on a scene's bounding box, plus the free-flight
//! poses the windowed viewer uses instead: spawning at the scene's center by default,
//! or (with `--orbit`) handed off from the orbit camera on the user's first input.

use std::time::Duration;

use glam::camera::rh::proj::directx::perspective_infinite_reverse;
use glam::camera::rh::view::{look_at_mat4, look_to_mat4};
use glam::{Mat4, Vec3};

use crate::scene::Bounds;

mod frustum;

pub(crate) use frustum::Frustum;

// Roblox's default `Camera.FieldOfView`, so a capture frames like a Studio one. A
// bounding sphere seen from 2 radii away subtends 60 degrees, so the orbit framing
// keeps a little slack around the scene. The orbit path and the CLI's --eye/--look-at
// capture always use this constant; only a free pose seeded from a place's own saved
// `Camera` (see `Pose::fov_degrees`) can override it.
const FIELD_OF_VIEW_DEGREES: f32 = 70.0;
const DISTANCE_IN_RADII: f32 = 2.0;
const PITCH_DEGREES: f32 = 25.0;
const SCREENSHOT_YAW_DEGREES: f32 = 45.0;
const SECONDS_PER_TURN: f32 = 8.0;
// A single 0.3-stud part has a sub-stud radius; without a floor the camera would sit
// inside its own near plane.
const MIN_DISTANCE: f32 = 4.0;
// Reversed-Z with an infinite far plane (see `projection`) needs no scene-scaled far
// plane and tolerates a near plane this small without shredding depth precision, so
// it's a fixed constant rather than derived from anything: small enough to walk up to
// a 2-stud prop, and non-zero because the projection is singular at exactly zero.
pub(crate) const NEAR_PLANE: f32 = 0.05;
// Where the free camera spawns by default: above the scene's own center rather than
// at a fixed world height, scaled to the scene like everything else the orbit camera
// already frames — a floor keeps a single tiny part from spawning the camera inside it.
const SPAWN_ELEVATION_FRACTION: f32 = 0.25;
const MIN_SPAWN_ELEVATION: f32 = 5.0;

/// A camera pose with no orbit target: where the eye is, and which way it looks.
/// What `FreeController` accumulates frame to frame.
///
/// Public: [`crate::Headless::pose`] hands this to an embedder (`rbxstudio`'s
/// render thread) so it can mirror the live view back into a DOM the renderer
/// itself never touches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Carried untouched across every frame the free controller updates the pose
    /// for (position/yaw/pitch only), so a viewpoint read from a place's own
    /// `Camera` keeps that field of view for the whole free-flight session.
    pub fov_degrees: f32,
}

/// What a single frame is drawn from.
///
/// One or the other, never both: a free pose replaces the orbit framing
/// entirely, so a yaw left over from an earlier frame cannot leak into it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Viewpoint {
    /// Orbiting the scene's bounds, `yaw` radians around its centre.
    Orbit(f32),
    Free(Pose),
}

/// Orbiting camera always framed on a scene's bounding box.
///
/// The camera orbits at a constant distance and pitch, ensuring all corners stay on-screen.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Camera {
    target: Vec3,
    distance: f32,
    pitch: f32,
}

impl Camera {
    /// Positions the camera to frame the entire scene with no slack.
    pub(crate) fn framing(bounds: &Bounds) -> Self {
        Camera {
            target: bounds.center(),
            distance: (bounds.radius() * DISTANCE_IN_RADII).max(MIN_DISTANCE),
            pitch: PITCH_DEGREES.to_radians(),
        }
    }

    /// The free-flight pose that lands exactly on the orbit camera at this yaw —
    /// the seed used the instant input ends the automatic orbit, so the view
    /// never jumps between the two controllers.
    pub(crate) fn orbit_pose(bounds: &Bounds, yaw: f32) -> Pose {
        let camera = Camera::framing(bounds);
        Pose {
            position: camera.eye(yaw),
            yaw,
            pitch: camera.pitch,
            fov_degrees: FIELD_OF_VIEW_DEGREES,
        }
    }

    /// Where the windowed viewer's free camera spawns when it doesn't start
    /// orbiting: the scene's own center, raised clear of the ground, looking
    /// flat along -Z — as if the viewer had just appeared in the middle of the map.
    pub(crate) fn spawn_pose(bounds: &Bounds) -> Pose {
        let elevation = (bounds.radius() * SPAWN_ELEVATION_FRACTION).max(MIN_SPAWN_ELEVATION);
        Pose {
            position: bounds.center() + Vec3::Y * elevation,
            yaw: 0.0,
            pitch: 0.0,
            fov_degrees: FIELD_OF_VIEW_DEGREES,
        }
    }

    /// Looks from another height, in degrees above the target — negative to look
    /// up from underneath, which is the only way to see the sky's zenith panel.
    pub(crate) fn pitched(self, degrees: f32) -> Self {
        Camera {
            pitch: degrees.to_radians(),
            ..self
        }
    }

    pub(crate) fn view_projection(&self, from: Viewpoint, aspect: f32) -> Mat4 {
        let view = match from {
            Viewpoint::Free(pose) => {
                look_to_mat4(pose.position, direction(pose.yaw, pose.pitch), Vec3::Y)
            }
            Viewpoint::Orbit(yaw) => look_at_mat4(self.eye(yaw), self.target, Vec3::Y),
        };
        self.projection(aspect, fov_degrees(from)) * view
    }

    /// The same view with the eye pinned at the origin.
    ///
    /// What the skybox draws with: it must turn with the camera but never
    /// translate with it, or the sky would slide past as the camera orbits or flies.
    pub(crate) fn view_rotation_projection(&self, from: Viewpoint, aspect: f32) -> Mat4 {
        let view = match from {
            Viewpoint::Free(pose) => {
                look_to_mat4(Vec3::ZERO, direction(pose.yaw, pose.pitch), Vec3::Y)
            }
            Viewpoint::Orbit(yaw) => look_at_mat4(Vec3::ZERO, self.target - self.eye(yaw), Vec3::Y),
        };
        self.projection(aspect, fov_degrees(from)) * view
    }

    /// Where the eye actually is this frame, free pose included.
    ///
    /// The shading needs it: specular highlights, reflections and fog are all
    /// measured from the camera, and `view_projection` hides it inside a matrix.
    pub(crate) fn eye_position(&self, from: Viewpoint) -> Vec3 {
        match from {
            Viewpoint::Free(pose) => pose.position,
            Viewpoint::Orbit(yaw) => self.eye(yaw),
        }
    }

    /// The eight world-space corners of this frame's view frustum, truncated at
    /// `distance` studs instead of running to the infinite far plane — what the
    /// shadow map is fitted to.
    ///
    /// Unprojected from the same matrix the frame is drawn with rather than
    /// rebuilt from the pose, so the two can never disagree. Reversed-Z (see
    /// [`Camera::projection`]) puts a point `d` studs away at depth
    /// `NEAR_PLANE / d`, which is where the two depth slices come from.
    pub(crate) fn frustum_corners(&self, from: Viewpoint, aspect: f32, distance: f32) -> [Vec3; 8] {
        let inverse = self.view_projection(from, aspect).inverse();
        let far_depth = reversed_depth(distance.max(NEAR_PLANE * 2.0));

        std::array::from_fn(|index| {
            let sign = |bit: usize| if index & (1 << bit) == 0 { -1.0 } else { 1.0 };
            let depth = if index & 0b100 == 0 { 1.0 } else { far_depth };
            let corner = inverse * glam::Vec4::new(sign(0), sign(1), depth, 1.0);
            corner.truncate() / corner.w
        })
    }

    fn projection(&self, aspect: f32, fov_degrees: f32) -> Mat4 {
        // Degenerate surfaces (a window collapsed to zero width) would make the
        // projection non-invertible; a square frame keeps the frame renderable.
        let aspect = if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            1.0
        };

        // The DirectX flavour is the WebGPU one: Z in [0, 1] with Y up. The infinite
        // reverse variant maps NEAR_PLANE to depth 1 and infinity to depth 0, which
        // spreads the float32 depth buffer's precision close to uniformly across that
        // whole range instead of concentrating it near the eye — see renderer.rs's
        // depth clear value and every pipeline's CompareFunction, which flip to match.
        perspective_infinite_reverse(fov_degrees.to_radians(), aspect, NEAR_PLANE)
    }

    /// Yaw a single offscreen frame is taken at, unless the caller picked one.
    pub(crate) fn screenshot_yaw(degrees: Option<f32>) -> f32 {
        degrees.unwrap_or(SCREENSHOT_YAW_DEGREES).to_radians()
    }

    pub(crate) fn orbit_yaw(elapsed: Duration) -> f32 {
        std::f32::consts::TAU * elapsed.as_secs_f32() / SECONDS_PER_TURN
    }

    fn eye(&self, yaw: f32) -> Vec3 {
        self.target - self.distance * direction(yaw, self.pitch)
    }
}

/// What a pixel with nothing drawn in it reads as, in studs.
///
/// The depth buffer is cleared to 0, which [`view_distance`] maps to an infinite
/// distance: the sky is behind everything, so anything keyed off depth has to
/// treat it as maximally far rather than as sitting at the eye. Finite and
/// absurdly large rather than `f32::INFINITY`, because the arithmetic downstream
/// (the depth-of-field ramp in `renderer::post`'s shader, which mirrors this)
/// subtracts distances and `inf - inf` is a NaN. Only the shader's own copy of
/// the number runs in anger; this one exists so the tests of both twins agree on
/// it.
#[cfg(test)]
pub(crate) const BACKGROUND_DISTANCE: f32 = 1.0e9;

/// The reversed-Z depth buffer value a point `studs` down the view axis lands on.
///
/// `perspective_infinite_reverse` (see [`Camera::projection`]) is a matrix whose
/// only z terms are `clip.z = NEAR_PLANE` and `clip.w = -view_z`, so the depth a
/// fragment stores is exactly `NEAR_PLANE / studs` — the infinite far plane means
/// nothing else enters into it. Singular at zero studs, which every caller floors
/// away itself rather than being silently clamped here.
fn reversed_depth(studs: f32) -> f32 {
    NEAR_PLANE / studs
}

/// The inverse of [`reversed_depth`]: how far down the view axis a depth buffer
/// value stands, in studs.
///
/// Mirrored by `view_distance` in `post.wgsl`, which is the copy that actually
/// runs per pixel — a shader cannot be unit-tested, and the two drifting apart
/// would misplace every focus plane at once. Only compiled for tests here for
/// that reason: nothing on the CPU side needs the reconstruction.
#[cfg(test)]
fn view_distance(depth: f32) -> f32 {
    if depth <= 0.0 {
        return BACKGROUND_DISTANCE;
    }
    NEAR_PLANE / depth
}

/// Builds a free-camera pose standing at `eye` and looking toward `look_at` — the
/// offscreen `--eye`/`--look-at` path, which places the camera anywhere rather than
/// only orbiting a scene's bounds. Degenerate input (`eye == look_at`) falls back to
/// looking down -Z rather than producing a NaN pose. Always framed at the crate's
/// default field of view: the CLI capture path has no DOM camera to read one from.
pub(crate) fn look_at_pose(eye: Vec3, look_at: Vec3) -> Pose {
    let forward = (look_at - eye).normalize_or(-Vec3::Z);
    Pose {
        position: eye,
        yaw: (-forward.x).atan2(-forward.z),
        pitch: (-forward.y).clamp(-1.0, 1.0).asin(),
        fov_degrees: FIELD_OF_VIEW_DEGREES,
    }
}

/// The field of view a frame is drawn with: a free pose carries its own (set to the
/// default everywhere except the place-camera path in `Headless::open_at`), while
/// orbit framing always uses the crate-wide default — it has no pose to carry one in.
fn fov_degrees(from: Viewpoint) -> f32 {
    match from {
        Viewpoint::Free(pose) => pose.fov_degrees,
        Viewpoint::Orbit(_) => FIELD_OF_VIEW_DEGREES,
    }
}

// The direction a viewer at this yaw/pitch looks. Orbiting always looks back at its
// target, i.e. the negative of the eye's offset from it — reusing that same
// trigonometry here is what makes the free camera's look direction agree with the
// orbit camera's at the instant one hands off to the other.
fn direction(yaw: f32, pitch: f32) -> Vec3 {
    -Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    )
}

#[cfg(test)]
#[path = "camera/tests.rs"]
mod tests;
