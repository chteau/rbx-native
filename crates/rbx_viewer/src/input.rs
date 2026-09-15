//! Raw device state for the free camera: which movement keys are held, whether
//! the look button is down, and the mouse/wheel motion accumulated since the
//! controller last looked.
//!
//! Hosts speak [`CameraInput`], never their own window library's event types:
//! the windowed viewer translates winit events (see `app.rs`) and an embedder
//! translates its own (see [`crate::Headless::input`]), so both drive the very
//! same controller.

use glam::Vec3;

/// One key of the free camera, named after what it does rather than what is
/// printed on it.
///
/// The caller maps *key positions*, not characters: the key W sits on is
/// [`CameraKey::Forward`] even on an AZERTY keyboard, where it is labelled Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraKey {
    Forward,
    Back,
    Left,
    Right,
    Up,
    Down,
    /// Studio's precision modifier (Shift): it *divides* the flight speed so a
    /// part can be lined up by hand, rather than sprinting.
    Slow,
}

/// One piece of camera input, in host-neutral terms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraInput {
    Key {
        key: CameraKey,
        pressed: bool,
    },
    /// The look button (the right mouse button) going down or up. Mouse motion
    /// only turns the view while it is held, as in Studio.
    LookButton(bool),
    /// Relative mouse motion in pixels, `dy` growing downward like every
    /// windowing system reports it.
    MouseLook {
        dx: f32,
        dy: f32,
    },
    /// Wheel motion in notches, positive away from the user.
    Wheel {
        notches: f32,
    },
    /// The host lost focus of the view: nothing may stay logically held, since
    /// the matching release event may never arrive.
    Release,
}

const MOVEMENT_KEYS: [CameraKey; 6] = [
    CameraKey::Forward,
    CameraKey::Back,
    CameraKey::Left,
    CameraKey::Right,
    CameraKey::Up,
    CameraKey::Down,
];

/// Polled once per frame by the camera controller, which then drains the deltas
/// it consumed — held keys and the look button stay until their own release
/// event, everything else (deltas) is one frame's worth only.
#[derive(Debug, Default)]
pub(crate) struct Input {
    keys: Vec<CameraKey>,
    look_button: bool,
    mouse_delta: (f32, f32),
    wheel_notches: f32,
}

impl Input {
    pub(crate) fn apply(&mut self, event: CameraInput) {
        match event {
            CameraInput::Key { key, pressed } => self.set_key(key, pressed),
            CameraInput::LookButton(down) => self.look_button = down,
            CameraInput::MouseLook { dx, dy } => {
                self.mouse_delta.0 += dx;
                self.mouse_delta.1 += dy;
            }
            CameraInput::Wheel { notches } => self.wheel_notches += notches,
            CameraInput::Release => *self = Input::default(),
        }
    }

    pub(crate) fn look_button_down(&self) -> bool {
        self.look_button
    }

    /// Drains the mouse motion accumulated since the last call. Draining
    /// unconditionally (even while the look button is up) keeps movement that
    /// happened before the button landed from causing a jump the instant it does.
    pub(crate) fn take_mouse_delta(&mut self) -> (f32, f32) {
        std::mem::take(&mut self.mouse_delta)
    }

    pub(crate) fn take_wheel_notches(&mut self) -> f32 {
        std::mem::take(&mut self.wheel_notches)
    }

    /// The trigger that ends the automatic orbit for good: the look button or
    /// any movement key, the instant either first appears.
    pub(crate) fn requests_free_flight(&self) -> bool {
        self.look_button || MOVEMENT_KEYS.iter().any(|key| self.key_held(*key))
    }

    pub(crate) fn slow_down(&self) -> bool {
        self.key_held(CameraKey::Slow)
    }

    /// Local-space movement direction (x right, y up, z forward), normalized so a
    /// diagonal isn't faster than a straight line — Studio does the same.
    pub(crate) fn movement_vector(&self) -> Vec3 {
        let axis = |positive: CameraKey, negative: CameraKey| {
            f32::from(self.key_held(positive)) - f32::from(self.key_held(negative))
        };
        let v = Vec3::new(
            axis(CameraKey::Right, CameraKey::Left),
            axis(CameraKey::Up, CameraKey::Down),
            axis(CameraKey::Forward, CameraKey::Back),
        );

        if v == Vec3::ZERO {
            v
        } else {
            v.normalize()
        }
    }

