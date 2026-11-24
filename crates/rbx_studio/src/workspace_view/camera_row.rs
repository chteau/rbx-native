//! Studio's "New Studio Camera Controls" beta, the view's side of it: the
//! control row under the View Selector (the camera mode and the speed
//! multiplier), Focus framing the selection the way Studio 0.748 does, and
//! the feel handed to the render thread carrying the beta's camera style.
//!
//! The camera itself flies on the render thread, see
//! `rbx_viewer::CameraStyle`.

use glam::Vec3;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_viewer::{CameraFeel, CameraMode, CameraStyle};

use super::WorkspaceView;
use crate::tokens;

/// Studio's beta of the same name, see `beta::FEATURES`.
pub(crate) const BETA_ID: &str = "new-studio-camera-controls";

/// The row's place below the orientation indicator, in px at the viewport
/// text size: the indicator's 96 px square plus the inset both share.
const BELOW_INDICATOR: f32 = 96.0 + 12.0;

impl WorkspaceView {
    /// How the free camera turns, flies and eases: the render thread owns
    /// the viewer, so it takes the change between two frames. The style is
    /// the view's own, from the beta and the row's mode.
    pub(crate) fn set_camera_feel(&mut self, feel: CameraFeel) {
        self.feel = feel;
        self.pump.camera_feel(CameraFeel {
            style: self
                .camera_mode
                .map_or(CameraStyle::Classic, CameraStyle::Studio),
            ..feel
        });
    }

    fn set_camera_mode(&mut self, mode: CameraMode, cx: &mut Context<Self>) {
        self.camera_mode = Some(mode);
        self.set_camera_feel(self.feel);
        cx.notify();
    }

    /// Focus on the box `low..high`: the beta camera glides to where Studio
    /// frames it and orbits its centre; the classic one snaps to fit the
    /// box's bounding sphere.
    pub(crate) fn focus_box(&self, low: Vec3, high: Vec3) {
        let Some(pose) = self.view else {
            return;
        };
        let centre = (low + high) * 0.5;
        let forward = pose.basis().0;
        if self.camera_mode.is_none() {
            let reach = ((high - low).length() * 0.5).max(1.0);
            let distance = reach / (pose.fov_degrees.to_radians() * 0.5).sin();
            let eye = centre - forward * distance;
            self.look_at(eye.to_array(), centre.to_array());
            return;
        }
        let corners = (0..8).map(|i| {
            Vec3::new(
                if i & 1 == 0 { low.x } else { high.x },
                if i & 2 == 0 { low.y } else { high.y },
                if i & 4 == 0 { low.z } else { high.z },
            )
        });
        let distance = rbx_viewer::focus_distance(
            corners,
            centre,
            pose.basis(),
            pose.fov_degrees,
            self.aspect(),
        );
        let eye = centre - forward * distance.max(0.1);
        self.pump.focus_on(eye.to_array(), centre.to_array());
    }

    /// The mode picker and the multiplier, under the orientation indicator
    /// as in Studio, while the beta is on.
    pub(super) fn camera_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let mode = self.camera_mode?;
        let k = crate::tokens::viewport_scale();
        let top = if self.axis_indicator {
            BELOW_INDICATOR * k
        } else {
            8.0
        };
        let speed = match self.multiplier {
            Some(multiplier) => format!("{multiplier:.2}x"),
            None => format!("{} studs/s", self.speed.round() as i64),
        };
        let chip = || {
            div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .bg(tokens::viewport_badge())
                .text_size(crate::tokens::viewport_text(12.))
                .text_color(tokens::viewport_badge_text())
        };
        Some(
            div()
                .absolute()
                .top(px(top))
                .right_2()
                .flex()
                .gap_1()
                .child(
                    chip()
                        .id("camera-mode")
                        .cursor_pointer()
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(
                                "Camera mode: Adaptive orbits after Focus and flies after a movement key, FPS always flies, Orbit always orbits",
                            )
                            .build(window, cx)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                view.set_camera_mode(next_mode(mode), cx);
                            }),
                        )
                        .child(mode_label(mode)),
                )
                .child(chip().when(self.multiplier.is_some(), |chip| chip.min_w(px(44.0 * k))).child(speed)),
        )
    }
}

/// The modes in Studio's dropdown order, a click stepping to the next.
fn next_mode(mode: CameraMode) -> CameraMode {
    match mode {
        CameraMode::Adaptive => CameraMode::Fps,
        CameraMode::Fps => CameraMode::Orbit,
        CameraMode::Orbit => CameraMode::Adaptive,
    }
}

fn mode_label(mode: CameraMode) -> &'static str {
    match mode {
        CameraMode::Adaptive => "Adaptive",
        CameraMode::Fps => "FPS",
        CameraMode::Orbit => "Orbit",
    }
}

#[cfg(test)]
mod tests {
    use super::{mode_label, next_mode};
    use rbx_viewer::CameraMode;

    #[test]
    fn a_click_walks_studios_three_modes_and_back() {
        let mut mode = CameraMode::Adaptive;
        let mut seen = Vec::new();
        for _ in 0..3 {
            mode = next_mode(mode);
            seen.push(mode_label(mode));
        }
        assert_eq!(seen, ["FPS", "Orbit", "Adaptive"]);
    }
}
