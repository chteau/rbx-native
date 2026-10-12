//! The bottom panel's draggable seams and its horizontal scrollbar: the
//! seam over the panel (its height against the 3D view), the seam beside
//! the channel names, and the bar under the keys that pans the timeline.
//!
//! All three gestures are driven from the window root's mouse handlers
//! (`anim_seam_drag`, `anim_seam_release`): a div's own `on_mouse_move`
//! only fires while the pointer is over it, and a seam is four pixels wide.

use gpui_kit::*;

use super::{split, Gesture, Shell};
use crate::shell::chrome;
use crate::timeline::scroll::Span;
use crate::tokens;

/// The scrollbar's height, and its thumb's within it.
pub(super) const BAR: f32 = 12.0;
const THUMB_H: f32 = 6.0;

impl Shell {
    /// The key area's left edge and width: the timeline's own frame, which
    /// the scrollbar's track lines up with.
    fn key_area(&self) -> Option<(f32, f32)> {
        let b = self.anim.sheet.get()?;
        Some((f32::from(b.origin.x), f32::from(b.size.width)))
    }

    /// The names column's width, kept clear of the keys in a narrow panel.
    pub(super) fn names_w(&self) -> f32 {
        match self.anim.bottom_bounds.get() {
            Some(b) => split::names_width(self.anim.names_width, f32::from(b.size.width)),
            None => self.anim.names_width,
        }
    }

    /// The range the scrollbar stands for right now.
    fn scroll_span(&self, width: f32) -> Span {
        self.anim
            .timeline
            .scroll_span(self.anim.editor.clip.length(), width)
    }

    /// Keeps a pan or zoom inside the clip and a little past its End.
    pub(super) fn clamp_timeline(&mut self) {
        let Some((_, width)) = self.key_area() else {
            return;
        };
        let span = self
            .anim
            .timeline
            .content_span(self.anim.editor.clip.length());
        self.anim.timeline.clamp_to(span, width);
    }

    /// The window root's mouse move: moves whichever seam or thumb is held.
    pub(in crate::shell) fn anim_seam_drag(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(gesture) = self.anim.gesture else {
            return;
        };
        match gesture {
            Gesture::Split { origin_y, height } => {
                let Some(room) = self.anim_split_room() else {
                    return;
                };
                let want = height - (f32::from(at.y) - origin_y);
                self.anim.sheet_height = Some(split::sheet_height(want, room));
            }
            Gesture::Names { origin_x, width } => {
                let total = self
                    .anim
                    .bottom_bounds
                    .get()
                    .map_or(800.0, |b| f32::from(b.size.width));
                let want = width + (f32::from(at.x) - origin_x);
                self.anim.names_width = split::names_width(want, total);
            }
            Gesture::Thumb { grab, span } => {
                let Some((left, width)) = self.key_area() else {
                    return;
                };
                let tl = self.anim.timeline;
                let thumb = tl.thumb(span, width, width);
                let x = f32::from(at.x) - left - grab;
                self.anim.timeline.start = tl.start_for_thumb(span, width, width, thumb.w, x);
            }
            _ => return,
        }
        cx.notify();
    }

    /// The window root's mouse up: a release anywhere ends the gesture.
    pub(in crate::shell) fn anim_seam_release(&mut self, cx: &mut Context<Self>) {
        self.sheet_release(cx);
    }

    /// The horizontal scrollbar under the keys: a spacer under the names,
    /// then the track. The thumb is the visible share of the range (the
    /// clip, a little before frame 0 and past its End); dragging it pans,
    /// a click beside it pages.
    pub(super) fn anim_hscroll(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (thumb, held) = match self.key_area() {
            Some((_, width)) => {
                let span = match self.anim.gesture {
                    Some(Gesture::Thumb { span, .. }) => span,
                    _ => self.scroll_span(width),
                };
                (
                    Some(self.anim.timeline.thumb(span, width, width)),
                    matches!(self.anim.gesture, Some(Gesture::Thumb { .. })),
                )
            }
            None => (None, false),
        };
        div()
            .flex()
            .flex_none()
            .h(px(BAR))
            .border_t(px(1.))
            .border_color(tokens::border())
            .bg(tokens::chrome())
            .child(
                div()
                    .w(px(self.names_w()))
                    .h_full()
                    .flex_none()
                    .border_r(px(1.))
                    .border_color(tokens::border()),
            )
            .child(
                div()
                    .id("anim-hscroll")
                    .relative()
                    .flex_1()
                    .h_full()
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |b, _, window, _| {
                                let Some(thumb) = thumb else { return };
                                let top = b.origin.y + (b.size.height - px(THUMB_H)) / 2.0;
                                let colour = if held {
                                    tokens::text_muted()
                                } else {
                                    tokens::border2()
                                };
                                window.paint_quad(
                                    fill(
                                        Bounds {
                                            origin: point(b.origin.x + px(thumb.x), top),
                                            size: size(px(thumb.w), px(THUMB_H)),
                                        },
                                        colour,
                                    )
                                    .corner_radii(px(THUMB_H / 2.0)),
                                );
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|shell, e: &MouseDownEvent, _, cx| {
                            shell.hscroll_press(f32::from(e.position.x));
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
            )
    }

    /// A press on the scrollbar: on the thumb it grabs it, beside it pages.
    pub(super) fn hscroll_press(&mut self, at_x: f32) {
        let Some((left, width)) = self.key_area() else {
            return;
        };
        let span = self.scroll_span(width);
        let thumb = self.anim.timeline.thumb(span, width, width);
        let x = at_x - left;
        if thumb.contains(x) {
            self.anim.gesture = Some(Gesture::Thumb {
                grab: x - thumb.x,
                span,
            });
        } else {
            self.anim.timeline.page(span, width, x > thumb.x);
        }
    }

    /// The seam between the channel names and the keys, laid over both so
    /// it takes no room: it sits across the names column's right border.
    pub(super) fn anim_names_seam(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let hit = f32::from(chrome::HANDLE_HIT);
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(self.names_w() - hit / 2.0))
            .w(chrome::HANDLE_HIT)
            .child(chrome::seam_handle(
                "anim-names-seam",
                true,
                None,
                cx.listener(|shell, e: &MouseDownEvent, _, cx| {
                    shell.anim.gesture = Some(Gesture::Names {
                        origin_x: f32::from(e.position.x),
                        width: shell.names_w(),
                    });
                    cx.stop_propagation();
                    cx.notify();
                }),
            ))
    }
}
