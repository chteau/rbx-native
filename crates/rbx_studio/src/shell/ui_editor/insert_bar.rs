//! The floating bar along the bottom of the canvas: one click inserts a UI
//! element where the canvas is looking — under the selected element, or
//! at the top of the screen on the canvas — instead of a trip to the
//! Explorer's `+`. It is that same insert (`Shell::insert_instance_under`),
//! seeded the same way and undone the same way; the bar only saves the
//! trip.

use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use super::super::chrome;
use super::super::menu::{self, MenuId};
use super::{is_gui_object, Shell};
use crate::explorer;
use crate::tokens;

/// The elements a screen is built from, one button each.
const ELEMENTS: [(&str, IconName); 7] = [
    ("Frame", IconName::Square),
    ("TextLabel", IconName::Type),
    ("TextButton", IconName::MousePointerClick),
    ("TextBox", IconName::TextCursorInput),
    ("ImageLabel", IconName::Image),
    ("ImageButton", IconName::ImagePlus),
    ("ScrollingFrame", IconName::ScrollText),
];

/// What shapes and arranges them, behind the bar's `+`.
const COMPONENTS: [&str; 11] = [
    "UIListLayout",
    "UIGridLayout",
    "UIPadding",
    "UICorner",
    "UIStroke",
    "UIGradient",
    "UIAspectRatioConstraint",
    "UIScale",
    "UISizeConstraint",
    "UITextSizeConstraint",
    "UIFlexItem",
];

const STARTER_GUI: &str = "StarterGui";
const SCREEN_CLASS: &str = "ScreenGui";

impl Shell {
    /// Where the bar puts what it inserts: the selected element when it is
    /// on the canvas's screen (a `GuiObject`, or the screen itself), the
    /// screen otherwise, nowhere with no screen up.
    fn insert_target(&self) -> Option<Ref> {
        let screen = self.canvas_request()?.screen;
        let inside = self
            .selected()
            .filter(|&r| super::root_of(&self.dom, &self.database, r) == Some(screen))
            .filter(|&r| r == screen || is_gui_object(&self.dom, &self.database, r));
        Some(inside.unwrap_or(screen))
    }

    fn insert_on_canvas(&mut self, class: &str, cx: &mut Context<Self>) {
        let parent = match class {
            // A screen goes where Studio's own do, and becomes the canvas's
            // as soon as the insert selects it.
            SCREEN_CLASS => explorer::resolve(&self.dom, STARTER_GUI),
            _ => self.insert_target(),
        };
        if let Some(parent) = parent {
            self.insert_instance_under(Some(parent), class, cx);
        }
    }

    pub(super) fn insert_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let target = self.insert_target().is_some();
        let buttons = ELEMENTS.map(|(class, icon)| {
            chrome::icon_button(
                SharedString::from(format!("ui-insert-{class}")),
                icon,
                class,
            )
            .when(!target, |this| this.opacity(0.4).cursor_not_allowed())
            .on_click(cx.listener(move |shell, _, _, cx| shell.insert_on_canvas(class, cx)))
            .into_any_element()
        });
        let items = std::iter::once(
            menu::item("ScreenGui")
                .icon(IconName::AppWindow)
                .on_click(|shell, cx| shell.insert_on_canvas(SCREEN_CLASS, cx)),
        )
        .chain(COMPONENTS.map(|class| {
            let item = menu::item(class);
            match target {
                true => item.on_click(move |shell, cx| shell.insert_on_canvas(class, cx)),
                false => item.disabled(),
            }
        }))
        .collect();
        let more = menu::dropdown(
            self,
            MenuId::UiInsert,
            chrome::Trigger::new(chrome::icon_button(
                "ui-insert-more",
                IconName::Plus,
                "Insert a screen, layout or modifier",
            )),
            items,
            cx,
        );

        div()
            .absolute()
            .bottom(px(16.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                h_flex()
                    .id("ui-insert-bar")
                    // Its own clicks stop here: a press on the bar is not a
                    // press on the canvas under it.
                    .occlude()
                    .items_center()
                    .gap(px(2.))
                    .p(px(4.))
                    .rounded(tokens::RADIUS)
                    .bg(tokens::chrome())
                    .shadow(tokens::elevation())
                    .children(buttons)
                    .child(div().w(px(1.)).h(px(14.)).mx(px(4.)).bg(tokens::divider()))
                    .child(more),
            )
            .into_any_element()
    }
}
