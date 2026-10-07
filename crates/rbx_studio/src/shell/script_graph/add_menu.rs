//! The add menu: Shift+A, a double-click, or a wire dropped on nothing.
//! A search field over the catalog's sections; a dropped wire narrows it to
//! the nodes the wire can end on, and the node placed takes the wire.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use super::style;
use crate::script_editor::graph::catalog::{self, Category, Kind, PinType, Wanted};
use crate::script_editor::graph::layout::Side;
use crate::script_editor::graph::End;
use crate::tokens;

use super::super::Shell;

const WIDTH: f32 = 310.0;
/// How many rows show at once; the arrow keys walk the rest into view.
const ROWS: usize = 7;
const ROW_HEIGHT: f32 = 28.0;
const SECTION_HEIGHT: f32 = 26.0;

pub(in crate::shell) struct AddMenu {
    /// Where it opened, in panel pixels; the node lands here.
    panel: [f32; 2],
    /// The wire it was opened to finish.
    wire: Option<(End, Side, Wanted)>,
    pub(super) query: Entity<InputState>,
    selected: usize,
    _subscription: Subscription,
}

impl AddMenu {
    /// The wire this menu was opened to finish, and where it opened.
    pub(super) fn waiting(&self) -> Option<(&End, Side, [f32; 2])> {
        self.wire
            .as_ref()
            .map(|(end, side, _)| (end, *side, self.panel))
    }
}

impl Shell {
    pub(super) fn open_add_menu(
        &mut self,
        reference: Ref,
        panel: [f32; 2],
        wire: Option<(End, Side, Wanted)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search nodes"));
        let subscription = cx.subscribe_in(
            &query,
            window,
            move |shell, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    if let Some(menu) = shell.graph_menu(reference) {
                        menu.selected = 0;
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => shell.place_selected(reference, window, cx),
                _ => {}
            },
        );
        let field = query.clone();
        window.defer(cx, move |window, cx| {
            field.update(cx, |state, cx| state.focus(window, cx));
        });
        if let Some(editor) = self.graphs.get_mut(&reference) {
            editor.menu = Some(AddMenu {
                panel,
                wire,
                query,
                selected: 0,
                _subscription: subscription,
            });
        }
        cx.notify();
    }

    pub(super) fn close_add_menu(&mut self, reference: Ref, cx: &mut Context<Self>) {
        if let Some(editor) = self.graphs.get_mut(&reference) {
            if editor.menu.take().is_some() {
                cx.notify();
            }
        }
    }

    fn graph_menu(&mut self, reference: Ref) -> Option<&mut AddMenu> {
        self.graphs.get_mut(&reference)?.menu.as_mut()
    }

    fn menu_rows(&self, reference: Ref, cx: &App) -> Vec<&'static Kind> {
        let Some(menu) = self.graphs.get(&reference).and_then(|e| e.menu.as_ref()) else {
            return Vec::new();
        };
        let query = menu.query.read(cx).value().to_string();
        catalog::search(&query, menu.wire.as_ref().map(|(_, _, wanted)| *wanted))
    }

    fn place_selected(&mut self, reference: Ref, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.menu_rows(reference, cx);
        let Some(index) = self.graph_menu(reference).map(|menu| menu.selected) else {
            return;
        };
        if let Some(kind) = rows.get(index).copied() {
            self.place_node(reference, kind, window, cx);
        }
    }

    fn place_node(
        &mut self,
        reference: Ref,
        kind: &'static Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        let Some(menu) = editor.menu.take() else {
            return;
        };
        let at = editor.view.to_canvas(menu.panel);
        let id = editor.graph.add(kind, [at[0].round(), at[1].round()]);
        if let Some((end, side, wanted)) = menu.wire {
            if let Some(pin) = wanted.pin(kind) {
                let new = End::new(id, pin.name);
                let (from, to) = match side {
                    Side::Output => (end, new),
                    Side::Input => (new, end),
                };
                if let Err(refused) = editor.graph.connect(from, to) {
                    editor.notice = Some(refused.message());
                }
            }
        }
        editor.selection = [id].into();
        window.focus(&editor.focus, cx);
        self.commit_graph(reference, cx);
    }

    fn step_menu(&mut self, reference: Ref, by: isize, cx: &mut Context<Self>) {
        let len = self.menu_rows(reference, cx).len();
        if let Some(menu) = self.graph_menu(reference) {
            if len > 0 {
                menu.selected = (menu.selected as isize + by).rem_euclid(len as isize) as usize;
            }
            cx.notify();
        }
    }

    pub(super) fn add_menu_element(
        &mut self,
        reference: Ref,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let rows = self.menu_rows(reference, cx);
        let editor = self.graphs.get(&reference)?;
        let menu = editor.menu.as_ref()?;
        let panel = editor.panel_size();
        let selected = menu.selected.min(rows.len().saturating_sub(1));

        let mut list: Vec<AnyElement> = Vec::new();
        let mut section = None;
        let first = selected.saturating_sub(ROWS - 1);
        for (index, kind) in rows.iter().enumerate().skip(first).take(ROWS) {
            if section != Some(kind.category) {
                section = Some(kind.category);
                list.push(section_label(kind.category));
            }
            list.push(row(kind, index == selected, index, reference, cx));
        }
        let filter = menu
            .wire
            .as_ref()
            .map(|(_, _, wanted)| filter_line(*wanted));
        // Kept inside the panel: its height is what it lists, plus the
        // search field, the filter line and the footer.
        let sections = list.len() - list_rows(&rows, first);
        let height = 46.0
            + if filter.is_some() { 22.0 } else { 0.0 }
            + list_rows(&rows, first) as f32 * ROW_HEIGHT
            + sections as f32 * SECTION_HEIGHT
            // The footer, whose hint can wrap onto a second line.
            + 56.0;
        let left = menu.panel[0].min(panel[0] - WIDTH - 8.0).max(8.0);
        let top = menu.panel[1].min(panel[1] - height - 8.0).max(8.0);

        Some(
            div()
                .id("graph-add-menu")
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(WIDTH))
                .bg(tokens::tile())
                .border_1()
                .border_color(tokens::border())
                .rounded(tokens::radius())
                .shadow_lg()
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .capture_key_down(cx.listener(move |shell, event: &KeyDownEvent, window, cx| {
                    match event.keystroke.key.as_str() {
                        "up" => shell.step_menu(reference, -1, cx),
                        "down" => shell.step_menu(reference, 1, cx),
                        "escape" => {
                            if let Some(editor) = shell.graphs.get(&reference) {
                                window.focus(&editor.focus, cx);
                            }
                            shell.close_add_menu(reference, cx);
                        }
                        _ => return,
                    }
                    cx.stop_propagation();
                }))
                .child(
                    div().p(px(8.0)).child(
                        Input::new(&menu.query)
                            .small()
                            .prefix(Icon::new(IconName::Search).xsmall())
                            .suffix(
                                div()
                                    .text_size(tokens::text_xs())
                                    .text_color(tokens::text_muted())
                                    .child(format!("{} of {}", rows.len(), catalog::count())),
                            ),
                    ),
                )
                .children(filter)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .px(px(4.0))
                        .pb(px(4.0))
                        .children(list)
                        .when(rows.is_empty(), |this| {
                            this.child(
                                div()
                                    .px(px(8.0))
                                    .py(px(10.0))
                                    .text_size(tokens::text_sm())
                                    .text_color(tokens::text_muted())
                                    .child("No node matches"),
                            )
                        }),
                )
                .child(
                    div()
                        .border_t_1()
                        .border_color(tokens::border())
                        .px(px(12.0))
                        .py(px(8.0))
                        .text_size(tokens::text_xs())
                        .text_color(tokens::text_muted())
                        .child(match menu.wire {
                            Some(_) => {
                                "Enter to place · Esc to cancel · the wire connects on placing"
                            }
                            None => "Enter to place · Esc to cancel",
                        }),
                )
                .into_any_element(),
        )
    }
}

