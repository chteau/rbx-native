//! The frame sheet's Lighting and Image sections: a `ViewportFrame`'s
//! `Ambient`, `LightColor`, `LightDirection`, `ImageColor3` and
//! `ImageTransparency` as the design panel's own fields, so a typed or
//! dragged value takes the same path — and the same one undo step — as
//! every other field here, and the picture follows live.

use gpui_kit::assets::IconName;
use gpui_kit::component::color_picker::ColorPicker;
use gpui_kit::component::Sizable as _;
use gpui_kit::*;

use super::view::{caption, line, section, Label};
use super::{Key, Shell};
use crate::tokens;

impl Shell {
    pub(in crate::shell::ui_editor) fn frame_lighting(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut rows = Vec::new();
        for (key, label) in [(Key::Ambient, "Ambient"), (Key::LightColor, "Light")] {
            rows.push(self.color_row(key, label, window, cx));
        }
        // A line of its own over the three fields: beside the caption they
        // are too narrow for `-1` to fit.
        rows.push(
            div()
                .h(px(18.))
                .flex()
                .items_center()
                .child(caption("Direction"))
                .into_any_element(),
        );
        let axes = [(Key::LightX, "X"), (Key::LightY, "Y"), (Key::LightZ, "Z")]
            .map(|(key, axis)| self.field(key, Label::Text(axis), None, window, cx));
        rows.push(line(axes.into()));
        section("Lighting", Vec::new(), rows).into_any_element()
    }

    /// `ImageColor3` and `ImageTransparency`: what the rendered picture is
    /// tinted and faded by on its way into the GUI.
    pub(in crate::shell::ui_editor) fn frame_image(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tint = self.color_row(Key::ImageTint, "Tint", window, cx);
        let opacity = line(vec![
            caption("Opacity"),
            self.field(
                Key::ImageAlpha,
                Label::Icon(IconName::Droplet),
                Some("%"),
                window,
                cx,
            ),
        ]);
        section("Image", Vec::new(), vec![tint, opacity]).into_any_element()
    }

    /// A caption, a swatch that opens the palette, and the hex.
    fn color_row(
        &mut self,
        key: Key,
        label: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = self.key_color(key, window, cx);
        line(vec![
            caption(label),
            div()
                .flex_none()
                .child(ColorPicker::new(&state).with_size(tokens::field_size()))
                .into_any_element(),
            self.field(key, Label::Text("#"), None, window, cx),
        ])
    }
}
