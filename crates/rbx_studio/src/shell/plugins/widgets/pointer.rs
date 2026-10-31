//! Pointer input over a widget's render: hover, presses and clicks on what
//! the boxes say is under the cursor, the wheel, and dragging a scrolling
//! frame's bar.

use gpui_kit::*;
use rbx_lua::plugin::{GuiEventKind, UiId};
use rbx_viewer::ThumbGrab;

use crate::shell::Shell;
use crate::workspace_view::input::Wheel;
use crate::workspace_view::Scroll;

#[derive(Clone, Copy)]
pub(in crate::shell::plugins) enum Pointer {
    /// `true` while the left button is held.
    Move(bool),
    Down(u8),
    Up(u8),
    Wheel(Wheel),
}

/// A scroll bar thumb being dragged: where the press landed and the canvas
/// position last asked for.
#[derive(Clone, Copy)]
pub(super) struct Grab {
    pub(super) widget: UiId,
    pub(super) thumb: ThumbGrab,
    pub(super) from: [f32; 2],
    pub(super) sent: f32,
}

impl Shell {
    pub(in crate::shell::plugins) fn widget_left(&mut self, id: UiId, cx: &mut Context<Self>) {
        if let Some((_, old)) = self.plugins.widgets.hover.filter(|(w, _)| *w == id) {
            self.plugins.widgets.hover = None;
            self.send_gui(old, GuiEventKind::Leave { x: -1.0, y: -1.0 }, cx);
        }
    }

    pub(in crate::shell::plugins) fn widget_pointer(
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
        let drawn = self.plugins.widgets.drawn.get(&id);
        let target = drawn
            .and_then(|drawn| crate::shell::plugins::hit::hit(&drawn.boxes, &drawn.sinks, x, y));
        // Pressing anywhere off the box being typed in ends it, as in Studio.
        let edited = self.plugins.widgets.edited_on(id);
        if let Some(edited) =
            edited.filter(|edited| matches!(pointer, Pointer::Down(_)) && target != Some(*edited))
        {
            self.end_widget_edit(edited, false, cx);
        }
        let drawn = self.plugins.widgets.drawn.get(&id);
        match pointer {
            Pointer::Move(held) => {
                if let Some(grab) = self.plugins.widgets.grab.filter(|g| g.widget == id) {
                    if held {
                        self.drag_thumb(grab, [x, y], cx);
                        return;
                    }
                    self.plugins.widgets.grab = None;
                }
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
                let thumb = drawn.and_then(|drawn| drawn.scrolls.thumb_at([x, y]));
                if let Some(thumb) = thumb.filter(|_| button == 1) {
                    self.plugins.widgets.grab = Some(Grab {
                        widget: id,
                        thumb,
                        from: [x, y],
                        sent: thumb.position[thumb.axis].clamp(0.0, thumb.range),
                    });
                    return;
                }
                self.plugins.widgets.pressed = target.map(|r| (id, r, button));
                if let Some(r) = target {
                    self.send_gui(r, GuiEventKind::Down { button, x, y }, cx);
                }
            }
            Pointer::Up(button) => {
                if button == 1 && self.plugins.widgets.grab.take().is_some() {
                    return;
                }
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
            Pointer::Wheel(wheel) => {
                // Studio scrolls the frame under the cursor itself and still
                // hands the notch to whatever is hovered.
                let scroll = drawn
                    .and_then(|drawn| drawn.scrolls.wheel_target([x, y], wheel.axis))
                    .map(|target| Scroll::of(target, wheel));
                if let Some(scroll) = scroll {
                    let kind = GuiEventKind::ScrollCanvas {
                        axis: scroll.axis,
                        by: scroll.delta,
                        range: scroll.range,
                    };
                    self.send_gui(scroll.referent, kind, cx);
                }
                if let Some(r) = target.filter(|_| wheel.axis == 1) {
                    let dy = -wheel.notches * 20.0;
                    self.send_gui(r, GuiEventKind::Scroll { dy }, cx);
                }
            }
        }
    }

    /// Moves a dragged thumb's canvas to follow the pointer.
    fn drag_thumb(&mut self, mut grab: Grab, at: [f32; 2], cx: &mut Context<Self>) {
        let ThumbGrab {
            referent,
            axis,
            position,
            range,
            per_pixel,
        } = grab.thumb;
        let wanted = (position[axis] + (at[axis] - grab.from[axis]) * per_pixel).clamp(0.0, range);
        if wanted == grab.sent {
            return;
        }
        let by = wanted - grab.sent;
        grab.sent = wanted;
        self.plugins.widgets.grab = Some(grab);
        self.send_gui(referent, GuiEventKind::ScrollCanvas { axis, by, range }, cx);
    }
}
