//! The layout switcher at the right end of the ribbon's tab row: the named
//! dock layouts Settings › Layout keeps, one click away the way Blender's
//! workspace tabs are, and the current arrangement saved as a new one from
//! the same place. Every change goes through the calls that page uses
//! (`save_named_layout`, `apply_named_layout`, `delete_named_layout`,
//! `reset_layout`), so both persist the same way.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, Icon, Sizable as _};
use gpui_kit::*;

use super::chrome::Trigger;
use super::{layout, menu, Shell};
use crate::tokens;

/// What the trigger reads: the saved layout in use, else Default for the
/// docks as they start, else Custom.
pub(super) fn switcher_label(active: Option<&str>, default: bool) -> SharedString {
    match active {
        Some(name) => SharedString::from(name.to_owned()),
        None if default => "Default".into(),
        None => "Custom".into(),
    }
}

impl Shell {
    /// Whether the docks are as a fresh install arranges them.
    fn layout_is_default(&self) -> bool {
        self.layout.saved() == layout::Layout::default().saved()
    }

    pub(super) fn layout_switcher(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = switcher_label(self.active_named_layout(), self.layout_is_default());
        let trigger = h_flex()
            .id("layout-switcher")
            .flex_none()
            .h(px(22.))
            .px(px(8.))
            .gap(px(6.))
            .items_center()
            .rounded(tokens::radius())
            .cursor_pointer()
            .text_size(tokens::text_sm())
            .line_height(tokens::line_sm())
            .text_color(tokens::text_tab_idle(tokens::text_placeholder()))
            .hover(|this| tokens::hover_fx(this).text_color(tokens::text_full()))
            .focus_visible(|this| this.shadow(tokens::focus_ring_inset()))
            .child(Icon::new(IconName::PanelsTopLeft).size(px(12.)))
            .child(label)
            .child(Icon::new(IconName::ChevronDown).size(px(10.)));
        let opened = cx.entity();
        let body = cx.entity();
        Popover::new("layout-switcher-popover")
            .appearance(false)
            .anchor(Anchor::TopRight)
            .open(self.layout_switcher_open)
            .on_open_change(move |open, _, cx| {
                let open = *open;
                opened.update(cx, |shell, cx| {
                    shell.layout_switcher_open = open;
                    cx.notify();
                });
            })
            .trigger(Trigger::new(trigger).tooltip("Dock layouts"))
            .content(move |_, _, cx| body.update(cx, |shell, cx| shell.layout_switcher_body(cx)))
    }

    /// Default, every saved layout (the one in use ticked, each with its
    /// delete), the name field that saves the docks as they are, and the
    /// way to the full Settings page.
    fn layout_switcher_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.active_named_layout().map(str::to_owned);
        let names: Vec<String> = self
            .named_layouts()
            .iter()
            .map(|named| named.name.clone())
            .collect();
        let row = |id: ElementId, icon, label: SharedString, checked: bool| {
            menu::row_chrome(id, Some(icon), label, true, checked)
        };
        let default = row(
            "layout-default".into(),
            IconName::RotateCcw,
            "Default".into(),
            active.is_none() && self.layout_is_default(),
        )
        .on_click(cx.listener(|shell, _, _, cx| {
            shell.layout_switcher_open = false;
            shell.reset_layout(cx);
        }));
        let saved = names.into_iter().map(|name| {
            let apply = name.clone();
            let delete = name.clone();
            let checked = active.as_deref() == Some(name.as_str());
            row(
                SharedString::from(format!("layout-apply-{name}")).into(),
                IconName::PanelsTopLeft,
                name.clone().into(),
                checked,
            )
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.layout_switcher_open = false;
                shell.apply_named_layout(&apply, cx);
            }))
            .child(
                div()
                    .id(SharedString::from(format!("layout-delete-{name}")))
                    .flex_none()
                    .p(px(3.))
                    .rounded(tokens::radius())
                    .text_color(tokens::text3())
                    .hover(|this| this.bg(tokens::hover()).text_color(tokens::text_full()))
                    .tooltip(move |window, cx| {
                        super::tooltip::text(format!("Delete \u{201c}{delete}\u{201d}"), window, cx)
                    })
                    .on_click({
                        let name = name.clone();
                        cx.listener(move |shell, _, _, cx| {
                            cx.stop_propagation();
                            shell.delete_named_layout(&name, cx);
                        })
                    })
                    .child(Icon::new(IconName::Trash).size(px(11.))),
            )
        });
        let save = h_flex()
            .gap(px(6.))
            .px(px(4.))
            .py(px(4.))
            .items_center()
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .h(px(24.))
                    .px(px(8.))
                    .items_center()
                    .border_1()
                    .border_color(tokens::border2())
                    .rounded(tokens::radius())
                    .bg(tokens::field_select())
                    .text_size(tokens::text_sm())
                    .child(Input::new(&self.layout_name).appearance(false)),
            )
            .child(
                Button::new("layout-save")
                    .label("Save")
                    .primary()
                    .xsmall()
                    .tooltip("Save the docks as they are under this name")
                    .on_click(cx.listener(|shell, _, window, cx| {
                        shell.save_layout_from_switcher(window, cx)
                    })),
            );
        let manage = row(
            "layout-manage".into(),
            IconName::Settings,
            "Manage in Settings\u{2026}".into(),
            false,
        )
        .on_click(cx.listener(|shell, _, _, cx| {
            shell.layout_switcher_open = false;
            shell.open_settings_on("layout", cx);
        }));
        menu::surface()
            .min_w(px(240.))
            .child(heading("Layouts"))
            .child(default)
            .children(saved)
            .child(rule())
            .child(heading("Save current as"))
            .child(save)
            .child(rule())
            .child(manage)
            .into_any_element()
    }

    /// Saves the docks under the typed name (replacing a layout saved under
    /// it) and empties the field, from Enter or the Save button.
    pub(super) fn save_layout_from_switcher(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self.layout_name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        self.save_named_layout(&name, cx);
        self.layout_name
            .update(cx, |state, cx| state.set_value("", window, cx));
    }
}

fn heading(text: &'static str) -> impl IntoElement {
    div()
        .px(px(8.))
        .pt(px(4.))
        .pb(px(2.))
        .text_size(tokens::text_xs())
        .line_height(tokens::line_sm())
        .text_color(tokens::text3())
        .child(text)
}

fn rule() -> impl IntoElement {
    div().mx(px(4.)).my(px(3.)).h(px(1.)).bg(tokens::border())
}

#[cfg(test)]
mod tests {
    use super::switcher_label;

    #[test]
    fn the_trigger_names_the_layout_in_use() {
        assert_eq!(switcher_label(Some("Scripting"), false), "Scripting");
        assert_eq!(switcher_label(Some("Scripting"), true), "Scripting");
        assert_eq!(switcher_label(None, true), "Default");
        assert_eq!(switcher_label(None, false), "Custom");
    }
}
