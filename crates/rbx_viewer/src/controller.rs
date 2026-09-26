//! Turns raw input and elapsed time into the viewpoint drawn each frame.
//!
//! Where it starts is the host's choice (see [`Start`]): the windowed viewer
//! spawns straight into free flight centered on the scene, an embedder usually
//! starts at the place's own saved camera, and `--orbit` instead auto-orbits
//! hands-free until the first look button, movement key, or recentre request
//! switches permanently to free flight — seeded from that exact instant so the
//! view never jumps between the two.

use std::time::Duration;

use glam::Vec3;

use crate::camera::{self, Camera, Pose, Viewpoint, MAX_ORTHO_SCALE, MIN_ORTHO_SCALE};
use crate::input::Input;
use crate::scene::Bounds;

const MIN_SPEED: f32 = 1.0;
const MAX_SPEED: f32 = 10_000.0;
// Studio's own wheel-to-speed feel: a handful of notches roughly doubles or halves it.
const SPEED_STEP: f32 = 1.2;
// "Studio divise fortement": Shift is for careful, slow positioning, not a sprint key.
const SHIFT_SLOW_FACTOR: f32 = 0.1;
const MAX_PITCH_DEGREES: f32 = 89.0;
// A wheel notch (no right-click) reads as "step forward/back roughly half a second at
// the current speed" rather than a fixed distance, so it scales with `--speed` too.
const ZOOM_SECONDS_PER_NOTCH: f32 = 0.5;
// The view `--orbit` starts from, and what F/Home returns to once triggered from it.
const INITIAL_ORBIT_YAW: f32 = 0.0;
// `--speed`'s own default, cruising speed for an ordinary baseplate-sized scene.
pub(crate) const DEFAULT_SPEED: f32 = 30.0;
// Past this radius a 30 studs/s cruise would take forever to cross the scene, so the
// default scales with it instead — still clamped like any other speed.
pub(crate) const LARGE_SCENE_RADIUS: f32 = 500.0;
pub(crate) const LARGE_SCENE_SPEED_DIVISOR: f32 = 20.0;
// Studio's own mouse-look feel, and slow enough that a full turn takes a
// deliberate sweep of the mouse rather than a flick.
pub(crate) const DEFAULT_SENSITIVITY: f32 = 0.1;
// Time constant for easing WASD velocity and the wheel's dolly hop: 3*tau
// (~180ms) is roughly how long the transient takes to settle. No Roblox/Studio
// value to match here — this is this renderer's own feel call, picked to read
// as smooth without lagging behind the keys.
const MOVEMENT_TIME_CONSTANT: f32 = 0.06;
// The `CameraFeel::smoothing` that gives `MOVEMENT_TIME_CONSTANT`: the
// setting's default reads 0.30 and has to land exactly on this feel.
const DEFAULT_SMOOTHING: f32 = 0.3;

/// How the free camera responds, as a host's settings put it: each value a
/// multiplier or amount relative to this renderer's own feel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraFeel {
    /// Mouse-look speed, times `DEFAULT_SENSITIVITY`.
    pub sensitivity: f32,
    /// WASD and wheel-dolly speed, times the current flight speed.
    pub speed: f32,
    /// How much moves ease in and out: 0 is none, `DEFAULT_SMOOTHING` is
    /// `MOVEMENT_TIME_CONSTANT`, and the ease time grows in step with it.
    pub smoothing: f32,
}

impl Default for CameraFeel {
    fn default() -> Self {
        CameraFeel {
            sensitivity: 1.0,
            speed: 1.0,
            smoothing: DEFAULT_SMOOTHING,
        }
    }
}

/// `CameraFeel` in the units `free_update` works in.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Feel {
    /// Degrees per pixel of mouse-look.
    sensitivity: f32,
    speed_scale: f32,
    /// The easing time constant, in seconds; 0 snaps.
    tau: f32,
}

