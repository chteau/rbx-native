//! The Script Editor's bar over the active script and its status line
//! under it, both shared by the Code and Graph sides: the Code | Graph
//! toggle and which script this is on the left, the side's own tools on
//! the right; where the cursor is or what the graph holds below.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Redo, Undo};
use gpui_kit::component::{h_flex, Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use lsp_types::DiagnosticSeverity;
use rbx_dom::{Ref, WeakDom};

use super::script_graph::Tool;
use super::Shell;
use crate::script_editor::source;
use crate::script_editor::tabs::View;
use crate::tokens;

impl Shell {
    pub(super) fn script_toolbar(
        &mut self,
        active: Ref,
        view: View,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.script_view_nav
            .begin(&self.tab_order, Some(View::ALL.len()), cx);
        let pills: Vec<AnyElement> = View::ALL
            .into_iter()
            .enumerate()
            .map(|(index, option)| {
                let tip = match option {
                    View::Code => "Edit this script as code",
                    View::Graph => "Edit this script as a node graph",
                };
                let pill = div()
                    .id(option.key())
                    .px(px(10.0))
                    .py(px(4.0))
                    .rounded(tokens::radius())
                    .text_size(tokens::text_sm())
                    .cursor_pointer()
                    .map(|this| match option == view {
                        true => this
                            .bg(tokens::field_select())
                            .text_color(tokens::text_strong())
                            .font_weight(tokens::WEIGHT_SEMIBOLD),
                        false => this
                            .text_color(tokens::text_muted())
                            .hover(|this| this.bg(tokens::hover())),
                    })
                    .tooltip(move |window, cx| super::tooltip::text(tip, window, cx))
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        shell.set_script_view(active, option, window, cx);
                    }))
                    .child(option.label());
                self.script_view_nav
                    .item(index, pill, cx)
                    .into_any_element()
            })
            .collect();

        let name = source::label(&self.dom, active).unwrap_or_default();
        let path = full_name(&self.dom, active);
        let tools = match view {
            View::Code => self.code_tools(active, cx),
            View::Graph => self.graph_tools(active, cx),
        };

        h_flex()
            .w_full()
            .h(px(38.0))
            .flex_none()
            .items_center()
            .gap(px(4.0))
            .px(px(8.0))
            .border_b_1()
            .border_color(tokens::border())
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                if shell.script_view_nav.key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .children(pills)
            .child(divider())
            .child(
                h_flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .py(px(3.0))
                    .rounded(tokens::radius())
                    .border_1()
                    .border_color(tokens::border())
                    .child(
                        Icon::new(IconName::Box)
                            .xsmall()
                            .text_color(tokens::text_muted()),
                    )
                    .child(
                        div()
                            .text_size(tokens::text_sm())
                            .text_color(tokens::text_strong())
                            .child(name),
                    )
                    .child(
                        div()
                            .font_family(tokens::FONT_FAMILY_MONO)
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text_muted())
                            .child(path),
                    ),
            )
            .child(div().flex_1())
            .child(tools)
            .into_any_element()
    }

    fn code_tools(&mut self, active: Ref, cx: &mut Context<Self>) -> AnyElement {
        let size = self.script_font_size();
        h_flex()
            .items_center()
            .gap(px(2.0))
            .child(tool(
                "code-undo",
                IconName::Undo2,
                "Undo",
                false,
                cx,
                move |shell, window, cx| {
                    shell.focus_script(active, window, cx);
                    window.dispatch_action(Box::new(Undo), cx);
                },
            ))
            .child(tool(
                "code-redo",
                IconName::Redo2,
                "Redo",
                false,
                cx,
                move |shell, window, cx| {
                    shell.focus_script(active, window, cx);
                    window.dispatch_action(Box::new(Redo), cx);
                },
            ))
            .child(divider())
            .child(
                div()
                    .id("script-font-size")
                    .px(px(6.0))
                    .py(px(3.0))
                    .rounded(tokens::radius())
                    .cursor_pointer()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text_muted())
                    .hover(|this| this.bg(tokens::hover()))
                    .tooltip(|window, cx| {
                        super::tooltip::text(
                            "Script font size — Studio Settings › Appearance",
                            window,
                            cx,
                        )
                    })
                    .on_click(cx.listener(|shell, _, _, cx| shell.open_settings(cx)))
                    .child(format!("{size:.0} px")),
            )
            .into_any_element()
    }

    fn graph_tools(&mut self, active: Ref, cx: &mut Context<Self>) -> AnyElement {
        let current = self.graph_tool(active).unwrap_or_default();
        let zoom = self.graph_status(active).map_or(1.0, |status| status.zoom);
        let pick = |id: &'static str,
                    icon: IconName,
                    label: &'static str,
                    which: Tool,
                    cx: &mut Context<Self>| {
            tool(
                id,
                icon,
                label,
                current == which,
                cx,
                move |shell, _, cx| {
                    shell.set_graph_tool(active, which, cx);
                },
            )
        };
        h_flex()
            .items_center()
            .gap(px(2.0))
            .child(pick(
                "graph-select",
                IconName::MousePointer2,
                "Select",
                Tool::Select,
                cx,
            ))
            .child(pick(
                "graph-hand",
                IconName::Hand,
                "Pan (or hold Space)",
                Tool::Hand,
                cx,
            ))
            .child(pick(
                "graph-marquee",
                IconName::SquareDashed,
                "Box select",
                Tool::Marquee,
                cx,
            ))
            .child(divider())
            .child(tool(
                "graph-undo",
                IconName::Undo2,
                "Undo",
                false,
                cx,
                |shell, _, cx| shell.undo(cx),
            ))
            .child(tool(
                "graph-redo",
                IconName::Redo2,
                "Redo",
                false,
                cx,
                |shell, _, cx| shell.redo(cx),
            ))
            .child(divider())
            .child(tool(
                "graph-optimize",
                IconName::Sparkle,
                "Optimize graph: tidy the layout",
                false,
                cx,
                move |shell, _, cx| {
                    shell.optimize_graph(active, cx);
                },
            ))
            .child(tool(
                "graph-fit",
                IconName::Maximize,
                "Fit the graph (F)",
                false,
                cx,
                move |shell, _, cx| {
                    shell.graph_fit(active, cx);
                },
            ))
            .child(tool(
                "graph-zoom-out",
                IconName::Minus,
                "Zoom out",
                false,
                cx,
                move |shell, _, cx| {
                    shell.graph_zoom_by(active, 1.0 / 1.25, cx);
                },
            ))
            .child(
                div()
                    .w(px(40.0))
                    .text_center()
                    .font_family(tokens::FONT_FAMILY_MONO)
                    .text_size(tokens::text_xs())
                    .text_color(tokens::text_muted())
                    .child(format!("{:.0}%", zoom * 100.0)),
            )
            .child(tool(
                "graph-zoom-in",
                IconName::Plus,
                "Zoom in",
                false,
                cx,
                move |shell, _, cx| {
                    shell.graph_zoom_by(active, 1.25, cx);
                },
            ))
            .into_any_element()
    }

    pub(super) fn script_status_bar(
        &self,
        active: Ref,
        view: View,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let (left, right) = match view {
            View::Code => (self.code_status(active, window, cx), "Luau".to_owned()),
            View::Graph => (
                self.graph_status(active)
                    .map(|status| status.summary)
                    .unwrap_or_default(),
                "Shift+A add a node · Space + drag pan · Del remove".to_owned(),
            ),
        };
        h_flex()
            .w_full()
            .h(px(24.0))
            .flex_none()
            .items_center()
            .justify_between()
            .px(px(12.0))
            .border_t_1()
            .border_color(tokens::border())
            .font_family(tokens::FONT_FAMILY_MONO)
            .text_size(tokens::text_xs())
            .text_color(tokens::text_muted())
            .child(left)
            .child(right)
            .into_any_element()
    }

    /// `Ln 7, Col 22 · Luau · no errors`, the problems being `luau-lsp`'s
    /// for this script alone.
    fn code_status(&self, active: Ref, _window: &Window, cx: &App) -> String {
        let Some(open) = self.scripts.open.get(&active) else {
            return String::new();
        };
        let at = open.state.read(cx).cursor_position();
        let problems = self
            .lsp
            .problems
            .iter()
            .find(|(script, _)| *script == active)
            .map(|(_, problems)| problems.as_slice())
            .unwrap_or_default();
        let count = |severity| {
            problems
                .iter()
                .filter(|problem| problem.severity == Some(severity))
                .count()
        };
        let (errors, warnings) = (
            count(DiagnosticSeverity::ERROR),
            count(DiagnosticSeverity::WARNING),
        );
        let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
        let problems = match (errors, warnings) {
            (0, 0) => "no errors".to_owned(),
            (e, 0) => plural(e, "error"),
            (0, w) => plural(w, "warning"),
            (e, w) => format!("{}, {}", plural(e, "error"), plural(w, "warning")),
        };
        format!(
            "Ln {}, Col {} · Luau · {problems}",
            at.line + 1,
            at.character + 1
        )
    }
}

