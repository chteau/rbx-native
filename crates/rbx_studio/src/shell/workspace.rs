//! **Row D** — the document, and whichever panels are parked around it.
//!
//! Which panel is on which edge is [`super::layout::Layout`]'s to say, not
//! this module's: Row D used to be three hardcoded `.child()` calls, so
//! "Properties is on the left" was source order and nothing could change
//! it at runtime. This walks the layout instead. Everything else about the
//! row is unchanged, including the panel bodies themselves.
//!
//! Every panel is still an unconditional child, which is what
//! `UX_GUIDELINES.md` §3's independence rule asks: nothing here is
//! conditional on which document Row A has open, and panels sharing an
//! edge split it rather than becoming tabs, so none of them is ever left
//! unrendered and a scroll position has nowhere to get lost.
//!
//! The frame fixes both side docks at 228px. They are draggable here
//! anyway — a place file's instance names are not 228px wide just because
//! the design's were — but they start there, and the handle that widens
//! them draws nothing until it is pointed at.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::*;

use crate::class_icons::IconPack;
use crate::tokens;

use super::chrome::{self, Document, Drag};
use super::layout::{Edge, Home, Landing, Panel};
use super::menu::{self, MenuId};
use super::Shell;

/// The most of the window's width one side dock may occupy.
const MAX_DOCK_SHARE: f32 = 0.28;

impl Shell {
    pub(super) fn workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + 'static {
        // At 2x the UI scale a 300px dock becomes 600, and two of them
        // leave a 1600px window ~350px of viewport. The scale is meant to
        // make the editor readable, not to squeeze out the thing being
        // edited, so a dock may never take more than this share of the
        // window however big its own tokens have grown. A *display* cap:
        // writing it back would mean a window narrowed once and widened
        // again had lost the size the user picked.
        let limit = f32::from(window.viewport_size().width) * MAX_DOCK_SHARE;

        // Built before the row is assembled, because it needs `&mut self`
        // and the row below only moves finished elements around.
        let document = self.document_content(window, cx);

        // Before the edges are built, so a panel torn out on the frame it
        // was dropped does not also draw itself into a dock for one frame.
        self.sync_panel_windows(cx);
        self.raise_panel_windows(window, cx);
        self.sync_stats(cx);

        let left = self.dock_edge(Edge::Left, limit, window, cx);
        let right = self.dock_edge(Edge::Right, limit, window, cx);
        let bottom = self.dock_edge(Edge::Bottom, limit, window, cx);

