//! Plugin widgets: the Plugin Widgets dock panel (one tab per enabled
//! `DockWidgetPluginGui`) and floating widgets drawn above the editor.
//! Each is rendered by `render` and hit-tested on its boxes: pointer and
//! key input become `Input::Gui`, a `TextBox` is edited through an input
//! field placed over it.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::input::{Input as TextField, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_lua::plugin::{GuiEvent, GuiEventKind, Input, LayoutBox, UiId, WidgetKind};

use super::render;
use crate::shell::{Panel, Shell};
use crate::tokens;

/// Where a widget's surface was last laid out, written during prepaint so
/// pointer positions can be made widget-local and the render sized.
pub(super) type Area = Rc<Cell<Option<Bounds<Pixels>>>>;

struct Editing {
    widget: UiId,
    referent: Ref,
    field: Entity<InputState>,
    _events: Subscription,
}

#[derive(Clone, Copy)]
pub(super) enum Pointer {
    Move,
    Down(u8),
    Up(u8),
    Scroll(f32),
}

#[derive(Default)]
pub(crate) struct Widgets {
    /// Last render per widget.
    pub(crate) drawn: HashMap<UiId, render::Drawn>,
    /// The layout last fed back to the plugins, per widget.
    layouts: HashMap<UiId, Vec<LayoutBox>>,
    /// The dock tab shown.
    pub(crate) current: Option<UiId>,
    pub(crate) renderer: Option<render::Renderer>,
    pub(super) areas: HashMap<UiId, Area>,
    /// The last revision and size asked of the renderer.
    requested: HashMap<UiId, (u64, (u32, u32))>,
    /// Each widget's `TextBox`es with their text as last rendered.
    text_boxes: HashMap<UiId, HashMap<Ref, String>>,
    /// Dock widgets enabled at the last `ui_changed`.
    enabled: HashSet<UiId>,
    hover: Option<(UiId, Ref)>,
    pressed: Option<(UiId, Ref, u8)>,
    editing: Option<Editing>,
    /// Where cursor-placed floating widgets opened.
    anchors: HashMap<UiId, Point<Pixels>>,
    poll: Option<Task<()>>,
    /// `Renderer::landed` as last acted on.
    landed_seen: u64,
    /// When the last render came back (see `collect_widget_renders`).
    last_render: Option<std::time::Instant>,
}

fn text_boxes(dom: &WeakDom) -> HashMap<Ref, String> {
    let mut found = HashMap::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        if instance.class() == "TextBox" {
            let text = match instance.properties().get("Text") {
                Some(Variant::String(text)) => text.clone(),
                _ => String::new(),
            };
            found.insert(referent, text);
        }
        stack.extend_from_slice(instance.children());
    }
    found
}

pub(super) fn pixel_size(bounds: Bounds<Pixels>) -> (u32, u32) {
    (
        f32::from(bounds.size.width).round().max(0.0) as u32,
        f32::from(bounds.size.height).round().max(0.0) as u32,
    )
}

fn backdrop() -> [f32; 3] {
    let colour = tokens::dock();
    [colour.r, colour.g, colour.b]
}

