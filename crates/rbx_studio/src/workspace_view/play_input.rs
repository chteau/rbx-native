//! Play (local): the movement keys, the right-drag and the wheel go to the
//! character and the classic third-person camera following it, not to the
//! free camera.

/// Radians of camera turn per pixel of right-drag.
const LOOK_PER_PIXEL: f32 = 0.008;
/// `StarterPlayer.CameraMinZoomDistance` and `CameraMaxZoomDistance`'s
/// defaults.
const ZOOM: (f32, f32) = (0.5, 400.0);
/// How far one wheel notch zooms, as a fraction of the distance.
const ZOOM_PER_NOTCH: f32 = 0.15;
/// The camera looks at this much above the root, about the head.
pub(crate) const FOCUS_ABOVE_ROOT: f32 = 1.5;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PlayInput {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    pub(crate) jump: bool,
    /// The camera's turn about the character, 0 looking along -Z.
    yaw: f32,
    pitch: f32,
    /// Studs from the focus.
    zoom: f32,
}

impl Default for PlayInput {
    fn default() -> Self {
        PlayInput {
            forward: false,
            back: false,
            left: false,
            right: false,
            jump: false,
            yaw: 0.0,
            pitch: -0.35,
            zoom: 12.5,
        }
    }
}

impl PlayInput {
    /// Takes a movement key (WASD, ZQSD on AZERTY, the arrows, Space);
    /// `false` for any other key.
    pub(crate) fn key(&mut self, key: &str, pressed: bool, azerty: bool) -> bool {
        let held = match key {
            "w" | "up" => &mut self.forward,
            "z" if azerty => &mut self.forward,
            "s" | "down" => &mut self.back,
            "a" if !azerty => &mut self.left,
            "q" if azerty => &mut self.left,
            "left" => &mut self.left,
            "d" | "right" => &mut self.right,
            "space" => &mut self.jump,
            _ => return false,
        };
        *held = pressed;
        true
    }

    /// Nothing stays held once the view loses the keyboard.
    pub(crate) fn release(&mut self) {
        (self.forward, self.back, self.left, self.right, self.jump) =
            (false, false, false, false, false);
    }

    pub(crate) fn look(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * LOOK_PER_PIXEL;
        self.pitch = (self.pitch - dy * LOOK_PER_PIXEL).clamp(-1.4, 1.4);
    }

    pub(crate) fn wheel(&mut self, notches: f32) {
        self.zoom = (self.zoom * (1.0 - ZOOM_PER_NOTCH).powf(notches)).clamp(ZOOM.0, ZOOM.1);
    }

    /// Where the camera looks on the ground, and to its right.
    fn axes(&self) -> ([f32; 3], [f32; 3]) {
        let (sin, cos) = self.yaw.sin_cos();
        ([-sin, 0.0, -cos], [cos, 0.0, -sin])
    }

    /// The walk direction the held keys ask for, relative to the camera,
    /// in world space; zero with none held.
    pub(crate) fn direction(&self) -> [f64; 3] {
        let (ahead, right) = self.axes();
        let along = f32::from(u8::from(self.forward)) - f32::from(u8::from(self.back));
        let side = f32::from(u8::from(self.right)) - f32::from(u8::from(self.left));
        let v = [0, 1, 2].map(|i| ahead[i] * along + right[i] * side);
        let length = (v[0] * v[0] + v[2] * v[2]).sqrt();
        if length < 1e-6 {
            return [0.0; 3];
        }
        v.map(|c| f64::from(c / length))
    }

    /// The camera's eye for a focus point.
    pub(crate) fn eye(&self, focus: [f32; 3]) -> [f32; 3] {
        let (ahead, _) = self.axes();
        let (sin, cos) = self.pitch.sin_cos();
        let look = [ahead[0] * cos, sin, ahead[2] * cos];
        [0, 1, 2].map(|i| focus[i] - look[i] * self.zoom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn w_walks_where_the_camera_looks() {
        let mut input = PlayInput::default();
        assert!(input.key("w", true, false));
        assert_eq!(input.direction(), [0.0, 0.0, -1.0]);
        input.look(-(std::f32::consts::FRAC_PI_2 / LOOK_PER_PIXEL), 0.0);
        let [x, _, z] = input.direction();
        assert!((x + 1.0).abs() < 1e-5 && z.abs() < 1e-5, "{x} {z}");
        assert!(!input.key("e", true, false));
    }

    #[test]
    fn the_eye_sits_behind_and_above_the_focus() {
        let input = PlayInput::default();
        let eye = input.eye([0.0, 0.0, 0.0]);
        assert!(eye[2] > 10.0 && eye[1] > 3.0, "{eye:?}");
    }
}
