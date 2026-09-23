//! Orbiting perspective camera framed on a scene's bounding box, plus the free-flight
//! poses the windowed viewer uses instead: spawning at the scene's center by default,
//! or (with `--orbit`) handed off from the orbit camera on the user's first input.

use std::time::Duration;

use glam::camera::rh::proj::directx::{orthographic, perspective_infinite_reverse};
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
// inside its own near plane. Reused by `initial_ortho_scale` for the same reason on a
// free pose: starting (or resyncing — see `Controller::sync_ortho_scale`) right on top
// of whatever it's measuring from must not collapse the orthographic view volume to
// zero size either.
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
// Orthographic mode has no infinite-far equivalent of `perspective_infinite_reverse`
// — its depth map is linear in view-space Z (see `orthographic_reversed_depth`), so
// unlike the hyperbolic perspective one it needs both ends finite or the matrix is
// singular. Scaled off the current zoom level (`Camera::orthographic_half_height`)
// rather than a flat constant: a first attempt used one fixed, generous constant
// (comfortably larger than any real Roblox build) regardless of zoom, on the theory
// that "as long as it never clips, the exact value doesn't matter" — but a near/far
// span that wide demonstrably wrecks float32 depth precision for tight, near-camera
// work (a 0.05-stud near plane next to a several-hundred-thousand-stud far plane
// leaves too few significant digits to place a depth-of-field focus plane or fit a
// shadow frustum with — this was caught by a test, not a screenshot). Scaling with
// zoom instead keeps the span proportionate to what's actually on screen: tight and
// precise zoomed in for detail work, generous (and precision no longer matters at
// that scale) zoomed out — clamped both ways rather than left pure proportional, so
// an extreme zoom-in still leaves room for ordinary background depth, and an extreme
// zoom-out doesn't have the range run away unboundedly. The same distance is used
// *behind* the eye plane too — see `orthographic_range`.
const ORTHOGRAPHIC_FAR_MULTIPLIER: f32 = 200.0;
const ORTHOGRAPHIC_MIN_FAR: f32 = 500.0;
const ORTHOGRAPHIC_MAX_FAR: f32 = 200_000.0;
// The orthographic view volume's usable half-height range, in studs (see
// `Pose::ortho_scale`) — what the mouse wheel's zoom clamps to while orthographic is
// on (`controller::free_update`). Floored well above zero so the matrix never goes
// singular and a still-explorable macro view stays reachable; ceilinged high enough
// that "zoom out" is indefinite in practice for any real build, without letting the
// wheel drive the view volume to an unusable, precision-losing extreme.
pub(crate) const MIN_ORTHO_SCALE: f32 = 0.5;
pub(crate) const MAX_ORTHO_SCALE: f32 = 1_000_000.0;

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
    /// Half the height of the orthographic view volume, in studs — the free
    /// pose's whole notion of "zoom" while orthographic mode is on (see
    /// `Camera::orthographic_half_height`), meaningless and unused otherwise.
    ///
    /// Given an initial value at construction (`initial_ortho_scale`, from
    /// whatever "distance to the scene" makes sense for the pose in
    /// question), then carried frame to frame and answered directly to the
    /// mouse wheel while orthographic is on (`controller::free_update`) —
    /// deliberately *not* recomputed from the pose's current position every
    /// frame the way an earlier version of this whole feature tried. That
    /// approach read as "zoom", panning closer to *something* made it look
    /// bigger, but it derived the view volume from the free pose's distance
    /// to the scene's single, fixed framing centre — on a level with several
    /// spread-out clusters of geometry (a real report: floating islands
    /// scattered across a big map), that number has nothing to do with how
    /// close the camera actually is to whatever it's currently looking at,
    /// so depending on where in the level you were, the view could still
    /// stay too zoomed out, or clip straight through nearby geometry. An
    /// explicit, user-driven value sidesteps needing to know "what am I
    /// looking at" at all: the wheel can zoom in or out indefinitely from
    /// wherever the camera happens to be, and nothing about it depends on
    /// the rest of the level's layout.
    pub ortho_scale: f32,
}