impl Shell {
    /// The Plugin Widgets panel's content (for `workspace::panel_parts`).
    pub(in crate::shell) fn plugin_widgets_dock(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<AnyElement>) {
        let docked: Vec<(UiId, String, u64)> = self
            .plugins
            .ui
            .widgets
            .iter()
            .filter(|w| w.enabled && matches!(w.kind, WidgetKind::Dock { .. }))
            .map(|w| (w.id, w.title.clone(), w.revision))
            .collect();
        let Some(first) = docked.first().map(|w| w.0) else {
            let empty = div()
                .p(px(12.))
                .text_color(tokens::text2())
                .child("No plugin widget is open.");
            return (None, Some(empty.into_any_element()));
        };
        let current = self
            .plugins
            .widgets
            .current
            .filter(|c| docked.iter().any(|w| w.0 == *c))
            .unwrap_or(first);
        self.plugins.widgets.current = Some(current);
        let revision = docked.iter().find(|w| w.0 == current).map_or(0, |w| w.2);
        let measured = self
            .plugins
            .widgets
            .areas
            .get(&current)
            .and_then(|area| area.get());
        if let Some(bounds) = measured {
            self.request_widget_render(current, revision, pixel_size(bounds), cx);
        }

        let tabs = h_flex()
            .w_full()
            .border_b_1()
            .border_color(tokens::border())
            .children(docked.iter().map(|(id, title, _)| {
                let id = *id;
                let close = div()
                    .id(ElementId::Name(format!("plugin-widget-close-{id}").into()))
                    .px(px(4.))
                    .text_color(tokens::text2())
                    .hover(|s| s.text_color(tokens::text()))
                    .child("×")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |shell, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            shell.plugin_input(Input::WidgetClosed(id), cx);
                        }),
                    );
                let tab = h_flex()
                    .id(ElementId::Name(format!("plugin-widget-tab-{id}").into()))
                    .gap(px(4.))
                    .px(px(8.))
                    .py(px(4.))
                    .text_color(if id == current {
                        tokens::text()
                    } else {
                        tokens::text2()
                    })
                    .hover(|s| s.bg(tokens::hover_subtle()))
                    .child(title.clone())
                    .child(close)
                    .on_click(cx.listener(move |shell, _: &ClickEvent, _, cx| {
                        shell.plugins.widgets.current = Some(id);
                        cx.notify();
                    }));
                if id == current {
                    tab.border_b_2().border_color(tokens::tab_active_bar())
                } else {
                    tab
                }
            }));
        let surface = self.widget_surface(current, cx);
        let content = v_flex()
            .size_full()
            .bg(tokens::dock())
            .child(tabs)
            .child(div().flex_1().min_h_0().w_full().child(surface));
        (None, Some(content.into_any_element()))
    }

    /// Floating widgets, for `plugin_overlays`.
    pub(super) fn floating_widget_overlays(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        #[allow(clippy::type_complexity)]
        let floating: Vec<(UiId, u64, Option<(f32, f32)>, (f32, f32), bool)> = self
            .plugins
            .ui
            .widgets
            .iter()
            .filter(|w| w.enabled)
            .filter_map(|w| match w.kind {
                WidgetKind::Floating {
                    position,
                    size,
                    light_dismiss,
                } => Some((w.id, w.revision, position, size, light_dismiss)),
                WidgetKind::Dock { .. } | WidgetKind::Overlay { .. } => None,
            })
            .collect();
        self.plugins
            .widgets
            .anchors
            .retain(|id, _| floating.iter().any(|w| w.0 == *id));
        let mut overlays = Vec::new();
        for (id, revision, position, (width, height), light_dismiss) in floating {
            let at = match position {
                Some((x, y)) => point(px(x), px(y)),
                None => *self
                    .plugins
                    .widgets
                    .anchors
                    .entry(id)
                    .or_insert_with(|| window.mouse_position()),
            };
            let size = (
                width.round().max(1.0) as u32,
                height.round().max(1.0) as u32,
            );
            self.request_widget_render(id, revision, size, cx);
            let surface = self.widget_surface(id, cx);
            let mut card = div()
                .w(px(width))
                .h(px(height))
                .bg(tokens::dock())
                .border_1()
                .border_color(tokens::border())
                .shadow_lg()
                .child(surface);
            if light_dismiss {
                card =
                    card.on_mouse_down_out(cx.listener(move |shell, _: &MouseDownEvent, _, cx| {
                        shell.plugin_input(Input::WidgetClosed(id), cx);
                    }));
            }
            overlays.push(
                deferred(
                    anchored()
                        .position(at)
                        .snap_to_window_with_margin(px(8.))
                        .child(card),
                )
                .into_any_element(),
            );
        }
        overlays
    }

    /// After `ui_changed`: opens the panel when a dock widget became
    /// enabled, re-requests renders whose widget revision changed.
    pub(super) fn plugin_widgets_changed(&mut self, cx: &mut Context<Self>) {
        let widgets: Vec<(UiId, u64, bool)> = self
            .plugins
            .ui
            .widgets
            .iter()
            .map(|w| {
                (
                    w.id,
                    w.revision,
                    w.enabled && matches!(w.kind, WidgetKind::Dock { .. }),
                )
            })
            .collect();
        let state = &mut self.plugins.widgets;
        let alive: HashSet<UiId> = widgets.iter().map(|w| w.0).collect();
        state.drawn.retain(|id, _| alive.contains(id));
        state.requested.retain(|id, _| alive.contains(id));
        state.areas.retain(|id, _| alive.contains(id));
        state.text_boxes.retain(|id, _| alive.contains(id));
        state.layouts.retain(|id, _| alive.contains(id));
        if state.hover.is_some_and(|(id, _)| !alive.contains(&id)) {
            state.hover = None;
        }
        if state.pressed.is_some_and(|(id, _, _)| !alive.contains(&id)) {
            state.pressed = None;
        }
        if state
            .editing
            .as_ref()
            .is_some_and(|e| !alive.contains(&e.widget))
        {
            state.editing = None;
        }

        let enabled: HashSet<UiId> = widgets.iter().filter(|w| w.2).map(|w| w.0).collect();
        let opened = widgets
            .iter()
            .find(|w| w.2 && !state.enabled.contains(&w.0))
            .map(|w| w.0);
        state.enabled = enabled;
        let stale: Vec<(UiId, u64, (u32, u32))> = widgets
            .iter()
            .filter_map(|&(id, revision, _)| {
                let &(asked, size) = state.requested.get(&id)?;
                (asked != revision).then_some((id, revision, size))
            })
            .collect();
        if let Some(id) = opened {
            state.current = Some(id);
            self.set_panel_open(Panel::PluginWidgets, true, cx);
        }
        for (id, revision, size) in stale {
            self.request_widget_render(id, revision, size, cx);
        }
        cx.notify();
    }

    pub(super) fn request_widget_render(
        &mut self,
        id: UiId,
        revision: u64,
        size: (u32, u32),
        cx: &mut Context<Self>,
    ) {
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        if self.plugins.widgets.requested.get(&id) == Some(&(revision, size)) {
            return;
        }
        let Some(dom) = self
            .plugins
            .host
            .as_ref()
            .and_then(|host| host.widget_dom(id))
        else {
            return;
        };
        let transparent = self
            .plugins
            .ui
            .widgets
            .iter()
            .any(|w| w.id == id && matches!(w.kind, WidgetKind::Overlay { .. }));
        let state = &mut self.plugins.widgets;
        state.text_boxes.insert(id, text_boxes(&dom));
        state
            .renderer
            .get_or_insert_with(render::Renderer::new)
            .request(id, revision, dom, size, backdrop(), transparent);
        state.requested.insert(id, (revision, size));
        self.arm_widget_poll(cx);
    }

    fn arm_widget_poll(&mut self, cx: &mut Context<Self>) {
        if self.plugins.widgets.poll.is_some() {
            return;
        }
        self.plugins.widgets.poll = Some(cx.spawn(async move |shell, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            match shell.update(cx, |shell, cx| shell.collect_widget_renders(cx)) {
                Ok(true) => {}
                _ => break,
            }
        }));
    }

    /// Takes finished renders; `false` (and the poll stops) once nothing
    /// asked for is still owed.
    fn collect_widget_renders(&mut self, cx: &mut Context<Self>) -> bool {
        let state = &mut self.plugins.widgets;
        // Images that were still downloading have arrived: draw everything
        // again (forgetting what was asked makes the next frame re-ask).
        let landed = state.renderer.as_ref().map_or(0, |r| r.landed());
        if landed != state.landed_seen {
            state.landed_seen = landed;
            state.requested.clear();
            cx.notify();
        }
        let finished = state
            .renderer
            .as_ref()
            .map(|r| r.finished())
            .unwrap_or_default();
        let mut layouts = Vec::new();
        for (id, drawn) in finished.iter() {
            let Some(origin) = drawn.boxes.first().map(|b| b.rect) else {
                continue;
            };
            let boxes: Vec<LayoutBox> = drawn
                .boxes
                .iter()
                .map(|b| LayoutBox {
                    referent: b.referent,
                    position: [b.rect[0] - origin[0], b.rect[1] - origin[1]],
                    size: [b.rect[2], b.rect[3]],
                })
                .collect();
            if state.layouts.get(id) != Some(&boxes) {
                state.layouts.insert(*id, boxes.clone());
                layouts.push(boxes);
            }
        }
        if !finished.is_empty() {
            state.last_render = Some(std::time::Instant::now());
            state.drawn.extend(finished);
            cx.notify();
        }
        for boxes in layouts {
            self.plugin_input(Input::Layout(boxes), cx);
        }
        let state = &mut self.plugins.widgets;
        let pending = state.requested.iter().any(|(id, &(revision, size))| {
            state
                .drawn
                .get(id)
                .is_none_or(|drawn| drawn.revision != revision || drawn.size != size)
        });
        // Keep listening for late images a little while after any render.
        let settling = state
            .last_render
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(10));
        let pending = pending || settling;
        if !pending {
            state.poll = None;
        }
        pending
    }

    fn widget_surface(&mut self, id: UiId, cx: &mut Context<Self>) -> AnyElement {
        let area = self.plugins.widgets.areas.entry(id).or_default().clone();
        let measure = canvas(
            move |bounds, window, _| {
                if area.get() != Some(bounds) {
                    area.set(Some(bounds));
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();
        let drawn = self.plugins.widgets.drawn.get(&id);
        let image = drawn.map(|drawn| {
            img(drawn.image.clone())
                .absolute()
                .left(px(0.))
                .top(px(0.))
                .w(px(drawn.size.0 as f32))
                .h(px(drawn.size.1 as f32))
        });
        let editor = self
            .plugins
            .widgets
            .editing
            .as_ref()
            .filter(|editing| editing.widget == id)
            .and_then(|editing| {
                let rect = drawn?
                    .boxes
                    .iter()
                    .find(|b| b.referent == editing.referent)?
                    .rect;
                Some(
                    div()
                        .absolute()
                        .left(px(rect[0]))
                        .top(px(rect[1]))
                        .w(px(rect[2]))
                        .h(px(rect[3]))
                        .child(TextField::new(&editing.field)),
                )
            });
        div()
            .id(ElementId::Name(format!("plugin-widget-{id}").into()))
            .relative()
            .size_full()
            .overflow_hidden()
            .child(measure)
            .children(image)
            .children(editor)
            .on_mouse_move(cx.listener(move |shell, e: &MouseMoveEvent, window, cx| {
                shell.widget_pointer(id, e.position, Pointer::Move, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |shell, e: &MouseDownEvent, window, cx| {
                    shell.widget_pointer(id, e.position, Pointer::Down(1), window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |shell, e: &MouseDownEvent, window, cx| {
                    shell.widget_pointer(id, e.position, Pointer::Down(2), window, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |shell, e: &MouseUpEvent, window, cx| {
                    shell.widget_pointer(id, e.position, Pointer::Up(1), window, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(move |shell, e: &MouseUpEvent, window, cx| {
                    shell.widget_pointer(id, e.position, Pointer::Up(2), window, cx);
                }),
            )
            .on_scroll_wheel(cx.listener(move |shell, e: &ScrollWheelEvent, window, cx| {
                let delta = e.delta.pixel_delta(px(20.));
                let dy = -f32::from(delta.y);
                shell.widget_pointer(id, e.position, Pointer::Scroll(dy), window, cx);
            }))
            .on_hover(cx.listener(move |shell, hovered: &bool, _, cx| {
                if !*hovered {
                    shell.widget_left(id, cx);
                }
            }))
            .into_any_element()
    }

    fn send_gui(&mut self, referent: Ref, kind: GuiEventKind, cx: &mut Context<Self>) {
        self.plugin_input(Input::Gui(GuiEvent { referent, kind }), cx);
    }

    pub(super) fn widget_left(&mut self, id: UiId, cx: &mut Context<Self>) {
        if let Some((_, old)) = self.plugins.widgets.hover.filter(|(w, _)| *w == id) {
            self.plugins.widgets.hover = None;
            self.send_gui(old, GuiEventKind::Leave { x: -1.0, y: -1.0 }, cx);
        }
    }

    pub(super) fn widget_pointer(
        &mut self,
        id: UiId,
        position: Point<Pixels>,
        pointer: Pointer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(area) = self.plugins.widgets.areas.get(&id).and_then(|a| a.get()) else {
            return;
        };
        let x = f32::from(position.x - area.origin.x);
        let y = f32::from(position.y - area.origin.y);
        let target = self
            .plugins
            .widgets
            .drawn
            .get(&id)
            .and_then(|drawn| super::hit::hit(&drawn.boxes, &drawn.sinks, x, y));
        match pointer {
            Pointer::Move => {
                let now = target.map(|r| (id, r));
                if self.plugins.widgets.hover != now {
                    if let Some((_, old)) = self.plugins.widgets.hover.take() {
                        self.send_gui(old, GuiEventKind::Leave { x, y }, cx);
                    }
                    if let Some(r) = target {
                        self.send_gui(r, GuiEventKind::Enter { x, y }, cx);
                    }
                    self.plugins.widgets.hover = now;
                }
                if let Some(r) = target {
                    self.send_gui(r, GuiEventKind::Move { x, y }, cx);
                }
            }
            Pointer::Down(button) => {
                self.plugins.widgets.pressed = target.map(|r| (id, r, button));
                if let Some(r) = target {
                    self.send_gui(r, GuiEventKind::Down { button, x, y }, cx);
                }
            }
            Pointer::Up(button) => {
                let pressed = self.plugins.widgets.pressed.take();
                let Some(r) = target else {
                    return;
                };
                self.send_gui(r, GuiEventKind::Up { button, x, y }, cx);
                if pressed == Some((id, r, button)) {
                    self.send_gui(r, GuiEventKind::Click { button, x, y }, cx);
                    let is_text_box = self
                        .plugins
                        .widgets
                        .text_boxes
                        .get(&id)
                        .is_some_and(|boxes| boxes.contains_key(&r));
                    if button == 1 && is_text_box {
                        self.begin_widget_edit(id, r, window, cx);
                    }
                }
            }
            Pointer::Scroll(dy) => {
                if let Some(r) = target {
                    self.send_gui(r, GuiEventKind::Scroll { dy }, cx);
                }
            }
        }
    }

    fn begin_widget_edit(
        &mut self,
        id: UiId,
        referent: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editing) = &self.plugins.widgets.editing {
            if editing.referent == referent {
                return;
            }
            let previous = editing.referent;
            self.end_widget_edit(previous, false, cx);
        }
        let text = self
            .plugins
            .widgets
            .text_boxes
            .get(&id)
            .and_then(|boxes| boxes.get(&referent))
            .cloned()
            .unwrap_or_default();
        let field = cx.new(|cx| InputState::new(window, cx).default_value(text));
        let events = cx.subscribe(
            &field,
            move |shell, field, event: &InputEvent, cx| match event {
                InputEvent::Change => {
                    let text = field.read(cx).value().to_string();
                    shell.send_gui(referent, GuiEventKind::TextChanged(text), cx);
                }
                InputEvent::PressEnter { .. } => shell.end_widget_edit(referent, true, cx),
                InputEvent::Blur => shell.end_widget_edit(referent, false, cx),
                _ => {}
            },
        );
        field.update(cx, |state, cx| state.focus(window, cx));
        self.plugins.widgets.editing = Some(Editing {
            widget: id,
            referent,
            field,
            _events: events,
        });
        self.send_gui(referent, GuiEventKind::Focused, cx);
        cx.notify();
    }

    fn end_widget_edit(&mut self, referent: Ref, enter: bool, cx: &mut Context<Self>) {
        if self
            .plugins
            .widgets
            .editing
            .as_ref()
            .is_none_or(|e| e.referent != referent)
        {
            return;
        }
        self.plugins.widgets.editing = None;
        self.send_gui(referent, GuiEventKind::FocusLost { enter }, cx);
        cx.notify();
    }
}
