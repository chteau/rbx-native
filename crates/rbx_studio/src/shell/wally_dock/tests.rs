use super::{columns_for, layout_for};

#[test]
fn the_rail_sits_beside_the_page_from_720() {
    assert!(layout_for(720.).wide);
    assert!(!layout_for(719.).wide);
}

#[test]
fn columns_follow_the_content_width() {
    // 1280 wide: 1280 − 208 − 1 − 40 = 1031 of content → 4 columns.
    assert_eq!(layout_for(1280.).columns, 4);
    // 640 floated: 612 of content → 2; 300: 272 → 1.
    assert_eq!(layout_for(640.).columns, 2);
    assert_eq!(layout_for(300.).columns, 1);
    assert_eq!(columns_for(448.), 2);
    assert_eq!(columns_for(447.), 1);
    assert_eq!(columns_for(904.), 4);
    assert_eq!(columns_for(903.), 3);
    assert_eq!(columns_for(2000.), 4);
    assert_eq!(columns_for(10.), 1);
}

#[test]
fn a_result_stacks_its_controls_under_520_and_an_update_its_versions_under_420() {
    // Content 520 and 420 exactly: still side by side.
    assert!(!layout_for(548.).stack_result);
    assert!(layout_for(547.).stack_result);
    assert!(!layout_for(448.).stack_update);
    assert!(layout_for(447.).stack_update);
    assert!(!layout_for(1280.).stack_result);
}
