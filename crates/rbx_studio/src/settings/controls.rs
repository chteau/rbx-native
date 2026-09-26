//! How the camera and the draggers feel: the free camera's look speed,
//! flight speed and smoothing (`"camera"`), and the Snap popover's two
//! increments (`"snap"`).

use rbx_viewer::CameraFeel;

/// The range the Settings window's sliders offer, and what a hand-edited
/// file is clamped into.
pub(crate) const FEEL_SCALE_RANGE: (f32, f32) = (0.1, 4.0);

/// The camera's feel and the snap increments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Controls {
    pub(crate) camera: CameraFeel,
    /// Studs a move or scale drag rounds to.
    pub(crate) move_increment: f32,
    /// Degrees a rotate drag rounds to.
    pub(crate) rotate_increment: f32,
}

impl Default for Controls {
    /// The increments are `transform::Transform`'s own defaults.
    fn default() -> Self {
        let transform = crate::transform::Transform::default();
        Controls {
            camera: CameraFeel::default(),
            move_increment: transform.translate.increment,
            rotate_increment: transform.rotate.increment,
        }
    }
}

impl Controls {
    /// Reads `"camera"` and `"snap"`, each value that is missing, not a
    /// number, or out of range left at (or clamped to) its default's range.
    pub(super) fn read(value: &serde_json::Value) -> Controls {
        let number = |object: &str, key: &str| {
            value
                .get(object)?
                .get(key)?
                .as_f64()
                .map(|n| n as f32)
                .filter(|n| n.is_finite())
        };
        let defaults = Controls::default();
        let scale = |key, default| {
            number("camera", key)
                .map(|n: f32| n.clamp(FEEL_SCALE_RANGE.0, FEEL_SCALE_RANGE.1))
                .unwrap_or(default)
        };
        // A zero or negative increment would be a grid nothing lands on.
        let increment = |key, default| number("snap", key).filter(|n| *n > 0.).unwrap_or(default);
        Controls {
            camera: CameraFeel {
                sensitivity: scale("sensitivity", defaults.camera.sensitivity),
                speed: scale("speed", defaults.camera.speed),
                smoothing: number("camera", "smoothing")
                    .map(|n| n.clamp(0., 1.))
                    .unwrap_or(defaults.camera.smoothing),
            },
            move_increment: increment("move", defaults.move_increment),
            rotate_increment: increment("rotate", defaults.rotate_increment),
        }
    }

    pub(super) fn json(self) -> [serde_json::Value; 2] {
        [
            serde_json::json!({
                "sensitivity": self.camera.sensitivity,
                "speed": self.camera.speed,
                "smoothing": self.camera.smoothing,
            }),
            serde_json::json!({
                "move": self.move_increment,
                "rotate": self.rotate_increment,
            }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_without_them_reads_the_defaults() {
        assert_eq!(Controls::read(&serde_json::json!({})), Controls::default());
        let defaults = Controls::default();
        assert_eq!(defaults.move_increment, 1.);
        assert_eq!(defaults.rotate_increment, 45.);
    }

    #[test]
    fn what_is_written_reads_back() {
        let controls = Controls {
            camera: CameraFeel {
                sensitivity: 2.5,
                speed: 0.5,
                smoothing: 0.,
            },
            move_increment: 0.25,
            rotate_increment: 15.,
        };
        let [camera, snap] = controls.json();
        let value = serde_json::json!({ "camera": camera, "snap": snap });
        assert_eq!(Controls::read(&value), controls);
    }

    #[test]
    fn hand_edited_values_are_clamped_or_dropped() {
        let value = serde_json::json!({
            "camera": { "sensitivity": 40, "speed": -1, "smoothing": 3 },
            "snap": { "move": 0, "rotate": "fast" },
        });
        let read = Controls::read(&value);
        assert_eq!(read.camera.sensitivity, FEEL_SCALE_RANGE.1);
        assert_eq!(read.camera.speed, FEEL_SCALE_RANGE.0);
        assert_eq!(read.camera.smoothing, 1.);
        assert_eq!(read.move_increment, 1.);
        assert_eq!(read.rotate_increment, 45.);
    }
}