impl Pose {
    /// This pose's own right-handed basis: `forward` is where it looks
    /// (exactly [`direction`]'s answer), `right`/`up` complete a frame around
    /// world-space `Vec3::Y` as up. What the viewport's orientation indicator
    /// (`rbx_studio::workspace_view::orientation`) projects the six world
    /// axes into screen space with, so that widget never derives camera
    /// rotation on its own. Mathematically degenerate only staring exactly
    /// straight up or down (`pitch` at ±90°, where `forward` runs parallel to
    /// world up and their cross product is zero): `right` falls back to
    /// world `+X` rather than propagating a zero/NaN vector, the same
    /// fallback [`look_at_pose`] uses for its own degenerate case. In
    /// practice a `pitch` that merely rounds to ±90° in `f32` still leaves
    /// the cross product a tiny but nonzero vector, so it normalizes on its
    /// own (to whichever of `±X` the rounding residue happens to land on)
    /// rather than actually reaching this fallback — either way the result
    /// stays finite and unit-length, which is the guarantee that matters.
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let forward = direction(self.yaw, self.pitch);
        let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(forward);
        (forward, right, up)
    }

    /// The matrix a frame flown from this pose is drawn with, at this aspect
    /// ratio and projection mode.
    ///
    /// Public because the cursor and the camera live on different threads: an
    /// embedder that hit-tests a click against the scene (`rbxstudio`'s
    /// click-to-select and its transform gizmo) has to unproject against
    /// exactly the matrix the frame under that cursor was drawn with, and
    /// rebuilding one from the pose's own fields on the far side is how the
    /// two quietly drift apart.
    pub fn view_projection(&self, orthographic: bool, aspect: f32) -> Mat4 {
        // Everything else `Camera` carries — the orbit target, distance and
        // pitch — is read only on the `Viewpoint::Orbit` path, which a free
        // pose never takes.
        Camera {
            target: Vec3::ZERO,
            distance: 0.0,
            pitch: 0.0,
            orthographic,
        }
        .view_projection(Viewpoint::Free(*self), aspect)
    }
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
    /// Parallel projection instead of the default perspective one — the
    /// viewport's orthographic toggle (`rbx_studio::Shell::set_orthographic`),
    /// which reuses this same orbit/free framing rather than a second,
    /// ortho-specific notion of zoom. See [`Camera::orthographic_projection`].
    orthographic: bool,
}

impl Camera {
    /// Positions the camera to frame the entire scene with no slack.
    pub(crate) fn framing(bounds: &Bounds) -> Self {
        Camera {
            target: bounds.center(),
            distance: framing_distance(bounds),
            pitch: PITCH_DEGREES.to_radians(),
            orthographic: false,
        }
    }

    /// Swaps between perspective and parallel projection, leaving the
    /// framing (target/distance/pitch) untouched — the WASD/mouse-look
    /// controller and the orbit path both keep working exactly as before,
    /// per the roadmap item this implements: no new interaction model.
    pub(crate) fn with_orthographic(self, orthographic: bool) -> Self {
        Camera {
            orthographic,
            ..self
        }
    }