impl Feel {
    fn of(feel: CameraFeel) -> Self {
        Feel {
            sensitivity: DEFAULT_SENSITIVITY * feel.sensitivity,
            speed_scale: feel.speed,
            tau: MOVEMENT_TIME_CONSTANT * feel.smoothing.max(0.0) / DEFAULT_SMOOTHING,
        }
    }
}

enum Mode {
    Orbit,
    Free(Pose),
}

/// Where the camera opens.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Start {
    /// Orbits the scene hands-free until the first input (`--orbit`).
    Orbit,
    /// Free flight from above the scene's own centre, the windowed default.
    Spawn,
    /// Free flight from a pose the host already knows, such as the viewpoint a
    /// place file was saved at.
    Pose(Pose),
}

/// Eased translation state for the free camera, carried across frames so
/// velocity ramps up/down instead of snapping: `velocity` is WASD's current
/// smoothed speed, `dolly_remaining` is how much of a wheel-notch hop is still
/// being eased in.
#[derive(Default)]
struct Motion {
    velocity: Vec3,
    dolly_remaining: Vec3,
}

/// Owns which of the two controllers is live and the free camera's current speed.
pub(crate) struct Controller {
    mode: Mode,
    orbit: bool,
    speed: f32,
    feel: Feel,
    motion: Motion,
}

impl Controller {
    /// `speed`: `None` picks `default_speed`'s scene-aware default.
    pub(crate) fn new(start: Start, bounds: &Bounds, speed: Option<f32>, sensitivity: f32) -> Self {
        Controller {
            mode: match start {
                Start::Orbit => Mode::Orbit,
                Start::Spawn => Mode::Free(Camera::spawn_pose(bounds)),
                Start::Pose(pose) => Mode::Free(pose),
            },
            orbit: matches!(start, Start::Orbit),
            speed: speed
                .unwrap_or_else(|| default_speed(bounds))
                .clamp(MIN_SPEED, MAX_SPEED),
            feel: Feel {
                sensitivity,
                speed_scale: 1.0,
                tau: MOVEMENT_TIME_CONSTANT,
            },
            motion: Motion::default(),
        }
    }

    pub(crate) fn speed(&self) -> f32 {
        self.speed
    }

    pub(crate) fn set_feel(&mut self, feel: CameraFeel) {
        self.feel = Feel::of(feel);
    }

    /// `F`/`Home`: snaps back to the spawn view (or, for `--orbit`, the initial
    /// orbit view) without leaving free mode.
    pub(crate) fn recentre(&mut self, bounds: &Bounds) {
        let pose = if self.orbit {
            Camera::orbit_pose(bounds, INITIAL_ORBIT_YAW)
        } else {
            Camera::spawn_pose(bounds)
        };
        self.mode = Mode::Free(pose);
        // Otherwise a stale velocity or half-eased dolly hop from before the
        // teleport would keep nudging the camera right after it recentres.
        self.motion = Motion::default();
    }

    /// Resyncs the free pose's orthographic view volume to its current
    /// distance from the scene, the moment orthographic mode switches on —
    /// see `Pose::ortho_scale`'s own doc comment for why this is only ever a
    /// reasonable starting guess (the mouse wheel is what actually corrects
    /// it from there, in `free_update` below), not a claim that this
    /// distance means anything in particular. A no-op while still orbiting:
    /// nothing to resync until the first input hands off to a free pose,
    /// which seeds its own `ortho_scale` the same way `Camera::orbit_pose`
    /// always has.
    pub(crate) fn sync_ortho_scale(&mut self, bounds: &Bounds) {
        if let Mode::Free(pose) = &mut self.mode {
            let distance = (pose.position - bounds.center()).length();
            pose.ortho_scale = camera::initial_ortho_scale(distance, pose.fov_degrees);
        }
    }