/// `Workspace.Lava.Script`: the dotted path from the service down, as
/// `Instance:GetFullName` gives it. Services are the DOM's roots.
fn full_name(dom: &WeakDom, script: Ref) -> String {
    let mut names = Vec::new();
    let mut at = Some(script);
    while let Some(reference) = at {
        let Some(instance) = dom.get(reference) else {
            break;
        };
        names.push(instance.name().to_owned());
        at = dom.parent(reference);
    }
    names.reverse();
    names.join(".")
}

fn divider() -> AnyElement {
    div()
        .w(px(1.0))
        .h(px(16.0))
        .mx(px(6.0))
        .bg(tokens::border())
        .into_any_element()
}

fn tool(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    on: bool,
    cx: &mut Context<Shell>,
    act: impl Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .size(px(26.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::radius())
        .cursor_pointer()
        .role(Role::Button)
        .aria_label(label)
        .aria_selected(on)
        .when(on, |this| this.bg(tokens::accent_soft()))
        .hover(|this| this.bg(tokens::hover()))
        .tooltip(move |window, cx| super::tooltip::text(label, window, cx))
        .on_click(cx.listener(move |shell, _, window, cx| act(shell, window, cx)))
        .child(Icon::new(icon).small().text_color(match on {
            true => tokens::check_on(),
            false => tokens::text_muted(),
        }))
        .into_any_element()
}

#[cfg(test)]
#[path = "script_toolbar/tests.rs"]
mod tests;
