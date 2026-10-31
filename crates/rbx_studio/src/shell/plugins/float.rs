//! Plugin dock widgets whose `InitialDockState` is `Float`: a panel over the
//! editor with a title bar, as Studio opens them in a floating window.
//! Dragging the bar moves it, the corner grip resizes it (never below the
//! widget's minimum), × closes it, and where it was left comes back next
//! session (the host keeps it in the plugin's settings).

use std::collections::HashMap;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use rbx_lua::plugin::{Input, UiId, WidgetKind};

use crate::shell::Shell;
use crate::tokens;

/// `Enum.InitialDockState.Float`.
const FLOAT: u8 = 4;
/// The title bar's height.
const BAR: f32 = 22.0;
/// What the 1 px frame and the title bar take off the window: Studio 0.743
/// gives a 300 x 200 `FloatingXSize`/`YSize` widget an `AbsoluteSize` of
/// 298 x 176.
const FRAME: (f32, f32) = (2.0, BAR + 2.0);
/// Window size for a widget whose info gives neither a size nor a minimum.
const DEFAULT_SIZE: (f32, f32) = (300.0, 200.0);

/// `[x, y, width, height]` in window pixels; the size is the content's,
/// the title bar sits above it.
type Rect = [f32; 4];

/// An open floating widget: id, revision, title, floating size, minimum.
type Open = (UiId, u64, String, (f32, f32), (f32, f32));

#[derive(Clone, Copy)]
struct Grab {
    widget: UiId,
    resize: bool,
    start: Point<Pixels>,
    rect: Rect,
    min: (f32, f32),
}

#[derive(Default)]
pub(crate) struct Floats {
    rects: HashMap<UiId, Rect>,
    grab: Option<Grab>,
    /// Open widgets back to front: one that opens or is clicked comes to
    /// the front, as Studio's windows do.
    order: Vec<UiId>,
}

/// `open` reordered back to front by `order`, which first drops widgets
/// that closed and puts newly opened ones in front.
fn stack(order: &mut Vec<UiId>, open: &mut [Open]) {
    order.retain(|id| open.iter().any(|w| w.0 == *id));
    for w in open.iter() {
        if !order.contains(&w.0) {
            order.push(w.0);
        }
    }
    open.sort_by_key(|w| order.iter().position(|id| *id == w.0));
}

pub(super) fn is_float(kind: &WidgetKind) -> bool {
    matches!(kind, WidgetKind::Dock { initial: FLOAT, .. })
}

/// The content size a floating widget opens at: a `FloatingXSize` x
/// `FloatingYSize` window (never below the minimum) less its frame.
fn content_size(float: (f32, f32), min: (f32, f32)) -> (f32, f32) {
    let pick = |f: f32, m: f32, fallback: f32| {
        if f > 0.0 {
            f.max(m)
        } else if m > 0.0 {
            m
        } else {
            fallback
        }
    };
    (
        pick(float.0, min.0, DEFAULT_SIZE.0) - FRAME.0,
        pick(float.1, min.1, DEFAULT_SIZE.1) - FRAME.1,
    )
}

/// Centred in a `window`-sized viewport.
fn centered(window: (f32, f32), size: (f32, f32)) -> Rect {
    [
        ((window.0 - size.0 - FRAME.0) / 2.0).max(0.0).round(),
        ((window.1 - size.1 - FRAME.1) / 2.0).max(0.0).round(),
        size.0,
        size.1,
    ]
}

/// `rect` after the pointer moved by `(dx, dy)`: moved, or resized from the
/// bottom-right corner down to `min`.
fn dragged(rect: Rect, resize: bool, (dx, dy): (f32, f32), min: (f32, f32)) -> Rect {
    if resize {
        [
            rect[0],
            rect[1],
            (rect[2] + dx).max((min.0 - FRAME.0).max(40.0)),
            (rect[3] + dy).max((min.1 - FRAME.1).max(20.0)),
        ]
    } else {
        [rect[0] + dx, (rect[1] + dy).max(0.0), rect[2], rect[3]]
    }
}

impl Shell {
    fn float_raise(&mut self, widget: UiId) {
        let order = &mut self.plugins.floats.order;
        order.retain(|id| *id != widget);
        order.push(widget);
    }

    fn float_grab(&mut self, widget: UiId, resize: bool, start: Point<Pixels>, min: (f32, f32)) {
        if let Some(&rect) = self.plugins.floats.rects.get(&widget) {
            self.plugins.floats.grab = Some(Grab {
                widget,
                resize,
                start,
                rect,
                min,
            });
        }
    }

