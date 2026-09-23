// The width → layout rule, checked at each threshold and one pixel below
// it. Kept out of the parent file: its glob import of the toolkit brings
// a `test` macro along that shadows the attribute.

use super::layout_for;

#[test]
fn nine_hundred_is_wide_and_one_pixel_less_stacks() {
    assert!(layout_for(900.).wide);
    assert!(!layout_for(899.).wide);
    assert_eq!(layout_for(1280.), layout_for(900.));
}

#[test]
fn the_grid_drops_to_one_column_under_560_of_content() {
    // 560 + 28 of padding.
    assert_eq!(layout_for(588.).columns, 2);
    assert_eq!(layout_for(587.).columns, 1);
    assert_eq!(layout_for(640.).columns, 2);
}

#[test]
fn restore_defaults_goes_icon_only_and_descriptions_clamp_under_420_of_content() {
    let roomy = layout_for(448.);
    assert!(!roomy.compact_restore && !roomy.clamp_descriptions);
    let compact = layout_for(447.);
    assert!(compact.compact_restore && compact.clamp_descriptions);
}

#[test]
fn the_action_row_wraps_under_360_of_content() {
    assert!(!layout_for(388.).wrap_actions);
    assert!(layout_for(387.).wrap_actions);
    let narrow = layout_for(300.);
    assert!(narrow.wrap_actions && narrow.compact_restore && narrow.columns == 1);
}
