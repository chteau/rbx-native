//! Plugin `ScreenGui`s parented to CoreGui, drawn over the 3D view with a
//! transparent background. Only buttons, text boxes, scrolling frames and
//! `Active` objects take the pointer; everywhere else it reaches the view
//! underneath.

use gpui_kit::*;
use rbx_lua::plugin::{UiId, WidgetKind};

use super::widgets::{pixel_size, Pointer};
use crate::shell::Shell;

impl Shell {
    /// One full-size layer per enabled overlay widget, lowest
    /// `DisplayOrder` first.
    pub(in crate::shell) fn plugin_viewport_overlays(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut overlays: Vec<(UiId, u64, _)> = self
            .plugins
            .ui
            .widgets
            .iter()
            .filter(|w| w.enabled)
            .filter_map(|w| match w.kind {
                WidgetKind::Overlay { display_order } => Some((w.id, w.revision, display_order)),
                _ => None,
            })
            .collect();
        overlays.sort_by_key(|o| o.2);
        overlays
            .into_iter()
            .map(|(id, revision, _)| {
                let measured = self
                    .plugins
                    .widgets
                    .areas
                    .get(&id)
                    .and_then(|area| area.get());
                if let Some(bounds) = measured {
                    self.request_widget_render(id, revision, pixel_size(bounds), cx);
                }
                self.overlay_layer(id, cx)
            })
            .collect()
    }

    fn over_sink(&self, id: UiId, position: Point<Pixels>) -> bool {
        let Some(area) = self.plugins.widgets.areas.get(&id).and_then(|a| a.get()) else {
            return false;
        };
        let x = f32::from(position.x - area.origin.x);
        let y = f32::from(position.y - area.origin.y);
        self.plugins
            .widgets
            .drawn
            .get(&id)
            .is_some_and(|drawn| super::hit::over_sink(&drawn.boxes, &drawn.sinks, x, y))
    }

    fn overlay_layer(&mut self, id: UiId, cx: &mut Context<Self>) -> AnyElement {
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
        let image = self.plugins.widgets.drawn.get(&id).map(|drawn| {
            img(drawn.image.clone())
                .absolute()
                .left(px(0.))
                .top(px(0.))
                .w(px(drawn.size.0 as f32))
                .h(px(drawn.size.1 as f32))
        });
        div()
            .id(ElementId::Name(format!("plugin-overlay-{id}").into()))
            .absolute()
            .inset_0()
            .overflow_hidden()
            .child(measure)
            .children(image)
            .on_mouse_move(cx.listener(move |shell, e: &MouseMoveEvent, window, cx| {
                shell.widget_pointer(id, e.position, Pointer::Move, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |shell, e: &MouseDownEvent, window, cx| {
                    if shell.over_sink(id, e.position) {
                        cx.stop_propagation();
                    }
                    shell.widget_pointer(id, e.position, Pointer::Down(1), window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |shell, e: &MouseDownEvent, window, cx| {
                    if shell.over_sink(id, e.position) {
                        cx.stop_propagation();
                    }
                    shell.widget_pointer(id, e.position, Pointer::Down(2), window, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |shell, e: &MouseUpEvent, window, cx| {
                    if shell.over_sink(id, e.position) {
                        cx.stop_propagation();
                    }
                    shell.widget_pointer(id, e.position, Pointer::Up(1), window, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(move |shell, e: &MouseUpEvent, window, cx| {
                    if shell.over_sink(id, e.position) {
                        cx.stop_propagation();
                    }
                    shell.widget_pointer(id, e.position, Pointer::Up(2), window, cx);
                }),
            )
            .on_scroll_wheel(cx.listener(move |shell, e: &ScrollWheelEvent, window, cx| {
                if shell.over_sink(id, e.position) {
                    cx.stop_propagation();
                }
                let dy = -f32::from(e.delta.pixel_delta(px(20.)).y);
                shell.widget_pointer(id, e.position, Pointer::Scroll(dy), window, cx);
            }))
            .on_hover(cx.listener(move |shell, hovered: &bool, _, cx| {
                if !*hovered {
                    shell.widget_left(id, cx);
                }
            }))
            .into_any_element()
    }
}
