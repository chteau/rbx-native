//! Studio's reworked edit camera, the "New Studio Camera Controls (Updated)"
//! beta, as Studio 0.748 flies it with that beta on.
//!
//! Every constant below was read back from `workspace.CurrentCamera.CFrame`
//! and `Focus` in real Studio, sampled each `RenderStepped`, with inputs
//! sent through XTest (a 759x395 viewport, default Studio settings). The
//! camera keeps a focus point `depth` studs straight ahead, as Studio's own
//! `Camera.Focus` does, and is either orbiting it or flying past it:
//!
//! - Orbiting (after Focus, or a script that aims the camera at its focus):
//!   the right button turns the view about the focus, the wheel zooms
//!   towards it, and the fly speed is the speed multiplier times `depth`
//!   (2.00x at 30 studs flew at 60 studs/s; after Focus on a 4-stud cube,
//!   6.08 studs off, at 12.1).
//! - Flying (after a movement key in Adaptive mode, always in FPS mode):
//!   the right button turns the view in place, the wheel zooms towards the
//!   surface under the cursor, and the speed stays what it was.
//!
//! Orbit mode never flies: the movement keys carry the focus along.

use std::time::Duration;

use glam::Vec3;

use super::{ease_and_advance, eased, finite, horizontal_right, look_direction, Feel, Room};
use crate::camera::Pose;
use crate::input::Input;

/// Which of the beta's three camera modes is live, Studio's control row
/// under the View Selector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    /// Orbits after Focus, flies after a movement key: Studio's default.
    #[default]
    Adaptive,
    /// Always flies; Focus frames the selection but leaves the speed alone.
    Fps,
    /// Always orbits, the movement keys included.
    Orbit,
}

/// Which camera the editor flies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraStyle {
    /// This renderer's own free camera.
    #[default]
    Classic,
    /// The beta's camera, in one of its modes.
    Studio(CameraMode),
}

/// Degrees the view turns per pixel of right-button drag, orbiting or not,
/// either axis: 100 px turned it 65.0°, 50 px pitched it 32.5°.
const DEGREES_PER_PIXEL: f32 = 0.65;
/// One wheel notch multiplies the distance to what it zooms towards by
/// `2^-ZOOM_DOUBLINGS`: 30 studs went to 26.12, 22.74 and 19.79, and 8
/// notches to 9.90.
const ZOOM_DOUBLINGS: f32 = 0.2;
/// How the zoom and Focus glide in: the gap left closed exponentially with
/// this time constant (0.036 s, settling in about a quarter of a second).
const GLIDE_TAU: f32 = 0.036;
/// The keys' ease: up to speed with a 0.137 s time constant, and back to rest
/// with 0.040 s ("keyboard flight now stops quickly").
const RAMP_TAU: f32 = 0.137;
const STOP_TAU: f32 = 0.040;
/// Shift's share of the fly speed (Studio's Shift Key Multiplier default).
const SHIFT_FACTOR: f32 = 0.2;
/// Alt + right drag dollies: the distance doubles every this many pixels,
/// right or up (100 px right went 30 to 47.62 studs, 50 px down to 23.81).
const DOLLY_PIXELS: f32 = 150.0;
/// Right button + wheel: each notch multiplies the speed by `2^(1/4)`
/// (2.00x, 2.37x, 2.83x).
const SPEED_DOUBLINGS: f32 = 0.25;
/// The speed multiplier Studio starts at, and the depth the camera assumes
/// when nothing told it one (a place camera with no focus ahead of it).
pub(crate) const DEFAULT_MULTIPLIER: f32 = 2.0;
const DEFAULT_DEPTH: f32 = 30.0;
/// Below this an aim point is the eye's own look direction, not a focus.
const MIN_DEPTH: f32 = 1.5;
const MIN_MULTIPLIER: f32 = 0.01;
const MAX_MULTIPLIER: f32 = 1000.0;
const MAX_PITCH: f32 = 89.0;

/// The beta camera's state between frames.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StudioCamera {
    pub(crate) mode: CameraMode,
    depth: f32,
    orbiting: bool,
    pub(crate) multiplier: f32,
    /// Studs per second while flying, frozen from the last orbit in Adaptive
    /// mode and the user's own in FPS mode.
    speed: f32,
    velocity: Vec3,
    /// Where a zoom, Focus or view jump is gliding the eye to.
    glide: Option<Glide>,
}

#[derive(Debug, Clone, Copy)]
struct Glide {
    position: Vec3,
    yaw: f32,
    pitch: f32,
}

impl StudioCamera {
    pub(crate) fn new(mode: CameraMode) -> Self {
        StudioCamera {
            mode,
            depth: DEFAULT_DEPTH,
            orbiting: mode == CameraMode::Orbit,
            multiplier: DEFAULT_MULTIPLIER,
            speed: DEFAULT_MULTIPLIER * DEFAULT_DEPTH,
            velocity: Vec3::ZERO,
            glide: None,
        }
    }

