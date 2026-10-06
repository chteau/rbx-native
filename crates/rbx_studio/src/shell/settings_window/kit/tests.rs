use gpui_kit::{App, ClickEvent, Window};

use super::{destructive_button, destructive_header, destructive_icon};
use crate::probe::assert_primary;

/// WCAG 2.5.5 on Settings' destructive buttons: Delete layout and Uninstall
/// theme (icon), Reset layout, and Reset all.
#[gpui_kit::test]
fn the_destructive_settings_buttons_lay_out_at_least_44_by_44(cx: &mut gpui_kit::TestAppContext) {
    assert_primary(cx, "delete-layout-x", 200., |_, _| {
        destructive_icon("delete-layout-x", "trash", "Delete")
    });
    assert_primary(cx, "uninstall-theme", 200., |_, _| {
        destructive_icon("uninstall-theme", "trash", "Uninstall")
    });
    assert_primary(cx, "reset-layout", 200., |_, _| {
        destructive_button("reset-layout", "rotate-ccw", "Reset layout")
    });
    assert_primary(cx, "reset-tools", 200., |_, _| {
        destructive_header(
            "reset-tools",
            "Reset all",
            Some(|_: &ClickEvent, _: &mut Window, _: &mut App| {}),
        )
    });
}
