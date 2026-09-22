//! The inspector's sections, top to bottom as Figma orders them: where the
//! element is, how it is sized and lays its children out, how it looks,
//! its fill, its stroke, and the constraints on it.

use gpui_kit::assets::IconName;
use gpui_kit::*;

use super::super::super::chrome;
use super::super::super::menu::{self, MenuId};
use super::super::super::rows::checkbox;
use super::super::toolbar::ALIGNS;
use super::actions::{Flow, ASPECT};
use super::view::{caption, grid, line, section, toggle, words, Label};
use super::{Key, Shell, CORNERS};

/// The auto layout flows, as Figma draws its buttons.
const FLOWS: [(Flow, IconName, &str); 4] = [
    (Flow::Free, IconName::SquareDashed, "No auto layout"),
    (
        Flow::Column,
        IconName::Rows3,
        "Vertical list (UIListLayout)",
    ),
    (
        Flow::Row,
        IconName::Columns3,
        "Horizontal list (UIListLayout)",
    ),
    (Flow::Grid, IconName::LayoutGrid, "Grid (UIGridLayout)"),
];

/// The constraints the `+` offers: the class, what the menu calls it, and
/// the captioned rows of fields it shows once it is on.
type Rows = &'static [(&'static str, &'static [(Key, &'static str)])];
const CONSTRAINTS: [(&str, &str, Rows); 4] = [
    (
        ASPECT,
        "Aspect ratio",
        &[("Aspect", &[(Key::Aspect, "W/H")])],
    ),
    (
        "UISizeConstraint",
        "Size limits",
        &[
            ("Min size", &[(Key::MinW, "W"), (Key::MinH, "H")]),
            ("Max size", &[(Key::MaxW, "W"), (Key::MaxH, "H")]),
        ],
    ),
    (
        TEXT_SIZE,
        "Text size limits",
        &[("Text size", &[(Key::MinText, "↓"), (Key::MaxText, "↑")])],
    ),
    ("UIScale", "Scale", &[("Scale", &[(Key::Scale, "×")])]),
];
const TEXT_SIZE: &str = "UITextSizeConstraint";

impl Shell {
    pub(in crate::shell::ui_editor) fn inspector_sections(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        vec![
            self.position_section(window, cx),
            self.layout_section(window, cx),
            self.appearance_section(window, cx),
            self.fill_section(window, cx),
            self.stroke_section(window, cx),
            self.constraints_section(window, cx),
        ]
    }