    /// A new mode keeps the multiplier and where the camera is.
    pub(crate) fn set_mode(&mut self, mode: CameraMode) {
        self.mode = mode;
        self.orbiting = match mode {
            CameraMode::Orbit => true,
            CameraMode::Fps => false,
            CameraMode::Adaptive => self.orbiting,
        };
    }

    /// Studs per second the movement keys fly at right now.
    pub(crate) fn speed(&self) -> f32 {
        if self.orbiting && self.mode != CameraMode::Fps {
            self.multiplier * self.depth
        } else {
            self.speed
        }
    }

    /// The multiplier Studio's speed field shows, `None` in FPS mode, whose
    /// field is in studs per second.
    pub(crate) fn multiplier(&self) -> Option<f32> {
        (self.mode != CameraMode::Fps).then_some(self.multiplier)
    }

    /// The camera was put at `pose` aiming at a point `depth` ahead (a place
    /// camera, a script): Studio orbits that point when there is one, and
    /// flies on from there when the aim is only a direction.
    pub(crate) fn place(&mut self, depth: f32) {
        self.glide = None;
        self.velocity = Vec3::ZERO;
        if depth >= MIN_DEPTH {
            self.depth = depth;
            self.orbiting = self.mode != CameraMode::Fps;
        } else {
            self.orbiting = self.mode == CameraMode::Orbit;
        }
    }

    /// Focus (and the View Selector's jumps): glide to `target`, a pose
    /// looking at a point `depth` ahead, and orbit it, at a speed that fits
    /// what was framed, except in FPS mode, which keeps flying at its own.
    pub(crate) fn frame(&mut self, target: Pose, depth: f32) {
        self.glide = Some(Glide {
            position: target.position,
            yaw: target.yaw,
            pitch: target.pitch,
        });
        self.velocity = Vec3::ZERO;
        self.depth = depth.max(MIN_DEPTH);
        if self.mode != CameraMode::Fps {
            self.orbiting = true;
        }
    }

    /// One frame. Returns the fly speed in studs per second, as the classic
    /// step does.
    pub(crate) fn step(
        &mut self,
        pose: &mut Pose,
        input: &mut Input,
        dt: Duration,
        feel: Feel,
        room: Room,
    ) -> f32 {
        let before = *pose;
        let (dx, dy) = input.take_mouse_delta();
        let looking = input.look_button_down();
        let notches = input.take_wheel_notches();
        let keys = input.movement_vector();
        if looking || input.pan_studs_per_pixel().is_some() || keys != Vec3::ZERO {
            self.glide = None;
        }

        if looking && input.dolly_held() {
            self.dolly_by((dx - dy) / DOLLY_PIXELS, pose);
        } else if looking {
            self.turn(pose, dx, dy, feel);
        }
        if let Some(studs_per_pixel) = input.pan_studs_per_pixel() {
            let rate = match (self.orbiting, room.height) {
                (true, Some(height)) if height > 0.0 => {
                    2.0 * self.depth * (pose.fov_degrees.to_radians() * 0.5).tan() / height
                }
                _ => studs_per_pixel,
            };
            let (_, right, up) = pose.basis();
            pose.position += (up * dy - right * dx) * rate;
        }
        if notches != 0.0 {
            if looking {
                self.change_speed(notches);
            } else {
                self.zoom(pose, notches, room);
            }
        }
        if let Some(glide) = self.glide {
            let t = eased(dt, GLIDE_TAU * feel.tau / super::MOVEMENT_TIME_CONSTANT);
            pose.position += (glide.position - pose.position) * t;
            pose.yaw += wrapped(glide.yaw - pose.yaw) * t;
            pose.pitch += (glide.pitch - pose.pitch) * t;
            if (glide.position - pose.position).length() < 1e-4
                && wrapped(glide.yaw - pose.yaw).abs() < 1e-5
                && (glide.pitch - pose.pitch).abs() < 1e-5
            {
                pose.position = glide.position;
                pose.yaw = glide.yaw;
                pose.pitch = glide.pitch;
                self.glide = None;
            }
        }
        self.fly(pose, input, keys, dt, feel);

        if !finite(pose) {
            *pose = before;
            self.velocity = Vec3::ZERO;
            self.glide = None;
        }
        self.speed()
    }

    fn focus(&self, pose: &Pose) -> Vec3 {
        pose.position + pose.basis().0 * self.depth
    }

    fn turn(&mut self, pose: &mut Pose, dx: f32, dy: f32, feel: Feel) {
        let scale = DEGREES_PER_PIXEL * feel.sensitivity / super::DEFAULT_SENSITIVITY;
        let focus = self.focus(pose);
        pose.yaw -= (dx * scale).to_radians();
        pose.pitch = (pose.pitch + (dy * scale).to_radians())
            .clamp(-MAX_PITCH.to_radians(), MAX_PITCH.to_radians());
        if self.orbiting {
            pose.position = focus - pose.basis().0 * self.depth;
        }
    }