        // Bottom is a sibling of the left/document/right row, not a child of
        // its document column: Output reads as the floor the whole
        // workspace stands on, not a third thing squeezed between two
        // docks. Left and right still run the window's full height, above
        // it.
        v_flex()
            .relative()
            .w_full()
            .flex_1()
            .overflow_hidden()
            .bg(tokens::black())
            .child(
                h_flex()
                    // Positioned, so the drop strips can be laid over it —
                    // an edge holding nothing has no column of its own to
                    // aim at.
                    .relative()
                    .w_full()
                    .flex_1()
                    .overflow_hidden()
                    .children(left)
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            // Nothing floats over the document: the view's
                            // own settings and numbers are the Viewport
                            // dock's (see `shell::viewport_dock`).
                            .child(
                                v_flex()
                                    .relative()
                                    .flex_1()
                                    .overflow_hidden()
                                    .child(document),
                            ),
                    )
                    .children(right),
            )
            .children(bottom)
    }

    /// The same parts a docked panel renders, for one that has been torn
    /// out into a window of its own (see `shell::panel_window`).
    ///
    /// The torn-out window renders *through* this rather than holding a
    /// copy of anything, which is what keeps a floating Explorer the
    /// Explorer instead of a second one to keep in step.
    pub(super) fn floating_parts(
        &mut self,
        panel: Panel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        self.panel_parts(panel, false, window, cx)
    }

    /// One panel's trailing controls and its content, with the tab strip
    /// left to `shell::docks` — the strip belongs to the *dock* now, since
    /// several panels share one.
    pub(super) fn panel_parts(
        &mut self,
        panel: Panel,
        collapsed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        match panel {
            Panel::Properties => self.properties_dock(window, cx),
            Panel::Explorer => self.explorer_dock(cx),
            Panel::Output => self.output_dock(collapsed, cx),
            Panel::Viewport => self.viewport_dock(window, cx),
            Panel::Argon => self.argon_dock(cx),
            Panel::Wally => self.wally_dock(cx),
            Panel::ScriptAnalysis => self.script_analysis_dock(cx),
            Panel::Watch => self.watch_dock(window, cx),
            Panel::CallStack => self.call_stack_dock(cx),
        }
    }

    /// Whichever editor Row A has open. This is the *only* thing Row A
    /// switches — see this module's own doc comment.
    fn document_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Every frame, whichever document is up: the render thread draws the
        // UI editor's canvas only while it is asked for, and stops the frame
        // it is not.
        let canvas = self.canvas_request();
        self.viewport
            .update(cx, |viewport, _| viewport.set_canvas(canvas));
        match self.document {
            Document::Viewport => self.viewport().into_any_element(),
            Document::Scripts => self.script_editor(window, cx).into_any_element(),
            Document::UiEditor => self.ui_editor(window, cx),
        }
    }

    fn explorer_dock(&self, cx: &mut Context<Self>) -> (Option<AnyElement>, Option<AnyElement>) {
        let show_all = self.show_all_services();
        let light_icons = self.icon_pack() == IconPack::Light;
        let mut items = self.move_items(Panel::Explorer);
        items.extend([
            menu::item("Show all services")
                .checked(show_all)
                .on_click(move |shell, cx| shell.set_show_all_services(!show_all, cx)),
            menu::item("Light Icons")
                .checked(light_icons)
                .on_click(move |shell, cx| {
                    let next = if light_icons {
                        IconPack::Dark
                    } else {
                        IconPack::Light
                    };
                    shell.set_icon_pack(next, cx);
                }),
        ]);
        // Only once somebody has installed a pack: a menu offering "Built-in
        // icons" as the sole choice would be a checked row that does nothing.
        let (packs, current) = self.installed_icon_packs();
        if !packs.is_empty() {
            items.push(
                menu::item("Built-in icons")
                    .checked(current.is_none())
                    .on_click(|shell, cx| shell.set_user_icon_pack(None, cx)),
            );
            items.extend(packs.iter().map(|name| {
                let chosen = name.clone();
                menu::item(format!("Icon pack: {name}"))
                    .checked(current == Some(name.as_str()))
                    .on_click(move |shell, cx| shell.set_user_icon_pack(Some(chosen.clone()), cx))
            }));
        }
        let overflow = menu::dropdown(
            self,
            MenuId::ExplorerOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "explorer-overflow",
                IconName::Ellipsis,
                "Explorer settings",
            )),
            items,
            cx,
        );

        let content = chrome::dock_content(
            v_flex()
                .size_full()
                .gap(px(10.))
                .child(search_field(self.tab_order.next(), &self.search, cx))
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .child(self.instance_tree(cx)),
                ),
        );
        (
            Some(overflow.into_any_element()),
            Some(content.into_any_element()),
        )
    }

    fn properties_dock(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let items = self.move_items(Panel::Properties);
        let rows = self.properties(window, cx);
        // This dock had no overflow menu at all until it needed somewhere
        // to put "Move to"; it still has nothing else in it.
        let overflow = menu::dropdown(
            self,
            MenuId::PropertiesOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "properties-overflow",
                IconName::Ellipsis,
                "Properties settings",
            )),
            items,
            cx,
        );

        (
            Some(overflow.into_any_element()),
            Some(chrome::dock_content(rows).into_any_element()),
        )
    }

    /// The "Move to Left / Right / Bottom" run every dock's overflow menu
    /// carries, with the edge it is already on ticked.
    ///
    /// This is the keyboard-operable way to rearrange, and the reason it is
    /// built before the drag rather than after: the accessibility guidance
    /// this project follows treats drag-only rearrangement as a failure,
    /// not a gap. The drag calls the same `Shell::move_panel`, so there is
    /// one transform rather than two that can disagree.
    pub(super) fn move_items(&self, panel: Panel) -> Vec<menu::Item> {
        let here = self.layout.home_of(panel);
        Edge::ALL
            .into_iter()
            .map(|edge| {
                let landing = Landing::NewGroup { edge, group: 0 };
                menu::item(format!("Move to {}", edge.label()))
                    .checked(matches!(here, Home::Docked { edge: at, .. } if at == edge))
                    .on_click(move |shell, cx| shell.land_panel(panel, landing, cx))
            })
            .chain([
                menu::item("Float")
                    .checked(here == Home::Floating)
                    .on_click(move |shell, cx| shell.float_panel(panel, cx)),
                menu::item("Close").on_click(move |shell, cx| shell.close_panel(panel, cx)),
            ])
            .collect()
    }

    /// Output keeps its filter/Clear strip and its own settings menu, and
    /// collapses to exactly its own tab strip — enough to find and re-open,
    /// and nothing else.
    fn output_dock(
        &self,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let timestamps = self.output_show_timestamps;
        let mut items = self.move_items(Panel::Output);
        items.push(
            menu::item("Show Timestamp")
                .checked(timestamps)
                .on_click(move |shell, cx| {
                    shell.output_show_timestamps = !timestamps;
                    cx.notify();
                }),
        );
        let overflow = menu::dropdown(
            self,
            MenuId::OutputOverflow,
            chrome::Trigger::new(chrome::icon_button(
                "output-overflow",
                IconName::Ellipsis,
                "Output settings",
            )),
            items,
            cx,
        );

        let controls = h_flex()
            .flex_1()
            .items_center()
            .gap(px(4.))
            .child(self.output_controls(cx))
            .child(overflow)
            .child(
                chrome::icon_button(
                    "output-collapse",
                    if collapsed {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    },
                    if collapsed {
                        "Expand Output"
                    } else {
                        "Collapse Output"
                    },
                )
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.output_collapsed = !shell.output_collapsed;
                    shell.save_settings();
                    cx.notify();
                })),
            );

        let content = (!collapsed).then(|| {
            chrome::dock_content(
                div()
                    .size_full()
                    .overflow_hidden()
                    .child(self.output_panel(cx)),
            )
            .into_any_element()
        });
        (Some(controls.into_any_element()), content)
    }

    pub(super) fn handle(&self, edge: Edge, cx: &mut Context<Self>) -> AnyElement {
        let size = self.layout.size(edge);

        chrome::resize_handle(
            edge,
            cx.listener(move |shell, event: &MouseDownEvent, _, cx| {
                let origin = if edge.is_vertical() {
                    event.position.x
                } else {
                    event.position.y
                };
                shell.drag = Some(Drag {
                    edge,
                    origin,
                    size: px(size),
                });
                cx.notify();
            }),
        )
        .into_any_element()
    }

    /// Applies an in-progress drag. Sizes follow the pointer exactly — the
    /// delta is measured from where the drag started, not from the previous
    /// frame, so a drag that leaves the window and comes back doesn't
    /// accumulate error.
    pub(super) fn drag_resize(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = self.drag else {
            return;
        };

        let size = f32::from(drag.size);
        let moved = match drag.edge {
            // The left edge is before its handle, so a rightward drag grows
            // it; the other two sit after theirs and shrink.
            Edge::Left => f32::from(position.x - drag.origin),
            Edge::Right => -f32::from(position.x - drag.origin),
            Edge::Bottom => -f32::from(position.y - drag.origin),
        };
        self.layout.resize(drag.edge, size + moved);
        cx.notify();
    }

    /// Saved on *release*, not on every frame of the drag: a resize is one
    /// decision, and writing the settings file sixty times a second to
    /// record its intermediate states would be absurd.
    pub(super) fn end_resize(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            self.save_settings();
            cx.notify();
        }
    }
}

