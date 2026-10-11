//! The dope sheet, Blender's layout: a ruler of frames along the top, the
//! channel list down the left (a summary row, then each joint in the rig's
//! hierarchy), and each joint's keys as diamonds on its row. Click a key to
//! select it (Shift adds), drag to move, click or drag on the ruler to scrub,
//! wheel to zoom, Shift+wheel or middle-drag to pan.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Gesture, Shell};

mod draw;
mod input;

use crate::anim_editor::{rows, Row};
use crate::timeline::{self, paint};
use crate::tokens;
use draw::paint_rows;

pub(super) const NAMES_WIDTH: f32 = 196.0;
const RULER: f32 = 26.0;
const ROW: f32 = 20.0;
const KEY: f32 = 4.5;

/// What the sheet paints, copied out of the editor for the paint closure.
#[derive(Clone)]
struct Lines {
    rows: Vec<(Vec<f32>, Vec<bool>)>,
    summary: Vec<(f32, bool)>,
    selected_row: Option<usize>,
    length: f32,
}

impl Shell {
    fn sheet_rows(&self) -> Vec<Row> {
        rows(
            self.anim.bound.as_ref().map(|b| &b.rig),
            &self.anim.editor.clip,
        )
    }

    fn sheet_lines(&self, rows: &[Row]) -> Lines {
        let e = &self.anim.editor;
        let summary = e
            .clip
            .keyframes
            .iter()
            .map(|k| {
                (
                    k.time,
                    k.poses.keys().all(|j| e.is_selected(j, k.time)) && !k.poses.is_empty(),
                )
            })
            .collect();
        Lines {
            rows: rows
                .iter()
                .map(|r| {
                    let keys: Vec<f32> = crate::anim_editor::keys::times(&e.clip, &r.name);
                    let sel = keys.iter().map(|t| e.is_selected(&r.name, *t)).collect();
                    (keys, sel)
                })
                .collect(),
            summary,
            selected_row: e
                .joint
                .as_ref()
                .and_then(|j| rows.iter().position(|r| &r.name == j)),
            length: e.clip.length(),
        }
    }