    fn dolly_by(&mut self, doublings: f32, pose: &mut Pose) {
        let focus = self.focus(pose);
        self.depth = (self.depth * doublings.exp2()).max(MIN_DEPTH * 0.1);
        pose.position = focus - pose.basis().0 * self.depth;
    }

    fn change_speed(&mut self, notches: f32) {
        let factor = (notches * SPEED_DOUBLINGS).exp2();
        self.multiplier = (self.multiplier * factor).clamp(MIN_MULTIPLIER, MAX_MULTIPLIER);
        self.speed = (self.speed * factor).clamp(super::MIN_SPEED, super::MAX_SPEED);
    }

    /// The wheel: towards the focus while orbiting, towards the surface
    /// under the cursor while flying (or, over empty sky, the point on the
    /// cursor's ray as far out as the focus). Logarithmic, so it slows as it
    /// nears and never passes through.
    fn zoom(&mut self, pose: &Pose, notches: f32, room: Room) {
        let factor = (-notches * ZOOM_DOUBLINGS).exp2();
        let from = self.glide.map_or(pose.position, |glide| glide.position);
        let (yaw, pitch) = self
            .glide
            .map_or((pose.yaw, pose.pitch), |glide| (glide.yaw, glide.pitch));
        if self.orbiting {
            let focus = from + look_direction(yaw, pitch) * self.depth;
            self.depth *= factor;
            self.glide = Some(Glide {
                position: focus - look_direction(yaw, pitch) * self.depth,
                yaw,
                pitch,
            });
            return;
        }
        let (direction, hit) = room
            .cursor
            .map_or((look_direction(yaw, pitch), None), |cursor| {
                (cursor.direction, cursor.hit)
            });
        let point = hit.unwrap_or(from + direction * self.depth);
        let position = point + (from - point) * factor;
        self.depth = (point - position).length().max(MIN_DEPTH * 0.1);
        self.glide = Some(Glide {
            position,
            yaw,
            pitch,
        });
    }

    fn fly(&mut self, pose: &mut Pose, input: &Input, keys: Vec3, dt: Duration, feel: Feel) {
        if keys != Vec3::ZERO && self.mode == CameraMode::Adaptive && self.orbiting {
            self.speed = self.multiplier * self.depth;
            self.orbiting = false;
        }
        let target = if keys == Vec3::ZERO {
            Vec3::ZERO
        } else {
            let forward = pose.basis().0;
            let right = horizontal_right(forward);
            let world = right * keys.x + Vec3::Y * keys.y + forward * keys.z;
            let slow = if input.slow_down() { SHIFT_FACTOR } else { 1.0 };
            world * self.speed() * feel.speed_scale * slow
        };
        let tau = if target == Vec3::ZERO {
            STOP_TAU
        } else {
            RAMP_TAU
        };
        let tau = tau * feel.tau / super::MOVEMENT_TIME_CONSTANT;
        let (velocity, displacement) = ease_and_advance(self.velocity, target, dt, tau);
        self.velocity = velocity;
        pose.position += displacement;
    }
}

/// `angle` brought into `(-π, π]`, so a glide turns the short way round.
fn wrapped(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (angle + PI).rem_euclid(TAU) - PI
}

/// How far from a box Studio's Focus puts the camera: the box's corners,
/// seen along `forward`, fill `FOCUS_FILL` of the view on whichever axis
/// they reach first. Returns the distance from the box's centre to the eye.
///
/// Studio's "'Zoom To' - Viewport Fill Percentage" setting, 70 by default.
/// Fits every measurement: a 4-stud cube framed at 6.081 studs, a 10x2x2
/// part at 6.308, a 1-stud cube at 1.520 and a 200x80x2 wall at 107.18, on a
/// 759x395 view with a 70° field of view, the camera looking down -Z.
pub fn focus_distance(
    corners: impl IntoIterator<Item = Vec3>,
    centre: Vec3,
    basis: (Vec3, Vec3, Vec3),
    fov_degrees: f32,
    aspect: f32,
) -> f32 {
    const FOCUS_FILL: f32 = 0.7;
    let (forward, right, up) = basis;
    let tan_v = (fov_degrees.to_radians() * 0.5).tan() * FOCUS_FILL;
    let tan_h = tan_v * aspect;
    corners
        .into_iter()
        .map(|corner| {
            let offset = corner - centre;
            let toward = -offset.dot(forward);
            toward + (offset.dot(right).abs() / tan_h).max(offset.dot(up).abs() / tan_v)
        })
        .fold(0.0, f32::max)
}

#[cfg(test)]
#[path = "studio/tests.rs"]
mod tests;