    fn position_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let aligns = ALIGNS
            .map(|(axis, mode, icon, label)| {
                chrome::icon_button(("ui-inspect-align", axis * 3 + mode as usize), icon, label)
                    .on_click(cx.listener(move |shell, _, _, cx| shell.align_gui(axis, mode, cx)))
                    .into_any_element()
            })
            .into();
        let x = self.field(Key::X, Label::Text("X"), None, window, cx);
        let y = self.field(Key::Y, Label::Text("Y"), None, window, cx);
        let rotation = self.field(
            Key::Rotation,
            Label::Icon(IconName::RotateCw),
            Some("°"),
            window,
            cx,
        );
        let anchor = self
            .anchor()
            .map(|anchor| anchor.map(|at| (at * 2.0).round() as usize));
        let handle = cx.entity();
        let anchors = grid("ui-inspect-anchor", anchor, move |cell, _, cx| {
            let at = cell.map(|index| index as f32 * 0.5);
            handle.update(cx, |shell, cx| shell.set_anchor(at, cx));
        });
        let turn = chrome::icon_button("ui-inspect-turn", IconName::RotateCwSquare, "Rotate 90°")
            .on_click(cx.listener(|shell, _, _, cx| shell.quarter_turn(cx)))
            .into_any_element();
        section(
            "Position",
            Vec::new(),
            vec![
                line(aligns),
                line(vec![x, y]),
                line(vec![caption("Anchor"), anchors, rotation, turn]),
            ],
        )
        .into_any_element()
    }

    fn layout_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let flow = self.flow();
        let flows = FLOWS
            .map(|(each, icon, label)| {
                toggle(
                    ("ui-inspect-flow", each as usize),
                    icon,
                    label,
                    flow == each,
                )
                .on_click(cx.listener(move |shell, _, _, cx| shell.set_flow(each, cx)))
                .into_any_element()
            })
            .into();
        let w = self.field(Key::W, Label::Text("W"), None, window, cx);
        let h = self.field(Key::H, Label::Text("H"), None, window, cx);
        let locked = self.anchor_child(ASPECT).is_some();
        let lock = toggle(
            "ui-inspect-aspect",
            IconName::Link2,
            "Keep the shape (UIAspectRatioConstraint)",
            locked,
        )
        .on_click(cx.listener(|shell, _, _, cx| shell.toggle_aspect(cx)))
        .into_any_element();
        let padding = self.field(Key::Padding, Label::Icon(IconName::Frame), None, window, cx);
        let mut rows = vec![line(flows), line(vec![w, h, lock])];
        if flow == Flow::Free {
            rows.push(line(vec![caption("Padding"), padding]));
        } else {
            let gap = self.field(
                Key::Gap,
                Label::Icon(IconName::BetweenVerticalStart),
                None,
                window,
                cx,
            );
            let handle = cx.entity();
            let align = grid(
                "ui-inspect-layout-align",
                self.layout_align(),
                move |cell, _, cx| {
                    handle.update(cx, |shell, cx| shell.set_layout_align(cell, cx));
                },
            );
            rows.push(line(vec![caption("Gap"), gap, padding]));
            rows.push(line(vec![caption("Align"), align]));
        }
        let handle = cx.entity();
        let clip = checkbox(
            "ui-inspect-clip",
            self.flag("ClipsDescendants"),
            move |_, _, cx| {
                handle.update(cx, |shell, cx| shell.toggle_flag("ClipsDescendants", cx));
            },
        )
        .into_any_element();
        rows.push(line(vec![clip, words("Clip content")]));
        section("Layout", Vec::new(), rows).into_any_element()
    }

    fn appearance_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let visible = self.flag("Visible") != Some(false);
        let eye = toggle(
            "ui-inspect-visible",
            if visible {
                IconName::Eye
            } else {
                IconName::EyeOff
            },
            "Show or hide (Visible)",
            false,
        )
        .on_click(cx.listener(|shell, _, _, cx| shell.toggle_flag("Visible", cx)))
        .into_any_element();
        let opacity = self.field(
            Key::Opacity,
            Label::Icon(IconName::Droplet),
            Some("%"),
            window,
            cx,
        );
        let radius = self.field(
            Key::Radius,
            Label::Icon(IconName::SquareRoundCorner),
            None,
            window,
            cx,
        );
        let open = self.ui.inspector.corners;
        let split = toggle(
            "ui-inspect-corners",
            IconName::Scan,
            "Each corner on its own",
            open,
        )
        .on_click(cx.listener(|shell, _, _, cx| {
            shell.ui.inspector.corners = !shell.ui.inspector.corners;
            cx.notify();
        }))
        .into_any_element();
        let mut rows = vec![line(vec![opacity, radius, split])];
        if open {
            let corners = CORNERS
                .into_iter()
                .zip(["↖", "↗", "↘", "↙"])
                .map(|(key, arrow)| self.field(key, Label::Text(arrow), None, window, cx))
                .collect();
            rows.push(line(corners));
        }
        section("Appearance", vec![eye], rows).into_any_element()
    }

    fn fill_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let filled = self.reading(Key::FillAlpha) != Some(Some(vec![0.0]));
        let gradient = self.anchor_child("UIGradient").is_some();
        let mut actions = vec![toggle(
            "ui-inspect-gradient",
            IconName::Blend,
            "Gradient (UIGradient)",
            gradient,
        )
        .on_click(cx.listener(move |shell, _, _, cx| match gradient {
            true => shell.remove_modifier("UIGradient", cx),
            false => shell.add_modifier("UIGradient", cx),
        }))
        .into_any_element()];
        if !filled {
            actions.push(add_button(
                "ui-inspect-fill-add",
                "Add a fill",
                cx,
                |shell, cx| shell.set_fill(true, cx),
            ));
        }
        let mut rows = Vec::new();
        if filled {
            let mut paint = self.paint(Key::Fill, Key::FillAlpha, window, cx);
            paint.push(remove_button(
                "ui-inspect-fill-remove",
                "Remove the fill",
                cx,
                |shell, cx| shell.set_fill(false, cx),
            ));
            rows.push(line(paint));
        }
        if gradient {
            let rotation = self.field(
                Key::GradientRotation,
                Label::Icon(IconName::RotateCw),
                Some("°"),
                window,
                cx,
            );
            rows.push(line(vec![
                caption("Gradient"),
                rotation,
                edit_button(
                    "ui-inspect-gradient-edit",
                    "Edit its colours",
                    "UIGradient",
                    cx,
                ),
            ]));
        }
        section("Fill", actions, rows).into_any_element()
    }

    fn stroke_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(_) = self.anchor_child("UIStroke") else {
            let add = add_button(
                "ui-inspect-stroke-add",
                "Add a stroke (UIStroke)",
                cx,
                |shell, cx| shell.add_modifier("UIStroke", cx),
            );
            return section("Stroke", vec![add], Vec::new()).into_any_element();
        };
        let mut paint = self.paint(Key::Stroke, Key::StrokeAlpha, window, cx);
        paint.push(remove_button(
            "ui-inspect-stroke-remove",
            "Remove the stroke",
            cx,
            |shell, cx| shell.remove_modifier("UIStroke", cx),
        ));
        let width = self.field(
            Key::StrokeWidth,
            Label::Icon(IconName::Minus),
            None,
            window,
            cx,
        );
        let more = edit_button(
            "ui-inspect-stroke-edit",
            "More stroke options",
            "UIStroke",
            cx,
        );
        section(
            "Stroke",
            Vec::new(),
            vec![line(paint), line(vec![caption("Weight"), width, more])],
        )
        .into_any_element()
    }

    fn constraints_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let text = self.inspected().first().is_some_and(|&r| self.is_text(r));
        let items = CONSTRAINTS
            .iter()
            .filter(|(class, ..)| *class != TEXT_SIZE || text)
            .map(|&(class, label, _)| {
                let item = menu::item(label);
                match (self.anchor_child(class), class) {
                    (Some(_), _) => item.disabled(),
                    // At the shape it has, as the lock makes it: a ratio
                    // of 1 would square whatever it went on.
                    (None, ASPECT) => item.on_click(|shell, cx| shell.toggle_aspect(cx)),
                    (None, _) => item.on_click(move |shell, cx| shell.add_modifier(class, cx)),
                }
            })
            .collect();
        let add = menu::dropdown_at(
            self,
            MenuId::UiConstraint,
            chrome::Trigger::new(chrome::icon_button(
                "ui-inspect-constraint-add",
                IconName::Plus,
                "Add a constraint",
            )),
            items,
            Anchor::TopRight,
            cx,
        )
        .into_any_element();
        let mut rows = Vec::new();
        for (class, _, lines) in CONSTRAINTS {
            if self.anchor_child(class).is_none() {
                continue;
            }
            for (index, (title, fields)) in lines.iter().enumerate() {
                let mut children = vec![caption(title)];
                for &(key, text) in *fields {
                    children.push(self.field(key, Label::Text(text), None, window, cx));
                }
                // The buttons for the whole constraint ride its first row.
                if index == 0 {
                    children.push(edit_button(
                        SharedString::from(format!("ui-inspect-edit-{class}")),
                        "Every option",
                        class,
                        cx,
                    ));
                    children.push(remove_button(
                        SharedString::from(format!("ui-inspect-remove-{class}")),
                        "Remove it",
                        cx,
                        move |shell, cx| shell.remove_modifier(class, cx),
                    ));
                }
                rows.push(line(children));
            }
        }
        section("Constraints", vec![add], rows).into_any_element()
    }
}

fn add_button(
    id: impl Into<ElementId>,
    label: &'static str,
    cx: &mut Context<Shell>,
    action: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> AnyElement {
    chrome::icon_button(id, IconName::Plus, label)
        .on_click(cx.listener(move |shell, _, _, cx| action(shell, cx)))
        .into_any_element()
}

fn remove_button(
    id: impl Into<ElementId>,
    label: &'static str,
    cx: &mut Context<Shell>,
    action: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> AnyElement {
    chrome::icon_button(id, IconName::Minus, label)
        .on_click(cx.listener(move |shell, _, _, cx| action(shell, cx)))
        .into_any_element()
}

/// Selects the modifier, for every row the inspector does not break out.
fn edit_button(
    id: impl Into<ElementId>,
    label: &'static str,
    class: &'static str,
    cx: &mut Context<Shell>,
) -> AnyElement {
    chrome::icon_button(id, IconName::SlidersHorizontal, label)
        .on_click(cx.listener(move |shell, _, _, cx| shell.select_modifier(class, cx)))
        .into_any_element()
}
