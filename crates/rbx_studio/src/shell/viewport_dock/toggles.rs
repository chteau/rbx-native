//! Which view settings the Camera, Overlays and Dragging & snapping
//! sections list, in their order, each with the shell state it reads and
//! the setter its row calls.

use crate::settings::DraggerSettings;

use super::super::Shell;
use super::Toggle;

impl Shell {
    /// The Camera, Overlays and Dragging & snapping switches, in the
    /// order their sections list them. A new setting is one more entry
    /// here and nothing else.
    pub(super) fn viewport_toggles(&self) -> [Vec<Toggle>; 3] {
        let camera: Vec<Toggle> = vec![
            ("Orthographic", self.orthographic, Shell::set_orthographic),
            (
                "Orientation Indicator",
                self.axis_indicator,
                Shell::set_axis_indicator,
            ),
        ];
        // Studio's dragger settings, under Studio's own names (see
        // `settings::DraggerSettings`).
        let overlays: Vec<Toggle> = vec![
            (
                "Show Light Guides",
                self.light_guides_shown(),
                // A flip, and the row only ever asks for the opposite of
                // what it shows, so the value it passes is already implied.
                |shell, _, cx| shell.toggle_light_guides(cx),
            ),
            (
                "Show Hover Ruler",
                self.dragger().show_hover_ruler,
                |shell, show_hover_ruler, cx| {
                    let settings = DraggerSettings {
                        show_hover_ruler,
                        ..shell.dragger()
                    };
                    shell.set_dragger(settings, cx)
                },
            ),
            (
                "Show Measurement",
                self.dragger().show_measurement,
                |shell, show_measurement, cx| {
                    let settings = DraggerSettings {
                        show_measurement,
                        ..shell.dragger()
                    };
                    shell.set_dragger(settings, cx)
                },
            ),
            (
                "Hide Selection Box Behind Parts",
                self.selection_occluded,
                Shell::set_selection_occluded,
            ),
        ];
        let dragging: Vec<Toggle> = vec![
            (
                "Snap to Parts",
                self.dragger().snap_to_parts,
                |shell, snap_to_parts, cx| {
                    let settings = DraggerSettings {
                        snap_to_parts,
                        ..shell.dragger()
                    };
                    shell.set_dragger(settings, cx)
                },
            ),
            (
                "Align Dragged Objects",
                self.dragger().align_dragged_objects,
                |shell, align_dragged_objects, cx| {
                    let settings = DraggerSettings {
                        align_dragged_objects,
                        ..shell.dragger()
                    };
                    shell.set_dragger(settings, cx)
                },
            ),
            (
                "Show Target Snap",
                self.dragger().show_target_snap,
                |shell, show_target_snap, cx| {
                    let settings = DraggerSettings {
                        show_target_snap,
                        ..shell.dragger()
                    };
                    shell.set_dragger(settings, cx)
                },
            ),
            (
                "Show Dragged Point",
                self.dragger().show_dragged_point,
                |shell, show_dragged_point, cx| {
                    let settings = DraggerSettings {
                        show_dragged_point,
                        ..shell.dragger()
                    };
                    shell.set_dragger(settings, cx)
                },
            ),
        ];
        [camera, overlays, dragging]
    }
}
