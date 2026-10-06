use gpui_kit::component::h_flex;
use gpui_kit::{px, ParentElement as _, Styled as _};

use super::{test_tiles, tokens};
use crate::probe::assert_primary;

/// WCAG 2.5.5 on the tiles for the controls a person reaches for in a hurry,
/// measured in the ribbon's own tile area (its height less the padding) with
/// Large Click Targets off.
#[gpui_kit::test]
fn the_play_and_stop_tiles_lay_out_at_least_44_by_44(cx: &mut gpui_kit::TestAppContext) {
    let area = f32::from(tokens::ribbon_height()) - 20.;
    for id in ["ribbon-play", "ribbon-stop"] {
        assert_primary(cx, id, area, move |_, _| {
            h_flex().h(px(area)).items_stretch().children(test_tiles())
        });
    }
}
