//! The two seams of the Animation Editor's bottom panel a drag can move:
//! between the 3D view and the dope sheet (or Curve Editor) under it, and
//! between the channel names and the keys. Pure clamps, so they can be
//! tested without a window.

/// The bottom panel's default share of the room it splits with the 3D view,
/// and its default bounds: room for an R15 rig's fifteen joints when there
/// is room at all. A height the user dragged replaces these.
pub(super) const SHEET_SHARE: f32 = 0.46;
pub(super) const SHEET_DEFAULT_RANGE: (f32, f32) = (170.0, 380.0);

/// The shortest the bottom panel may be dragged: the ruler, a summary row,
/// two joint rows and the scrollbar.
pub(super) const SHEET_MIN: f32 = 110.0;
/// The shortest the 3D view above it may be left.
pub(super) const VIEW_MIN: f32 = 120.0;

/// The channel-name column's width by default, and its bounds.
pub(super) const NAMES_DEFAULT: f32 = 196.0;
pub(super) const NAMES_RANGE: (f32, f32) = (120.0, 480.0);
/// The least room a drag may leave the keys beside the names.
pub(super) const KEYS_MIN: f32 = 160.0;

/// The bottom panel's height for `want` when it and the 3D view share
/// `room` pixels: no shorter than [`SHEET_MIN`], and leaving the view at
/// least [`VIEW_MIN`]. When there is not room for both, the panel keeps
/// its minimum and the view gets what is left.
pub(super) fn sheet_height(want: f32, room: f32) -> f32 {
    let most = (room - VIEW_MIN).max(SHEET_MIN);
    want.clamp(SHEET_MIN, most)
}

/// The height the panel takes before anyone drags its seam.
pub(super) fn default_sheet_height(room: f32) -> f32 {
    sheet_height(
        (room * SHEET_SHARE).clamp(SHEET_DEFAULT_RANGE.0, SHEET_DEFAULT_RANGE.1),
        room,
    )
}

/// The names column's width for `want` in a panel `total` pixels wide:
/// inside [`NAMES_RANGE`], and leaving the keys at least [`KEYS_MIN`].
pub(super) fn names_width(want: f32, total: f32) -> f32 {
    let most = (total - KEYS_MIN).min(NAMES_RANGE.1).max(NAMES_RANGE.0);
    want.clamp(NAMES_RANGE.0, most)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_keeps_both_sides_open() {
        assert_eq!(sheet_height(50.0, 700.0), SHEET_MIN);
        assert_eq!(sheet_height(300.0, 700.0), 300.0);
        assert_eq!(sheet_height(690.0, 700.0), 700.0 - VIEW_MIN);
        // Too little room for both: the sheet keeps its minimum.
        assert_eq!(sheet_height(200.0, 150.0), SHEET_MIN);
    }

    #[test]
    fn the_default_is_the_old_share() {
        assert_eq!(default_sheet_height(600.0), 600.0 * SHEET_SHARE);
        assert_eq!(default_sheet_height(2000.0), SHEET_DEFAULT_RANGE.1);
        assert_eq!(default_sheet_height(300.0), SHEET_DEFAULT_RANGE.0);
        assert_eq!(default_sheet_height(250.0), 250.0 - VIEW_MIN);
    }

    #[test]
    fn the_names_column_leaves_room_for_keys() {
        assert_eq!(names_width(60.0, 1200.0), NAMES_RANGE.0);
        assert_eq!(names_width(260.0, 1200.0), 260.0);
        assert_eq!(names_width(900.0, 1200.0), NAMES_RANGE.1);
        assert_eq!(names_width(400.0, 500.0), 500.0 - KEYS_MIN);
        assert_eq!(names_width(400.0, 200.0), NAMES_RANGE.0);
    }
}