    /// Advances the camera by `dt` and returns what this frame draws from.
    ///
    /// `elapsed` is only read while still orbiting: the orbit is driven by wall
    /// time, so it turns at the same rate however many frames were drawn.
    /// `orthographic` changes what the mouse wheel does without the look
    /// button held — see `free_update`.
    pub(crate) fn update(
        &mut self,
        input: &mut Input,
        dt: Duration,
        elapsed: Duration,
        bounds: &Bounds,
        orthographic: bool,
    ) -> Viewpoint {
        if matches!(self.mode, Mode::Orbit) && input.requests_free_flight() {
            let yaw = Camera::orbit_yaw(elapsed);
            self.mode = Mode::Free(Camera::orbit_pose(bounds, yaw));
        }

        match &mut self.mode {
            Mode::Orbit => Viewpoint::Orbit(Camera::orbit_yaw(elapsed)),
            Mode::Free(pose) => {
                self.speed = free_update(
                    pose,
                    input,
                    dt,
                    self.speed,
                    self.feel,
                    &mut self.motion,
                    orthographic,
                );
                Viewpoint::Free(*pose)
            }
        }
    }
}

/// `--speed`'s default when the user doesn't pick one: the usual cruising speed,
/// scaled up for a scene too large to cross at it in reasonable time.
fn default_speed(bounds: &Bounds) -> f32 {
    let radius = bounds.radius();
    if radius > LARGE_SCENE_RADIUS {
        radius / LARGE_SCENE_SPEED_DIVISOR
    } else {
        DEFAULT_SPEED
    }
}

/// The free-flight step proper: mouse-look, keyboard movement, and the wheel's
/// roles (speed with the look button held; a dolly hop without it in
/// perspective, or the orthographic view volume's own zoom without it in
/// orthographic — see the wheel-handling block below). Kept free of `self`
/// so it's trivially callable from a pure unit test.
fn free_update(
    pose: &mut Pose,
    input: &mut Input,
    dt: Duration,
    speed: f32,
    feel: Feel,
    motion: &mut Motion,
    orthographic: bool,
) -> f32 {
    let (dx, dy) = input.take_mouse_delta();
    if input.look_button_down() {
        // `look_direction`'s yaw runs the opposite way from screen-right: with yaw=0
        // looking down -Z, increasing yaw turns the view towards -X (left in a Y-up
        // right-handed frame), so a rightward mouse move (dx > 0) has to *subtract*
        // from yaw to turn the view right, matching Studio instead of inverting it.
        pose.yaw -= (dx * feel.sensitivity).to_radians();
        // Pitch needs no such flip: `dy` grows downward, and `look_direction`'s
        // vertical component already grows as pitch shrinks, so dy < 0 (mouse up)
        // already looks up — this is a plain, uninverted Y axis.
        pose.pitch = (pose.pitch + (dy * feel.sensitivity).to_radians()).clamp(
            -MAX_PITCH_DEGREES.to_radians(),
            MAX_PITCH_DEGREES.to_radians(),
        );
    }

    let forward = look_direction(pose.yaw, pose.pitch);
    let mut speed = speed;
    let notches = input.take_wheel_notches();
    if notches != 0.0 {
        if input.look_button_down() {
            speed = (speed * SPEED_STEP.powf(notches)).clamp(MIN_SPEED, MAX_SPEED);
        } else if orthographic {
            // A parallel projection has no perspective divide to make
            // dollying the eye change apparent size — moving `pose.position`
            // here would do nothing visible and risk flying the eye straight
            // through geometry with no size cue at all (see
            // `Pose::ortho_scale`'s doc comment for why that's a real,
            // previously-reported bug). The wheel instead zooms the view
            // volume directly, using the same notch-to-factor feel
            // `SPEED_STEP` already gives perspective's own dolly.
            pose.ortho_scale = (pose.ortho_scale / SPEED_STEP.powf(notches))
                .clamp(MIN_ORTHO_SCALE, MAX_ORTHO_SCALE);
        } else {
            motion.dolly_remaining +=
                forward * speed * feel.speed_scale * ZOOM_SECONDS_PER_NOTCH * notches;
        }
    }
    // Ease the dolly hop in over a few frames instead of teleporting the full
    // notch distance in a single tick; `eased` is what's still left to cover.
    let eased = ease_towards(motion.dolly_remaining, Vec3::ZERO, dt, feel.tau);
    pose.position += motion.dolly_remaining - eased;
    motion.dolly_remaining = eased;

    let local = input.movement_vector();
    let target_velocity = if local == Vec3::ZERO {
        Vec3::ZERO
    } else {
        let right = horizontal_right(forward);
        let world = right * local.x + Vec3::Y * local.y + forward * local.z;
        let factor = if input.slow_down() {
            SHIFT_SLOW_FACTOR
        } else {
            1.0
        };
        world * speed * feel.speed_scale * factor
    };
    // Ease towards the target every frame, not just while a key is held, so
    // releasing WASD decelerates instead of stopping dead. The distance moved
    // is the exact integral of that curve (see `ease_and_advance`), not
    // `new_velocity * dt`: multiplying the *post*-ease velocity by the whole
    // step treats the velocity as constant over an interval where it is
    // actually curving, which over- or under-shoots by an amount that grows
    // with `dt`. At a rock-steady frame rate that shows up as a fixed bias
    // nobody notices, but real frame times wobble around their average, so
    // the bias itself wobbles frame to frame — jitter with no effect on the
    // reported FPS, since that only ever sees the average.
    let (velocity, displacement) = ease_and_advance(motion.velocity, target_velocity, dt, feel.tau);
    motion.velocity = velocity;
    pose.position += displacement;

    speed
}