/// A dock's search field: `field_select`, a hairline `border`, radius
/// `RADIUS` — the reference's own field look.
///
/// The toolkit `Input` keeps the caret, selection and IME handling; its own
/// chrome is switched off so this container can be the frame's.
pub(super) fn search_field(
    tab_index: isize,
    state: &Entity<gpui_kit::component::input::InputState>,
    cx: &App,
) -> impl IntoElement {
    search_field_sized(tab_index, state, tokens::input_height(), cx)
}

/// The Output strip's search box: the same field at the shorter height the
/// reference gives it (5px of vertical padding around 11px text), which is
/// also what lets it sit centred in a tab strip its dock-sized twin
/// overflows.
pub(super) fn search_field_compact(
    tab_index: isize,
    state: &Entity<gpui_kit::component::input::InputState>,
    cx: &App,
) -> impl IntoElement {
    search_field_sized(tab_index, state, tokens::strip_field_height(), cx)
}

fn search_field_sized(
    tab_index: isize,
    state: &Entity<gpui_kit::component::input::InputState>,
    height: Pixels,
    cx: &App,
) -> impl IntoElement {
    // Tracks the input's own handle, so the box wears the reference's
    // focused border whenever the field inside it has focus.
    let handle = state.read(cx).focus_handle(cx);
    h_flex()
        .track_focus(&handle)
        .focus(|this| this.border_color(tokens::accent_line()))
        .w_full()
        .h(height)
        .flex_none()
        .items_center()
        .justify_center()
        .px(px(8.))
        .rounded(tokens::radius())
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border())
        .text_size(tokens::text_sm())
        .line_height(tokens::line_sm())
        .text_color(tokens::text_strong())
        .child(
            Input::new(state)
                .appearance(false)
                .with_size(tokens::field_size())
                .h(tokens::input_height())
                // Without this the field sits at the toolkit's default
                // index 0 and sorts ahead of every region — a search box
                // reached before the menu bar.
                .tab_index(tab_index),
        )
}
