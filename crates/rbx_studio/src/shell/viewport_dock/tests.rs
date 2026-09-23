use super::{layout_for, Columns};

#[test]
fn four_columns_need_1100_of_content_under_the_wide_padding() {
    // 1140 wide: 1140 − 40 = 1100.
    assert_eq!(layout_for(1140.), Columns::Four);
    assert_eq!(layout_for(1139.), Columns::Grid);
    assert_eq!(layout_for(1280.), Columns::Four);
}

#[test]
fn the_grid_needs_560_of_content_under_the_14px_padding() {
    // 588 wide: 588 − 28 = 560.
    assert_eq!(layout_for(588.), Columns::Grid);
    assert_eq!(layout_for(587.), Columns::One);
    assert_eq!(layout_for(640.), Columns::Grid);
    assert_eq!(layout_for(300.), Columns::One);
}