/// Exponential ease of `current` towards `target`, closing ~63% of the
/// remaining gap per `tau` seconds. Framerate-independent: unlike
/// `current.lerp(target, factor)` applied once per frame (which drifts with
/// tick rate), many small `dt` steps converge to the same result as one big
/// step, since `exp(-a) * exp(-b) == exp(-(a + b))`.
fn ease_towards(current: Vec3, target: Vec3, dt: Duration, tau: f32) -> Vec3 {
    let t = 1.0 - (-dt.as_secs_f32() / tau).exp();
    current + (target - current) * t
}

/// `ease_towards`, plus the exact distance covered while `current` curves
/// towards `target` over `dt` — the closed-form integral of
/// `v(s) = target + (current - target) * exp(-s / tau)` from 0 to `dt`,
/// rather than approximating it with `new_velocity * dt` (a straight line
/// through the curve's *end* value, which is only exact once the ease has
/// settled). Splitting the same wall-clock interval into different `dt`s
/// still sums to this exact displacement, for the same reason `ease_towards`
/// itself is split-independent.
fn ease_and_advance(current: Vec3, target: Vec3, dt: Duration, tau: f32) -> (Vec3, Vec3) {
    let t = 1.0 - (-dt.as_secs_f32() / tau).exp();
    let new_value = current + (target - current) * t;
    let displacement = target * dt.as_secs_f32() - (target - current) * tau * t;
    (new_value, displacement)
}

// Mirrors `camera::direction`: forward always includes the vertical component of
// where the camera looks, matching Studio's "W flies you exactly where you're aimed".
fn look_direction(yaw: f32, pitch: f32) -> Vec3 {
    -Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    )
}

// Strafing stays level regardless of pitch: this is `forward × world up` with the
// vertical component already zero by construction, which is screen-right in a
// right-handed Y-up frame. Negating it instead swaps D and A, which reads as
// correct in isolation because both keys still move sideways.
fn horizontal_right(forward: Vec3) -> Vec3 {
    Vec3::new(-forward.z, 0.0, forward.x).normalize_or_zero()
}

#[cfg(test)]
#[path = "controller/tests.rs"]
mod tests;
