//! The thumbnail of a user template's tile in New.

use gpui_kit::component::h_flex;
use gpui_kit::*;

use crate::launcher::ui;

/// The saved viewport shot, or an icon box when the template has none.
pub(super) fn user_template_thumb(template: &crate::place_templates::PlaceTemplate) -> AnyElement {
    match &template.thumbnail {
        Some(path) => div()
            .size(px(52.))
            .flex_none()
            .rounded(px(6.))
            .overflow_hidden()
            .bg(ui::bg())
            .child(img(path.clone()).size_full().object_fit(ObjectFit::Cover))
            .into_any_element(),
        None => h_flex()
            .size(px(52.))
            .flex_none()
            .rounded(px(6.))
            .bg(ui::bg())
            .items_center()
            .justify_center()
            .text_color(ui::accent())
            .child(ui::icon("layout-template", 20.))
            .into_any_element(),
    }
}