    pub(super) fn dope_sheet(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.sheet_rows();
        let lines = self.sheet_lines(&rows);
        let timeline = self.anim.timeline;
        let time = self.anim.editor.time;
        let fps = self.anim.editor.fps;
        let ruler = self.anim_ruler(cx);

        let names = rows.iter().enumerate().map(|(i, row)| {
            let selected = lines.selected_row == Some(i);
            let name = row.name.clone();
            let face = crate::anim_editor::keys::face_channel(&row.name).map(str::to_owned);
            div()
                .id(("anim-channel", i))
                .h(px(ROW))
                .flex_none()
                .flex()
                .items_center()
                .pl(px(10.0 + row.depth as f32 * 12.0))
                .pr(px(6.0))
                .gap(px(6.0))
                .text_size(tokens::text_sm())
                .text_color(if row.in_rig {
                    tokens::text_strong()
                } else {
                    tokens::warning()
                })
                .when(selected, |this| this.bg(tokens::accent_soft()))
                .cursor_pointer()
                .hover(|this| this.bg(tokens::hover_subtle()))
                .child(
                    component::Icon::new(if face.is_some() {
                        IconName::ScanFace
                    } else {
                        IconName::Bone
                    })
                    .size(px(12.))
                    .text_color(tokens::text_muted()),
                )
                .child(
                    div()
                        .truncate()
                        .child(face.map_or(row.name.clone(), |c| format!("Face · {c}"))),
                )
                .when(!row.in_rig, |this| {
                    this.tooltip(|window, cx| {
                        super::super::tooltip::text(
                            "Not a joint of this rig: it will not play",
                            window,
                            cx,
                        )
                    })
                })
                .on_click(cx.listener(move |shell, _, window, cx| {
                    shell.focus_channel(&name, cx);
                    window.focus(&shell.anim.focus, cx);
                }))
        });

        let sheet_report = self.anim.sheet.clone();
        let body_lines = lines.clone();
        let body_height = (rows.len() + 1) as f32 * ROW;

        let keys = div()
            .id("anim-keys")
            .relative()
            .flex_1()
            .h(px(body_height))
            .child(
                canvas(
                    move |b, _, _| sheet_report.set(Some(b)),
                    move |b, _, window, _| paint_rows(window, b, &timeline, &body_lines, time),
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|shell, e: &MouseDownEvent, window, cx| {
                    window.focus(&shell.anim.focus, cx);
                    shell.sheet_press(e.position, e.modifiers.shift, e.click_count, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|shell, e: &MouseDownEvent, _, _| {
                    shell.anim.gesture = Some(Gesture::Pan {
                        last_x: f32::from(e.position.x),
                    });
                }),
            );

        let summary_label = format!("Summary · {}", self.anim.editor.clip.name);
        div()
            .id("anim-sheet")
            .size_full()
            .flex()
            .flex_col()
            .bg(tokens::dock())
            .track_focus(&self.anim.focus)
            .on_key_down(cx.listener(|shell, e: &KeyDownEvent, window, cx| {
                if shell.sheet_key(&e.keystroke, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_move(
                cx.listener(|shell, e: &MouseMoveEvent, _, cx| shell.sheet_drag(e.position, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|shell, _, _, cx| shell.sheet_release(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|shell, _, _, cx| shell.sheet_release(cx)),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|shell, _, _, cx| shell.sheet_release(cx)),
            )
            .on_scroll_wheel(cx.listener(|shell, e: &ScrollWheelEvent, window, cx| {
                shell.sheet_wheel(e, window, cx);
            }))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .child(
                        div()
                            .w(px(NAMES_WIDTH))
                            .h(px(RULER))
                            .flex_none()
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .bg(tokens::chrome())
                            .border_b(px(1.))
                            .border_r(px(1.))
                            .border_color(tokens::border())
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text_muted())
                            .child(format!("{} fps", fps)),
                    )
                    .child(ruler),
            )
            .child(
                div()
                    .id("anim-sheet-body")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .child(
                                div()
                                    .w(px(NAMES_WIDTH))
                                    .flex_none()
                                    .border_r(px(1.))
                                    .border_color(tokens::border())
                                    .child(
                                        div()
                                            .h(px(ROW))
                                            .flex()
                                            .items_center()
                                            .px(px(8.))
                                            .gap(px(6.))
                                            .text_size(tokens::text_sm())
                                            .text_color(tokens::text_label())
                                            .bg(tokens::wash_faint())
                                            .child(
                                                component::Icon::new(IconName::Film).size(px(12.)),
                                            )
                                            .child(div().truncate().child(summary_label)),
                                    )
                                    .children(names),
                            )
                            .child(keys),
                    ),
            )
            .into_any_element()
    }

    /// The ruler of frames over the dope sheet and the Curve Editor alike:
    /// ticks, event markers, the playhead and its frame. Click or drag on it
    /// to scrub.
    pub(super) fn anim_ruler(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let timeline = self.anim.timeline;
        let time = self.anim.editor.time;
        let ruler_bounds: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::new(Cell::new(None));
        let width = self
            .anim
            .sheet
            .get()
            .map_or(800.0, |b| f32::from(b.size.width));
        let ticks = timeline.ticks(width);
        let fps = self.anim.editor.fps;
        let markers: Vec<f32> = self
            .anim
            .editor
            .clip
            .keyframes
            .iter()
            .filter(|k| !k.markers.is_empty())
            .map(|k| k.time)
            .collect();

        let report_ruler = ruler_bounds.clone();
        div()
            .id("anim-ruler")
            .relative()
            .h(px(RULER))
            .flex_1()
            .bg(tokens::chrome())
            .border_b(px(1.))
            .border_color(tokens::border())
            .overflow_hidden()
            .child(
                canvas(
                    move |b, _, _| report_ruler.set(Some(b)),
                    move |b, _, window, _| {
                        let origin = b.origin;
                        let (ox, oy) = (f32::from(origin.x), f32::from(origin.y));
                        paint::ticks(window, origin, &ticks, RULER, 0.0);
                        for m in &markers {
                            let x = ox + timeline.x_of(*m);
                            paint::diamond(
                                window,
                                x,
                                oy + 6.0,
                                3.0,
                                tokens::warning(),
                                tokens::black(),
                            );
                        }
                        paint::playhead(window, origin, &timeline, time, RULER);
                    },
                )
                .absolute()
                .size_full(),
            )
            .children(
                timeline
                    .ticks(width)
                    .into_iter()
                    .filter(|t| t.major)
                    .map(|t| {
                        div()
                            .absolute()
                            .top(px(3.))
                            .left(px(t.x + 3.))
                            .text_size(tokens::text_xs())
                            .text_color(tokens::text_muted())
                            .child(timeline::label(t.frame, fps, false))
                    }),
            )
            .child(
                div()
                    .absolute()
                    .top(px(3.))
                    .left(px(timeline.x_of(time) - 14.))
                    .w(px(28.))
                    .h(px(18.))
                    .rounded(px(4.))
                    .bg(tokens::check_on())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(tokens::text_xs())
                    .text_color(tokens::avatar_on_accent())
                    .child(self.anim.editor.frame().to_string()),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |shell, e: &MouseDownEvent, window, cx| {
                    window.focus(&shell.anim.focus, cx);
                    if let Some(b) = ruler_bounds.get() {
                        let x = f32::from(e.position.x - b.origin.x);
                        shell.anim.gesture = Some(Gesture::Scrub);
                        shell.anim.editor.playing = false;
                        shell.anim.editor.set_time(shell.anim.timeline.time_at(x));
                        cx.notify();
                    }
                }),
            )
    }
}
