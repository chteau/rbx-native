//! The Fill section's gradient rows: its rotation and colours, the shape
//! of the ramp (`UIGradient.Type`), what fills past its ends
//! (`TileMode`) and how far it reaches (`Scale`).

use gpui_kit::assets::IconName;
use gpui_kit::*;

use super::super::super::chrome;
use super::actions::enum_item;
use super::view::{caption, line, toggle, Label};
use super::{Key, Shell};

pub(in crate::shell::ui_editor) const GRADIENT: &str = "UIGradient";

/// A choice the inspector offers for one of the gradient's enums: the
/// property, the enum it holds, and each item's name, icon and tooltip.
pub(super) struct Choice {
    pub(super) property: &'static str,
    pub(super) enumeration: &'static str,
    pub(super) items: &'static [(&'static str, IconName, &'static str)],
}

/// `Elliptical`, the dump's fourth `GradientType`, is left out: the GUI
/// renderer has no ellipse yet and would draw it as a linear ramp, so the
/// canvas would show something other than what was picked. A place that
/// already holds one shows no shape lit.
pub(super) const SHAPE: Choice = Choice {
    property: "Type",
    enumeration: "GradientType",
    items: &[
        ("Linear", IconName::MoveRight, "Linear"),
        ("Radial", IconName::CircleDot, "Radial"),
        ("Conical", IconName::ChartPie, "Conical"),
    ],
};

pub(super) const TILE: Choice = Choice {
    property: "TileMode",
    enumeration: "GradientTileMode",
    items: &[
        (
            "Clamp",
            IconName::ArrowRightToLine,
            "Clamp: the end colours fill past the ramp",
        ),
        ("Repeat", IconName::Repeat, "Repeat the ramp"),
        (
            "Mirror",
            IconName::FlipHorizontal2,
            "Mirror the ramp at each end",
        ),
    ],
};

impl Shell {
    /// The rows a gradient adds under the fill.
    pub(super) fn gradient_rows(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let rotation = self.field(
            Key::GradientRotation,
            Label::Icon(IconName::RotateCw),
            Some("°"),
            window,
            cx,
        );
        let scale = self.field(
            Key::GradientScale,
            Label::Icon(IconName::Scaling),
            None,
            window,
            cx,
        );
        let edit = chrome::icon_button(
            "ui-inspect-gradient-edit",
            IconName::SlidersHorizontal,
            "Edit its colours",
        )
        .on_click(cx.listener(|shell, _, _, cx| shell.select_modifier(GRADIENT, cx)))
        .into_any_element();
        let mut shape = vec![caption("Shape")];
        shape.extend(self.choice_buttons(&SHAPE, cx));
        let mut tile = vec![caption("Tile")];
        tile.extend(self.choice_buttons(&TILE, cx));
        tile.push(scale);
        vec![
            line(vec![caption("Gradient"), rotation, edit]),
            line(shape),
            line(tile),
        ]
    }

    fn choice_buttons(&self, choice: &'static Choice, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let current = self.gradient_item(choice);
        choice
            .items
            .iter()
            .enumerate()
            .map(|(index, &(item, icon, label))| {
                toggle(
                    (
                        SharedString::from(format!("ui-inspect-gradient-{}", choice.property)),
                        index,
                    ),
                    icon,
                    label,
                    current.as_deref() == Some(item),
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.set_gradient_item(choice, item, cx)
                }))
                .into_any_element()
            })
            .collect()
    }

    /// The first inspected element's gradient's item for `choice`, its
    /// class default where it stores none.
    fn gradient_item(&self, choice: &Choice) -> Option<String> {
        let gradient = self.anchor_child(GRADIENT)?;
        enum_item(
            &self.dom,
            &self.database,
            gradient,
            choice.property,
            choice.enumeration,
        )
        .map(str::to_owned)
    }

    /// Sets `item` on every inspected element's gradient, as one step.
    fn set_gradient_item(&mut self, choice: &Choice, item: &str, cx: &mut Context<Self>) {
        let writes = self
            .inspected()
            .into_iter()
            .filter_map(|element| self.child_of(element, GRADIENT))
            .map(|gradient| (gradient, choice.property, item.to_owned()))
            .collect();
        self.write_all(writes, cx);
    }
}

#[cfg(test)]
mod tests;