    /// Floating dock widgets, plus a pointer-capturing layer while one is
    /// being moved or resized.
    pub(super) fn float_widget_overlays(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let alive: Vec<UiId> = self.plugins.ui.widgets.iter().map(|w| w.id).collect();
        self.plugins.floats.rects.retain(|id, _| alive.contains(id));
        let open: Vec<Open> = self
            .plugins
            .ui
            .widgets
            .iter()
            .filter(|w| w.enabled)
            .filter_map(|w| match w.kind {
                WidgetKind::Dock {
                    initial: FLOAT,
                    float_size,
                    min_size,
                } => Some((w.id, w.revision, w.title.clone(), float_size, min_size)),
                _ => None,
            })
            .collect();
        let mut open = open;
        stack(&mut self.plugins.floats.order, &mut open);
        let viewport = window.viewport_size();
        let viewport = (f32::from(viewport.width), f32::from(viewport.height));
        let mut overlays = Vec::new();
        for (id, revision, title, float, min) in open {
            let saved = self.plugins.host.as_ref().and_then(|h| h.widget_rect(id));
            let rect = *self.plugins.floats.rects.entry(id).or_insert_with(|| {
                saved.unwrap_or_else(|| centered(viewport, content_size(float, min)))
            });
            let size = (rect[2].round() as u32, rect[3].round() as u32);
            self.request_widget_render(id, revision, size, cx);
            let surface = self.widget_surface(id, window, cx);
            let close = div()
                .id(ElementId::Name(format!("plugin-float-close-{id}").into()))
                .px(px(6.))
                .absolute()
                .right(px(2.))
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
            let bar = h_flex()
                .relative()
                .h(px(BAR))
                .w_full()
                .justify_center()
                .bg(tokens::chrome())
                .border_b_1()
                .border_color(tokens::border())
                .text_color(tokens::text())
                .text_size(px(12.))
                .cursor_move()
                .child(title)
                .child(close)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |shell, event: &MouseDownEvent, _, cx| {
                        shell.float_grab(id, false, event.position, min);
                        cx.notify();
                    }),
                );
            let grip = div()
                .absolute()
                .right(px(0.))
                .bottom(px(0.))
                .size(px(12.))
                .cursor(CursorStyle::ResizeUpLeftDownRight)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |shell, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        shell.float_grab(id, true, event.position, min);
                        cx.notify();
                    }),
                );
            let card = v_flex()
                .relative()
                .w(px(rect[2] + FRAME.0))
                .h(px(rect[3] + FRAME.1))
                .bg(tokens::dock())
                .border_1()
                .border_color(tokens::border())
                .shadow_lg()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |shell, _: &MouseDownEvent, _, cx| {
                        shell.float_raise(id);
                        cx.notify();
                    }),
                )
                .child(bar)
                .child(
                    div()
                        .relative()
                        .w(px(rect[2]))
                        .h(px(rect[3]))
                        .child(surface),
                )
                .child(grip);
            overlays.push(
                deferred(
                    anchored()
                        .position(point(px(rect[0]), px(rect[1])))
                        .child(card),
                )
                .into_any_element(),
            );
        }
        if let Some(grab) = self.plugins.floats.grab {
            let capture = div()
                .w(px(viewport.0))
                .h(px(viewport.1))
                .occlude()
                .on_mouse_move(cx.listener(move |shell, event: &MouseMoveEvent, _, cx| {
                    let delta = event.position - grab.start;
                    let moved = (f32::from(delta.x), f32::from(delta.y));
                    let rect = dragged(grab.rect, grab.resize, moved, grab.min).map(f32::round);
                    shell.plugins.floats.rects.insert(grab.widget, rect);
                    cx.notify();
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |shell, _: &MouseUpEvent, _, cx| {
                        shell.plugins.floats.grab = None;
                        let rect = shell.plugins.floats.rects.get(&grab.widget).copied();
                        if let (Some(host), Some(rect)) = (shell.plugins.host.as_mut(), rect) {
                            host.set_widget_rect(grab.widget, rect);
                        }
                        cx.notify();
                    }),
                );
            overlays.push(
                deferred(anchored().position(point(px(0.), px(0.))).child(capture))
                    .with_priority(2)
                    .into_any_element(),
            );
        }
        overlays
    }
}

#[cfg(test)]
mod tests {
    use super::{centered, content_size, dragged, stack, DEFAULT_SIZE, FRAME};

    #[test]
    fn opens_at_the_floating_size_less_the_frame_like_studio() {
        // Studio 0.743: 300 x 200 gives AbsoluteSize 298 x 176, 400 x 250
        // gives 398 x 226.
        assert_eq!(content_size((300.0, 200.0), (150.0, 100.0)), (298.0, 176.0));
        assert_eq!(content_size((400.0, 250.0), (0.0, 0.0)), (398.0, 226.0));
        assert_eq!(content_size((180.0, 150.0), (200.0, 100.0)), (198.0, 126.0));
        let default = (DEFAULT_SIZE.0 - FRAME.0, DEFAULT_SIZE.1 - FRAME.1);
        assert_eq!(content_size((0.0, 0.0), (0.0, 0.0)), default);
    }

    #[test]
    fn centred_then_moved_and_resized() {
        let rect = centered((1600.0, 1000.0), (298.0, 176.0));
        assert_eq!(rect, [650.0, 400.0, 298.0, 176.0]);
        assert_eq!(
            dragged(rect, false, (-100.0, -500.0), (0.0, 0.0)),
            [550.0, 0.0, 298.0, 176.0]
        );
        let min = (300.0, 200.0);
        assert_eq!(
            dragged(rect, true, (50.0, -100.0), min),
            [650.0, 400.0, 348.0, 176.0]
        );
    }

    #[test]
    fn a_newly_opened_widget_goes_on_top_and_closed_ones_leave() {
        let w = |id| (id, 0, String::new(), (0.0, 0.0), (0.0, 0.0));
        let mut order = vec![3, 1];
        let mut open = vec![w(1), w(2), w(3)];
        stack(&mut order, &mut open);
        assert_eq!(order, [3, 1, 2]);
        assert_eq!(open.iter().map(|w| w.0).collect::<Vec<_>>(), [3, 1, 2]);
        let mut open = vec![w(2), w(3)];
        stack(&mut order, &mut open);
        assert_eq!(order, [3, 2]);
    }
}