    /// Whether this camera projects in parallel rather than in perspective —
    /// what the gizmo's constant-on-screen sizing has to branch on, the two
    /// having entirely different notions of how big a stud is on screen.
    pub(crate) fn is_orthographic(&self) -> bool {
        self.orthographic
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
            ortho_scale: initial_ortho_scale(camera.distance, FIELD_OF_VIEW_DEGREES),
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
            ortho_scale: initial_ortho_scale(framing_distance(bounds), FIELD_OF_VIEW_DEGREES),
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

    /// Scales the orbit distance `Camera::framing` picked, floored the same way it floors
    /// distance to begin with so a very small `factor` can't collapse the view volume to
    /// zero. < 1.0 moves the camera closer (a tighter crop on the scene's own bounds), > 1.0
    /// further. Only the offscreen screenshot path uses this, same as `pitched`.
    pub(crate) fn zoomed(self, factor: f32) -> Self {
        Camera {
            distance: (self.distance * factor).max(MIN_DISTANCE),
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
        self.projection(
            aspect,
            fov_degrees(from),
            self.orthographic_half_height(from),
        ) * view
    }

    /// Orthographic mode's whole notion of zoom (see `orthographic_projection`):
    /// half the height of the view volume, in studs.
    ///
    /// The orbit camera has no zoom control of its own — it derives this
    /// fresh from its own fixed framing distance and FOV every call, the same
    /// apparent size perspective would frame it at. A free pose instead
    /// carries its own value (`Pose::ortho_scale`) that persists frame to
    /// frame and answers directly to the mouse wheel while orthographic is on
    /// (see `controller::free_update`) — see that field's own doc comment for
    /// why this is read verbatim rather than recomputed from the pose here.
    fn orthographic_half_height(&self, from: Viewpoint) -> f32 {
        match from {
            Viewpoint::Orbit(_) => self.distance * (FIELD_OF_VIEW_DEGREES * 0.5).to_radians().tan(),
            Viewpoint::Free(pose) => pose.ortho_scale,
        }
    }

    /// The same view with the eye pinned at the origin.
    ///
    /// What the skybox/stars/sun draw with: it must turn with the camera but
    /// never translate with it, or the sky would slide past as the camera
    /// orbits or flies. Always perspective, even when the main camera itself
    /// is orthographic (see `orthographic_projection`): their geometry
    /// (`renderer::sky`/`stars.wgsl`/`sun.wgsl`) is built at near-unit
    /// magnitude and relies on the perspective divide by `clip.w` to spread
    /// across the screen regardless of a vertex's actual distance from the
    /// origin — an orthographic `w`, fixed at 1, cannot do that (an earlier
    /// version of this method tried compensating with a scale instead, which
    /// visibly seamed at the sky cube's own face boundaries: a linear map has
    /// no per-pixel angular spread to hide them the way a perspective one
    /// does). A parallel *scene* with a perspective *background* is a
    /// deliberate, common decoupling other orthographic-capable 3D tools make
    /// too, not an oversight.
    pub(crate) fn view_rotation_projection(&self, from: Viewpoint, aspect: f32) -> Mat4 {
        let view = match from {
            Viewpoint::Free(pose) => {
                look_to_mat4(Vec3::ZERO, direction(pose.yaw, pose.pitch), Vec3::Y)
            }
            Viewpoint::Orbit(yaw) => look_at_mat4(Vec3::ZERO, self.target - self.eye(yaw), Vec3::Y),
        };
        perspective_infinite_reverse(fov_degrees(from).to_radians(), aspect, NEAR_PLANE) * view
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
    /// `NEAR_PLANE / d`, which is where the two depth slices come from —
    /// orthographic's are `±distance` about the eye plane instead, per its
    /// own linear map (see [`orthographic_range`]).
    pub(crate) fn frustum_corners(&self, from: Viewpoint, aspect: f32, distance: f32) -> [Vec3; 8] {
        let inverse = self.view_projection(from, aspect).inverse();
        let capped_distance = distance.max(NEAR_PLANE * 2.0);
        // Must match whichever branch `projection` actually took, or the far
        // corners unproject to the wrong world depth: perspective's depth map
        // is hyperbolic, orthographic's is linear (see `orthographic_projection`).
        // Orthographic's own view volume runs `distance` studs *behind* the eye
        // plane as well (see `orthographic_range`), so its near slice sits at
        // `-distance`, not at the near plane: fitting the shadow map from the
        // near plane itself would stretch it over the whole range behind the
        // camera and starve everything on screen of resolution.
        let (near_depth, far_depth) = if self.orthographic {
            let range = orthographic_range(self.orthographic_half_height(from));
            (
                orthographic_reversed_depth(-capped_distance, range.near, range.far),
                orthographic_reversed_depth(capped_distance, range.near, range.far),
            )
        } else {
            (1.0, reversed_depth(capped_distance))
        };

        std::array::from_fn(|index| {
            let sign = |bit: usize| if index & (1 << bit) == 0 { -1.0 } else { 1.0 };
            let depth = if index & 0b100 == 0 {
                near_depth
            } else {
                far_depth
            };
            let corner = inverse * glam::Vec4::new(sign(0), sign(1), depth, 1.0);
            corner.truncate() / corner.w
        })
    }

    fn projection(&self, aspect: f32, fov_degrees: f32, half_height: f32) -> Mat4 {
        // Degenerate surfaces (a window collapsed to zero width) would make the
        // projection non-invertible; a square frame keeps the frame renderable.
        let aspect = if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            1.0
        };

        if self.orthographic {
            return orthographic_projection(aspect, half_height);
        }

        // The DirectX flavour is the WebGPU one: Z in [0, 1] with Y up. The infinite
        // reverse variant maps NEAR_PLANE to depth 1 and infinity to depth 0, which
        // spreads the float32 depth buffer's precision close to uniformly across that
        // whole range instead of concentrating it near the eye — see renderer.rs's
        // depth clear value and every pipeline's CompareFunction, which flip to match.
        perspective_infinite_reverse(fov_degrees.to_radians(), aspect, NEAR_PLANE)
    }

    /// `Some(range)` when this camera is orthographic, `None` for the
    /// ordinary perspective path (whose near plane is fixed and far plane
    /// infinite, so nothing needs passing anywhere). What `Post::prepare`
    /// needs to pick the right depth-reconstruction formula in `post.wgsl`'s
    /// `view_distance`.
    pub(crate) fn orthographic_range(&self, from: Viewpoint) -> Option<DepthRange> {
        self.orthographic
            .then(|| orthographic_range(self.orthographic_half_height(from)))
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

/// The parallel-projection counterpart of `perspective_infinite_reverse` (see
/// [`Camera::projection`]): same DirectX convention (Z in `[0, 1]`, Y up),
/// same reversed-Z direction (near maps to depth 1, far to depth 0 — see
/// [`orthographic_reversed_depth`]), but a linear map instead of a
/// hyperbolic one, since an orthographic `w` never varies with distance the
/// way a perspective one does.
///
/// `half_height` is [`Camera::orthographic_half_height`]'s answer — the
/// view volume's actual current zoom. `glam`'s builder takes `(near, far)`;
/// passed here as `(far, near)` to flip its default near-to-0/far-to-1
/// mapping to the reversed convention every other pass in this renderer
/// assumes (see `NEAR_PLANE`'s doc comment and `renderer.rs`'s depth clear
/// value/`CompareFunction`).
fn orthographic_projection(aspect: f32, half_height: f32) -> Mat4 {
    let half_width = half_height * aspect;
    let range = orthographic_range(half_height);
    orthographic(
        -half_width,
        half_width,
        -half_height,
        half_height,
        range.far,
        range.near,
    )
}

/// The depth an orthographic frame spans, in studs down the view axis from
/// the eye plane: `near` is *negative* — see [`orthographic_range`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DepthRange {
    pub(crate) near: f32,
    pub(crate) far: f32,
}

/// Orthographic mode's finite depth range for a view volume of this
/// `half_height` — see [`ORTHOGRAPHIC_FAR_MULTIPLIER`] for why the extent is
/// scaled off the current zoom level rather than fixed.
///
/// Symmetric about the eye plane rather than starting at `NEAR_PLANE` like
/// perspective does: under a parallel projection the eye's position has no
/// optical meaning (the observer is effectively at infinity, every ray is the
/// same direction), so a near plane pinned just in front of it slices clean
/// through anything the camera has flown into or alongside — a real report,
/// a hillside cut in half where the free camera happened to be standing
/// inside its bounds. Other orthographic-capable 3D tools resolve this the
/// same way, by clipping around the view's focus rather than at the camera.
/// The known consequence: geometry behind the eye plane draws *in front of*
/// what's ahead of it (it is nearer to that observer at infinity), so
/// orthographic isn't a way to look around from inside a closed room — that
/// is what perspective is for.
fn orthographic_range(half_height: f32) -> DepthRange {
    let far = (half_height * ORTHOGRAPHIC_FAR_MULTIPLIER)
        .clamp(ORTHOGRAPHIC_MIN_FAR, ORTHOGRAPHIC_MAX_FAR);
    DepthRange { near: -far, far }
}

/// The framing distance [`Camera::framing`]'s orbit camera uses, factored out
/// so [`Camera::spawn_pose`] can seed a free pose's own
/// [`Pose::ortho_scale`] with the same apparent scale an orbit camera over
/// the same bounds would frame at, without constructing a whole `Camera`
/// just to read one field back out of it.
fn framing_distance(bounds: &Bounds) -> f32 {
    (bounds.radius() * DISTANCE_IN_RADII).max(MIN_DISTANCE)
}

/// [`Pose::ortho_scale`]'s starting value: the apparent half-height,
/// `fov_degrees` wide, of something `distance` studs away — the same
/// perspective-equivalent framing every free pose used to derive orthographic
/// zoom from every frame before this was a persistent, wheel-driven value.
/// Floored the same way [`Camera::framing`] floors orbit distance, so a pose
/// created (or resynced — see `Controller::sync_ortho_scale`) right on top of
/// whatever `distance` was measured from doesn't collapse the view volume to
/// zero size.
pub(crate) fn initial_ortho_scale(distance: f32, fov_degrees: f32) -> f32 {
    distance.max(MIN_DISTANCE) * (fov_degrees * 0.5).to_radians().tan()
}

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

/// [`reversed_depth`]'s orthographic counterpart: linear rather than
/// hyperbolic, since [`Camera::orthographic_projection`]'s `w` never varies
/// with distance. Used both by that matrix build (via the swapped-argument
/// call to `orthographic`, whose closed form this mirrors) and by
/// [`Camera::frustum_corners`] to place a shadow-fitting far corner at the
/// right depth. Real (non-test) code, unlike [`reversed_depth`]'s own
/// distance-reconstruction twin below, because shadow fitting needs it at
/// runtime, not just in a test.
fn orthographic_reversed_depth(studs: f32, near: f32, far: f32) -> f32 {
    (far - studs) / (far - near)
}

/// The inverse of [`orthographic_reversed_depth`]: how far down the view axis
/// an orthographic depth buffer value stands, in studs. Mirrored by
/// `view_distance` in `post.wgsl`'s orthographic branch, for the same reason
/// [`view_distance`] above mirrors the perspective one.
#[cfg(test)]
fn orthographic_view_distance(depth: f32, near: f32, far: f32) -> f32 {
    far - depth * (far - near)
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
        ortho_scale: initial_ortho_scale((look_at - eye).length(), FIELD_OF_VIEW_DEGREES),
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