    fn set_key(&mut self, key: CameraKey, pressed: bool) {
        if pressed {
            if !self.key_held(key) {
                self.keys.push(key);
            }
        } else {
            self.keys.retain(|held| *held != key);
        }
    }

    fn key_held(&self, key: CameraKey) -> bool {
        self.keys.contains(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(keys: &[CameraKey]) -> Input {
        let mut input = Input::default();
        for key in keys {
            input.apply(CameraInput::Key {
                key: *key,
                pressed: true,
            });
        }
        input
    }

    #[test]
    fn the_movement_keys_map_to_their_own_axis() {
        for (key, expected) in [
            (CameraKey::Forward, Vec3::Z),
            (CameraKey::Back, -Vec3::Z),
            (CameraKey::Right, Vec3::X),
            (CameraKey::Left, -Vec3::X),
            (CameraKey::Up, Vec3::Y),
            (CameraKey::Down, -Vec3::Y),
        ] {
            assert_eq!(held(&[key]).movement_vector(), expected, "{key:?}");
        }
    }

    #[test]
    fn opposite_keys_held_together_cancel_out() {
        let input = held(&[CameraKey::Forward, CameraKey::Back]);
        assert_eq!(input.movement_vector(), Vec3::ZERO);
    }

    #[test]
    fn a_diagonal_is_normalized_not_faster() {
        let input = held(&[CameraKey::Forward, CameraKey::Right]);

        let v = input.movement_vector();
        assert!((v.length() - 1.0).abs() < 1e-6, "{v}");
        assert!((v.x - v.z).abs() < 1e-6, "not an even diagonal: {v}");
    }

    #[test]
    fn a_released_key_stops_moving() {
        let mut input = held(&[CameraKey::Forward]);
        input.apply(CameraInput::Key {
            key: CameraKey::Forward,
            pressed: false,
        });

        assert_eq!(input.movement_vector(), Vec3::ZERO);
    }

    #[test]
    fn a_repeated_press_is_not_held_twice() {
        let input = held(&[CameraKey::Forward, CameraKey::Forward]);
        assert_eq!(input.movement_vector(), Vec3::Z);
    }

    #[test]
    fn a_movement_key_or_the_look_button_requests_free_flight() {
        let mut input = Input::default();
        assert!(!input.requests_free_flight());

        input.apply(CameraInput::Key {
            key: CameraKey::Forward,
            pressed: true,
        });
        assert!(input.requests_free_flight());
        input.apply(CameraInput::Key {
            key: CameraKey::Forward,
            pressed: false,
        });
        assert!(!input.requests_free_flight());

        input.apply(CameraInput::LookButton(true));
        assert!(input.requests_free_flight());
    }

    #[test]
    fn the_slow_modifier_alone_does_not_request_free_flight() {
        let input = held(&[CameraKey::Slow]);

        assert!(input.slow_down());
        assert!(!input.requests_free_flight());
        assert_eq!(input.movement_vector(), Vec3::ZERO);
    }

    #[test]
    fn mouse_and_wheel_deltas_accumulate_and_drain() {
        let mut input = Input::default();
        input.apply(CameraInput::MouseLook { dx: 3.0, dy: -2.0 });
        input.apply(CameraInput::MouseLook { dx: 1.0, dy: 1.0 });
        assert_eq!(input.take_mouse_delta(), (4.0, -1.0));
        assert_eq!(input.take_mouse_delta(), (0.0, 0.0));

        input.apply(CameraInput::Wheel { notches: 1.0 });
        input.apply(CameraInput::Wheel { notches: 1.0 });
        assert_eq!(input.take_wheel_notches(), 2.0);
        assert_eq!(input.take_wheel_notches(), 0.0);
    }

    #[test]
    fn releasing_drops_every_key_button_and_delta() {
        let mut input = held(&[CameraKey::Forward, CameraKey::Slow]);
        input.apply(CameraInput::LookButton(true));
        input.apply(CameraInput::MouseLook { dx: 5.0, dy: 5.0 });
        input.apply(CameraInput::Wheel { notches: 3.0 });

        input.apply(CameraInput::Release);

        assert_eq!(input.movement_vector(), Vec3::ZERO);
        assert!(!input.slow_down());
        assert!(!input.look_button_down());
        assert!(!input.requests_free_flight());
        assert_eq!(input.take_mouse_delta(), (0.0, 0.0));
        assert_eq!(input.take_wheel_notches(), 0.0);
    }
}