/// How many node rows show from `first`.
fn list_rows(rows: &[&'static Kind], first: usize) -> usize {
    rows.len().saturating_sub(first).min(ROWS)
}

fn filter_line(wanted: Wanted) -> AnyElement {
    let (verb, ty) = match wanted {
        Wanted::Input(ty) => ("take", ty),
        Wanted::Output(ty) => ("give", ty),
    };
    if ty == PinType::Exec {
        let text = match wanted {
            Wanted::Input(_) => "Nodes that run next",
            Wanted::Output(_) => "Nodes that run before",
        };
        return div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(12.0))
            .pb(px(6.0))
            .text_size(tokens::text_xs())
            .text_color(tokens::text_muted())
            .child(div().size(px(7.0)).rounded_full().bg(style::pin(ty)))
            .child(text)
            .into_any_element();
    }
    let article = match ty.name().starts_with(['a', 'e', 'i', 'o', 'u', 'I']) {
        true => "an",
        false => "a",
    };
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(12.0))
        .pb(px(6.0))
        .text_size(tokens::text_xs())
        .text_color(tokens::text_muted())
        .child(div().size(px(7.0)).rounded_full().bg(style::pin(ty)))
        .child(format!("Nodes that {verb} {article}"))
        .child(div().text_color(tokens::text()).child(ty.name()))
        .into_any_element()
}

fn section_label(category: Category) -> AnyElement {
    div()
        .h(px(SECTION_HEIGHT))
        .flex()
        .items_end()
        .px(px(8.0))
        .pb(px(4.0))
        .text_size(tokens::text_xxs())
        .font_weight(tokens::WEIGHT_SEMIBOLD)
        .text_color(tokens::text_muted())
        .child(category.label().to_uppercase())
        .into_any_element()
}

fn row(
    kind: &'static Kind,
    selected: bool,
    index: usize,
    reference: Ref,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let (input, output) = kind.signature();
    let name = |ty: Option<PinType>| ty.map_or("—", PinType::name);
    let signature = match (input, output) {
        (None, None) => String::new(),
        (input, output) => format!("{} → {}", name(input), name(output)),
    };
    let (_, icon) = style::header(kind.category);
    div()
        .id(("graph-add-row", index))
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(ROW_HEIGHT))
        .px(px(8.0))
        .rounded(tokens::radius_row())
        .cursor_pointer()
        .when(selected, |this| this.bg(tokens::field_select()))
        .hover(|this| this.bg(tokens::hover()))
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.place_node(reference, kind, window, cx);
        }))
        .child(
            Icon::new(style::icon(kind.category))
                .xsmall()
                .text_color(icon),
        )
        .child(
            div()
                .flex_1()
                .text_size(tokens::text_sm())
                .text_color(if selected {
                    tokens::text_strong()
                } else {
                    tokens::text()
                })
                .child(kind.title),
        )
        .child(
            div()
                .font_family(tokens::FONT_FAMILY_MONO)
                .text_size(tokens::text_xs())
                .text_color(tokens::text_muted())
                .child(signature),
        )
        .into_any_element()
}
