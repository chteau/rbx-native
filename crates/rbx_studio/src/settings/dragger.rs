//! The dragger guides' switches, each named for and defaulting to the Studio
//! setting it mirrors (see `crate::dragger`). Stored as one `"dragger"`
//! object in the settings file.

/// Which of the dragger guides show, and whether a drag snaps to parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DraggerSettings {
    /// `Studio.DraggerShowHoverRuler`: the white ruler and dot under the
    /// cursor with nothing held.
    pub(crate) show_hover_ruler: bool,
    /// `Studio.DraggerShowTargetSnap`: a free drag's ruler, or the face
    /// alignments it snapped to.
    pub(crate) show_target_snap: bool,
    /// `Studio.DraggerShowMeasurement`: the distance label on a handle drag
    /// (and this editor's own readout on a free one).
    pub(crate) show_measurement: bool,
    /// `Studio.DraggerShowDraggedPoint`: the yellow dot on the point a free
    /// drag holds, and its bar down to the face.
    pub(crate) show_dragged_point: bool,
    /// Studio's Snap to Parts toggle (`DraggerService.PartSnapEnabled`): a
    /// free drag aligns with the face it lands on, a handle drag with the
    /// faces of the parts along its axis. `Shift` suspends it.
    pub(crate) snap_to_parts: bool,
    /// Studio's Align Dragged Objects toggle
    /// (`StudioService.AlignDraggedObjects`): a free drag turns the
    /// selection to lie on the face it lands on. `Alt` suspends it.
    pub(crate) align_dragged_objects: bool,
}

impl Default for DraggerSettings {
    /// Studio ships with every one of these on. Align Dragged Objects is
    /// the one it hides: its default is read off Studio's own test fixture
    /// and the Hold Orientation hint its drags always show.
    fn default() -> Self {
        DraggerSettings {
            show_hover_ruler: true,
            show_target_snap: true,
            show_measurement: true,
            show_dragged_point: true,
            snap_to_parts: true,
            align_dragged_objects: true,
        }
    }
}

const KEYS: [&str; 6] = [
    "show_hover_ruler",
    "show_target_snap",
    "show_measurement",
    "show_dragged_point",
    "snap_to_parts",
    "align_dragged_objects",
];

impl DraggerSettings {
    fn fields(&mut self) -> [&mut bool; 6] {
        [
            &mut self.show_hover_ruler,
            &mut self.show_target_snap,
            &mut self.show_measurement,
            &mut self.show_dragged_point,
            &mut self.snap_to_parts,
            &mut self.align_dragged_objects,
        ]
    }

    /// Reads the `"dragger"` object, each switch missing from it (or the whole
    /// object, in a file from before it existed) left at Studio's default.
    pub(super) fn read(value: &serde_json::Value) -> DraggerSettings {
        let mut settings = DraggerSettings::default();
        let stored = value.get("dragger");
        for (key, field) in KEYS.into_iter().zip(settings.fields()) {
            if let Some(on) = stored.and_then(|stored| stored.get(key)?.as_bool()) {
                *field = on;
            }
        }
        settings
    }

    pub(super) fn json(mut self) -> serde_json::Value {
        let pairs = KEYS
            .into_iter()
            .zip(self.fields())
            .map(|(key, field)| (key.to_string(), serde_json::Value::Bool(*field)));
        serde_json::Value::Object(pairs.collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_from_before_the_guides_existed_shows_them_all() {
        let value = serde_json::json!({ "orthographic": true });
        assert_eq!(DraggerSettings::read(&value), DraggerSettings::default());
    }

    #[test]
    fn each_switch_round_trips_on_its_own() {
        let settings = DraggerSettings {
            show_target_snap: false,
            snap_to_parts: false,
            ..DraggerSettings::default()
        };
        let value = serde_json::json!({ "dragger": settings.json() });
        assert_eq!(DraggerSettings::read(&value), settings);
    }

    #[test]
    fn a_malformed_switch_keeps_its_default() {
        let value = serde_json::json!({ "dragger": { "show_measurement": "no" } });
        assert!(DraggerSettings::read(&value).show_measurement);
    }
}
